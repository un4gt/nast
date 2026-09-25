//! A frozen request is retried here, never in the prompt/plugin pipeline.
use crate::{
    model_catalog::{self, Conversation, Route},
    rpc::RpcError,
    state::{EventHub, SharedState},
};
use futures_util::StreamExt;
use nast_model::{chat::ChatFile, preset::OaiSettings};
use nast_providers::{GenRequest, ProviderError, StreamEvent};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub struct Routing {
    pub model_id: String,
    pub conversation: Conversation,
    routes: Vec<Route>,
    secrets: Value,
    active: RefCell<Option<String>>,
    pub task_id: String,
    deadline: tokio::time::Instant,
    pub phase: Arc<Mutex<Value>>,
    pub reasoning: Arc<Mutex<String>>,
}
pub struct Outcome {
    pub text: String,
    pub reasoning: String,
    pub route: Route,
    pub complete: bool,
    pub error: Option<Value>,
    pub first_token_at: Option<Instant>,
    pub reasoning_started: Option<Instant>,
}
impl Outcome {
    pub fn metadata(&self, routing: &Routing) -> Value {
        json!({"logical_model":routing.model_id,"route":self.route.id,"upstream_model":self.route.upstream_model,"task_id":routing.task_id,"status":if self.complete {"complete"} else {"incomplete"},"error":self.error})
    }
}
pub fn task_id(params: &Value) -> String {
    params["task_id"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 128)
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}
impl Routing {
    pub async fn prepare(
        state: &SharedState,
        conversation: Conversation,
        params: &Value,
        task_id: String,
    ) -> Result<Self, RpcError> {
        let secrets_guard = state.secrets.read().await;
        let catalog = state.catalog.lock().unwrap();
        let secrets = secrets_guard.clone();
        let mut chat = conversation.read(state)?;
        model_catalog::bind(&mut chat, &catalog.default_model, false);
        let selected = &chat.0[0]["chat_metadata"]["nast_model"];
        let model_id = selected["selected_model"].as_str().unwrap().to_string();
        let model = catalog.model(&model_id).map_err(RpcError::BadRequest)?;
        let active = selected["active_route"].as_str().map(str::to_string);
        // Keep the snapshot in canonical priority order; sticky state can change between group members.
        let routes = model_catalog::ordered_routes(model, None);
        if routes.is_empty() {
            return Err(RpcError::BadRequest(
                "模型没有可用路由，请打开模型设置启用或添加路由".into(),
            ));
        }
        conversation.write(state, &chat)?;
        let budget = params["time_budget_secs"]
            .as_u64()
            .unwrap_or(600)
            .clamp(1, 600);
        Ok(Self {
            model_id,
            conversation,
            routes,
            secrets,
            active: RefCell::new(active),
            task_id,
            deadline: tokio::time::Instant::now() + Duration::from_secs(budget),
            phase: Arc::new(Mutex::new(json!({"phase":"preparing"}))),
            reasoning: Arc::new(Mutex::new(String::new())),
        })
    }
    pub fn configure_prompt(&self, oai: &mut OaiSettings) {
        let route = self.first();
        oai.chat_completion_source = match route.protocol.as_str() {
            "anthropic" => "claude",
            "gemini" => "makersuite",
            _ => "custom",
        }
        .into();
        oai.custom_model = route.upstream_model.clone();
        oai.claude_model = route.upstream_model.clone();
        oai.google_model = route.upstream_model.clone();
        oai.openai_model = route.upstream_model.clone();
        if let Some(limit) = route.config.context_limit {
            oai.openai_max_context = limit;
        }
        if let Some(limit) = route.config.output_limit {
            oai.openai_max_tokens = oai.openai_max_tokens.min(limit);
        }
        // Route overrides determine the actual output reservation before prompt assembly.
        if let Ok(max) = output_tokens(route, oai.openai_max_tokens) {
            oai.openai_max_tokens = max;
        }
        oai.custom_include_body.clear();
        oai.custom_include_headers.clear();
    }
    fn first(&self) -> &Route {
        self.active
            .borrow()
            .as_ref()
            .and_then(|id| self.routes.iter().find(|r| &r.id == id))
            .unwrap_or(&self.routes[0])
    }
    pub fn commit(&self, chat: &mut ChatFile, outcome: &Outcome) {
        if outcome.complete {
            chat.0[0]["chat_metadata"]["nast_model"]["active_route"] = json!(outcome.route.id);
            chat.0[0]["chat_metadata"]["nast_model"]["active_route_info"] = json!({"id":outcome.route.id,"provider":outcome.route.provider,"upstream_model":outcome.route.upstream_model});
            *self.active.borrow_mut() = Some(outcome.route.id.clone());
        }
    }
    fn stage(&self, hub: &EventHub, phase: &str, route: &Route, attempt: usize) {
        let value = json!({"conversation":self.conversation,"task_id":self.task_id,"phase":phase,"route":route.id,"attempt":attempt});
        let mut current = self.phase.lock().unwrap();
        if *current != value {
            *current = value.clone();
            hub.emit("generation_route", value);
        }
    }
    pub async fn execute(
        &self,
        request: &GenRequest,
        hub: &EventHub,
        abort: &tokio_util::sync::CancellationToken,
        progress: &Arc<Mutex<String>>,
        publish: bool,
    ) -> Outcome {
        *progress.lock().unwrap() = String::new();
        self.reasoning.lock().unwrap().clear();
        let mut ordered = self.routes.clone();
        if let Some(index) = ordered.iter().position(|r| r.id == self.first().id) {
            let r = ordered.remove(index);
            ordered.insert(0, r);
        }
        let mut outcome = Outcome {
            text: String::new(),
            reasoning: String::new(),
            route: ordered[0].clone(),
            complete: false,
            error: None,
            first_token_at: None,
            reasoning_started: None,
        };
        'routes: for (index, route) in ordered.iter().enumerate() {
            outcome.route = route.clone();
            let req = match prepare_request(request, route) {
                Ok(r) => r,
                Err(e) => {
                    outcome.error = Some(diagnostic(&e, route));
                    break;
                }
            };
            for attempt in 0..2 {
                self.stage(
                    hub,
                    if attempt > 0 {
                        "retrying"
                    } else if index > 0 {
                        "fallback"
                    } else {
                        "connecting"
                    },
                    route,
                    attempt + 1,
                );
                let provider = model_catalog::provider(route, &self.secrets);
                let attempt_start = tokio::time::Instant::now();
                let first_deadline =
                    attempt_start + Duration::from_secs(route.config.first_token_timeout_secs);
                let started = tokio::select! {
                    biased;
                    _ = abort.cancelled()=>Err(ProviderError::Aborted),
                    _ = tokio::time::sleep_until(self.deadline)=>Err(ProviderError::Timeout("total budget".into())),
                    _ = tokio::time::sleep_until(first_deadline)=>Err(ProviderError::Timeout("first content".into())),
                    value = provider.generate_stream(&req)=>value,
                };
                let error = match started {
                    Err(e) => e,
                    Ok(stream) => {
                        tokio::pin!(stream);
                        let mut idle_deadline = first_deadline;
                        loop {
                            let event = tokio::select! {
                                biased;
                                _ = abort.cancelled()=>Err(ProviderError::Aborted),
                                _ = tokio::time::sleep_until(self.deadline)=>Err(ProviderError::Timeout("total budget".into())),
                                _ = tokio::time::sleep_until(idle_deadline)=>Err(ProviderError::Timeout("stream content".into())),
                                event = stream.next()=>event.unwrap_or(Err(ProviderError::Disconnected)),
                            };
                            match event {
                                Ok(StreamEvent::Token(t)) if !t.is_empty() => {
                                    outcome.first_token_at.get_or_insert_with(Instant::now);
                                    outcome.text.push_str(&t);
                                    *progress.lock().unwrap() = outcome.text.clone();
                                    if publish {
                                        hub.emit("stream_token_received",json!({"text":t,"task_id":self.task_id,"conversation":self.conversation}));
                                    }
                                    idle_deadline = tokio::time::Instant::now()
                                        + Duration::from_secs(route.config.idle_timeout_secs);
                                    self.stage(hub, "streaming", route, attempt + 1);
                                }
                                Ok(StreamEvent::Reasoning(t)) if !t.is_empty() => {
                                    outcome.reasoning_started.get_or_insert_with(Instant::now);
                                    outcome.reasoning.push_str(&t);
                                    *self.reasoning.lock().unwrap() = outcome.reasoning.clone();
                                    if publish {
                                        hub.emit("stream_reasoning_received",json!({"text":t,"task_id":self.task_id,"conversation":self.conversation}));
                                    }
                                    idle_deadline = tokio::time::Instant::now()
                                        + Duration::from_secs(route.config.idle_timeout_secs);
                                    self.stage(hub, "streaming", route, attempt + 1);
                                }
                                Ok(StreamEvent::Done) => {
                                    outcome.complete = true;
                                    outcome.error = None;
                                    self.stage(hub, "complete", route, attempt + 1);
                                    return outcome;
                                }
                                Ok(StreamEvent::Error(e)) | Err(e) => break e,
                                _ => {}
                            }
                        }
                    }
                };
                outcome.error = Some(diagnostic(&error, route));
                if !outcome.text.is_empty()
                    || !outcome.reasoning.is_empty()
                    || !error.retryable()
                    || abort.is_cancelled()
                    || tokio::time::Instant::now() >= self.deadline
                {
                    break 'routes;
                }
                if error.retry_after().is_some_and(|v| v > 30) {
                    continue 'routes;
                }
                if attempt == 0 {
                    self.stage(hub, "retrying", route, 2);
                    let delay = Duration::from_millis(
                        error.retry_after().map(|s| s * 1000).unwrap_or(1000)
                            + u64::from(rand::random::<u8>()),
                    );
                    tokio::select! {
                        biased;
                        _ = abort.cancelled()=> { outcome.error=Some(diagnostic(&ProviderError::Aborted,route)); break 'routes; },
                        _ = tokio::time::sleep_until(self.deadline)=> { outcome.error=Some(diagnostic(&ProviderError::Timeout("total budget".into()),route)); break 'routes; },
                        _ = tokio::time::sleep(delay)=>{},
                    }
                }
            }
        }
        self.stage(hub, "incomplete", &outcome.route, 0);
        hub.emit("generation_diagnostic",json!({"task_id":self.task_id,"conversation":self.conversation,"diagnostic":outcome.error}));
        outcome
    }
}
fn diagnostic(error: &ProviderError, route: &Route) -> Value {
    json!({"route":route.id,"protocol":route.protocol,"message":error.to_string(),"retryable":error.retryable(),"detail":error})
}
fn output_tokens(route: &Route, fallback: i64) -> Result<i64, ProviderError> {
    let parameters = &route.config.parameters;
    let value = if route.protocol == "gemini" {
        parameters
            .get("generationConfig")
            .and_then(|v| v.get("maxOutputTokens"))
    } else {
        parameters
            .get("max_completion_tokens")
            .filter(|_| {
                !route
                    .config
                    .remove_parameters
                    .iter()
                    .any(|k| k == "max_completion_tokens")
            })
            .or_else(|| {
                parameters.get("max_tokens").filter(|_| {
                    !route
                        .config
                        .remove_parameters
                        .iter()
                        .any(|k| k == "max_tokens")
                })
            })
    };
    let max = match value {
        Some(v) => v
            .as_i64()
            .ok_or_else(|| ProviderError::Config("output budget must be an integer".into()))?,
        None => fallback,
    };
    if max <= 0 || route.config.output_limit.is_some_and(|limit| max > limit) {
        return Err(ProviderError::Config(
            "output budget exceeds route limit".into(),
        ));
    }
    Ok(max)
}
fn prepare_request(request: &GenRequest, route: &Route) -> Result<GenRequest, ProviderError> {
    let mut req = request.clone();
    req.model = route.upstream_model.clone();
    req.extra_headers = route.config.headers.clone().into_iter().collect();
    req.extra_body = json!(route.config.parameters);
    req.max_tokens = output_tokens(route, req.max_tokens)?;
    for key in &route.config.remove_parameters {
        if model_catalog::protected(key)
            || key == "generationConfig"
            || (key == "max_tokens"
                && !(route.protocol == "openai"
                    && route
                        .config
                        .parameters
                        .contains_key("max_completion_tokens")
                    && !route
                        .config
                        .remove_parameters
                        .iter()
                        .any(|k| k == "max_completion_tokens")))
        {
            return Err(ProviderError::Config(format!(
                "cannot remove required field {key}"
            )));
        }
    }
    if route.protocol != "openai"
        && ((req.frequency_penalty != 0.0
            && !route
                .config
                .remove_parameters
                .iter()
                .any(|k| k == "frequency_penalty"))
            || (req.presence_penalty != 0.0
                && !route
                    .config
                    .remove_parameters
                    .iter()
                    .any(|k| k == "presence_penalty"))
            || req.messages.iter().any(|m| m.name.is_some()))
    {
        return Err(ProviderError::Config(
            "route cannot express penalties or named messages".into(),
        ));
    }
    if route.protocol == "gemini" && (req.assistant_prefill.is_some() || req.stop.len() > 5) {
        return Err(ProviderError::Config(
            "Gemini cannot express prefill or more than five stop sequences".into(),
        ));
    }
    let tokenizer = nast_engine::tokens::resolve_tokenizer(&req.model);
    let input = req
        .messages
        .iter()
        .map(|m| nast_engine::tokens::count_tokens(&m.content, tokenizer) + 4)
        .sum::<usize>()
        + req
            .assistant_prefill
            .as_ref()
            .map(|p| nast_engine::tokens::count_tokens(p, tokenizer))
            .unwrap_or(0)
        + 3;
    if route
        .config
        .context_limit
        .is_some_and(|limit| input as i64 + req.max_tokens > limit)
    {
        return Err(ProviderError::Config(
            "frozen prompt exceeds route context limit; choose a larger route".into(),
        ));
    }
    Ok(req)
}
