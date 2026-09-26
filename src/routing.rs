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
    pub finish_reason: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub error: Option<Value>,
    pub first_token_at: Option<Instant>,
    pub reasoning_started: Option<Instant>,
}
impl Outcome {
    pub fn metadata(&self, routing: &Routing) -> Value {
        json!({"logical_model":routing.model_id,"route":self.route.id,"upstream_model":self.route.upstream_model,"task_id":routing.task_id,"finish_reason":self.finish_reason,"input_tokens":self.input_tokens,"output_tokens":self.output_tokens,"status":if self.complete {"complete"} else {"incomplete"},"error":self.error})
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
                "模型尚未配置可用连接，请打开模型设置编辑 API 配置".into(),
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
        if let Some(input) = route.config.input_limit {
            let context = input.saturating_add(oai.openai_max_tokens);
            oai.openai_max_context = route.config.context_limit.map_or(context, |limit| context.min(limit));
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
        let started_at = Instant::now();
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
            finish_reason: None,
            input_tokens: None,
            output_tokens: None,
            error: None,
            first_token_at: None,
            reasoning_started: None,
        };
        'routes: for (index, route) in ordered.iter().enumerate() {
            outcome.route = route.clone();
            let req = match prepare_request(request, route) {
                Ok(r) => r,
                Err(e) => {
                    log_failure(&self.task_id, route, &e, 0, &outcome);
                    outcome.error = Some(diagnostic(&e, route));
                    break;
                }
            };
            for attempt in 0..2 {
                outcome.finish_reason = None;
                outcome.input_tokens = None;
                outcome.output_tokens = None;
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
                tracing::info!(event="generation_attempt", task_id=%self.task_id, logical_model=%self.model_id,
                    route=%route.id, protocol=%route.protocol, upstream_model=%route.upstream_model,
                    attempt=attempt+1, fallback=index>0, max_output=req.max_tokens, max_input=?route.config.input_limit,
                    context_limit=?route.config.context_limit, estimated_input_tokens=estimated_input(&req, route), streaming=req.stream,
                    remaining_secs=self.deadline.saturating_duration_since(tokio::time::Instant::now()).as_secs());
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
                                Ok(StreamEvent::FinishReason(reason)) => {
                                    outcome.finish_reason = Some(safe_finish_reason(&reason).into());
                                }
                                Ok(StreamEvent::Usage { input, output }) => {
                                    if input.is_some() { outcome.input_tokens = input; }
                                    if output.is_some() { outcome.output_tokens = output; }
                                }
                                Ok(StreamEvent::Done) => {
                                    let limited = matches!(outcome.finish_reason.as_deref(), Some("length" | "max_tokens" | "MAX_TOKENS"));
                                    outcome.complete = !limited;
                                    outcome.error = if limited { Some(diagnostic(&ProviderError::OutputLimit(outcome.finish_reason.clone().unwrap()), route)) } else { None };
                                    self.stage(hub, if limited { "incomplete" } else { "complete" }, route, attempt + 1);
                                    if limited {
                                        hub.emit("generation_diagnostic",json!({"task_id":self.task_id,"conversation":self.conversation,"diagnostic":outcome.error}));
                                    }
                                    log_outcome(&self.task_id, &outcome, started_at);
                                    return outcome;
                                }
                                Ok(StreamEvent::Error(e)) | Err(e) => break e,
                                _ => {}
                            }
                        }
                    }
                };
                log_failure(&self.task_id, route, &error, attempt+1, &outcome);
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
                    tracing::info!(event="generation_backoff", task_id=%self.task_id, route=%route.id, delay_ms=delay.as_millis() as u64);
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
        log_outcome(&self.task_id, &outcome, started_at);
        outcome
    }
}
fn safe_finish_reason(reason: &str) -> &str {
    match reason {
        "stop" | "length" | "content_filter" | "tool_calls" | "function_call" |
        "end_turn" | "max_tokens" | "stop_sequence" | "pause_turn" | "refusal" |
        "STOP" | "MAX_TOKENS" | "SAFETY" | "RECITATION" | "OTHER" => reason,
        _ => "unknown",
    }
}
fn log_failure(task_id: &str, route: &Route, error: &ProviderError, attempt: usize, outcome: &Outcome) {
    let detail = serde_json::to_value(error).unwrap_or(Value::Null);
    // Log typed diagnostics, never upstream bodies, prompts, URLs, headers or secrets.
    tracing::warn!(event="generation_attempt_failed", task_id, route=%route.id, protocol=%route.protocol,
        attempt, error_kind=detail["type"].as_str().unwrap_or("unknown"),
        http_status=?detail["data"]["status"].as_u64(), timeout_stage=if matches!(error, ProviderError::Timeout(_)) { detail["data"].as_str().unwrap_or("") } else { "" }, retryable=error.retryable(), retry_after=?error.retry_after(),
        input_tokens=?detail["data"]["input_tokens"].as_i64(), output_tokens=?detail["data"]["output_tokens"].as_i64(), limit=?detail["data"]["limit"].as_i64(),
        text_chars=outcome.text.chars().count(), reasoning_chars=outcome.reasoning.chars().count());
}
fn log_outcome(task_id: &str, outcome: &Outcome, started: Instant) {
    tracing::info!(event="generation_finished", task_id, route=%outcome.route.id,
        status=if outcome.complete {"complete"} else {"incomplete"}, finish_reason=?outcome.finish_reason,
        error_kind=outcome.error.as_ref().and_then(|e| e["detail"]["type"].as_str()).unwrap_or("none"),
        elapsed_ms=started.elapsed().as_millis() as u64,
        first_content_ms=outcome.first_token_at.into_iter().chain(outcome.reasoning_started).min().map(|t|t.duration_since(started).as_millis() as u64),
        text_chars=outcome.text.chars().count(), reasoning_chars=outcome.reasoning.chars().count(),
        input_tokens=?outcome.input_tokens, output_tokens=?outcome.output_tokens);
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
            .filter(|_| route.protocol == "openai")
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
fn estimated_input(req: &GenRequest, route: &Route) -> usize {
    let source = match route.protocol.as_str() { "anthropic" => "claude", "gemini" => "makersuite", _ => "custom" };
    let tokenizer = nast_engine::tokens::tokenizer_for_source(source, &req.model);
    req
        .messages
        .iter()
        .map(|m| nast_engine::tokens::count_message_tokens(&m.content, m.name.as_deref(), tokenizer))
        .sum::<usize>()
        + req
            .assistant_prefill
            .as_ref()
            .map(|p| nast_engine::tokens::count_message_tokens(p, None, tokenizer))
            .unwrap_or(0)
        + 3

}
fn prepare_request(request: &GenRequest, route: &Route) -> Result<GenRequest, ProviderError> {
    let mut req = request.clone();
    req.model = route.upstream_model.clone();
    req.extra_headers = route.config.headers.clone().into_iter().collect();
    req.extra_body = json!(route.config.parameters);
    req.max_tokens = output_tokens(route, req.max_tokens)?;
    model_catalog::validate_parameters(route, Some(req.max_tokens)).map_err(ProviderError::Config)?;
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
    let input = estimated_input(&req, route);
    if let Some(limit) = route.config.input_limit.filter(|limit| input as i64 > *limit) {
        return Err(ProviderError::InputLimit { model: req.model.clone(), input_tokens: input as i64, limit });
    }
    if let Some(limit) = route.config.context_limit.filter(|limit| (input as i64).saturating_add(req.max_tokens) > *limit) {
        return Err(ProviderError::ContextLimit { model: req.model.clone(), input_tokens: input as i64, output_tokens: req.max_tokens, limit });
    }
    Ok(req)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn route(protocol: &str) -> Route {
        Route { id: "r".into(), provider: "".into(), protocol: protocol.into(), upstream_model: "gpt-4o".into(), priority: 0, enabled: true, config: Default::default() }
    }
    fn request() -> GenRequest {
        GenRequest { messages: vec![nast_providers::ChatMessage { role: "user".into(), content: "hello".into(), name: None }], model: "gpt-4o".into(), temperature: 1.0, top_p: 1.0, frequency_penalty: 0.0, presence_penalty: 0.0, max_tokens: 300, stop: vec![], stream: true, assistant_prefill: None, use_sysprompt: true, extra_headers: vec![], extra_body: json!({}) }
    }
    fn routing(route: Route) -> Routing {
        Routing { model_id: "m".into(), conversation: Conversation::Private { avatar: "a".into(), chat_file: "c".into() }, routes: vec![route], secrets: json!({}), active: RefCell::new(None), task_id: "test".into(), deadline: tokio::time::Instant::now(), phase: Arc::new(Mutex::new(json!({}))), reasoning: Arc::new(Mutex::new(String::new())) }
    }
    #[test]
    fn explicit_output_overrides_preset_before_input_budget_is_reserved() {
        let mut route = route("openai");
        route.config.input_limit = Some(8192);
        route.config.output_limit = Some(4096);
        route.config.context_limit = Some(10000);
        route.config.parameters.insert("max_completion_tokens".into(), json!(4096));
        route.config.remove_parameters.push("max_tokens".into());
        let mut oai = OaiSettings::default();
        oai.openai_max_tokens = 300;
        routing(route.clone()).configure_prompt(&mut oai);
        assert_eq!(oai.openai_max_tokens, 4096);
        assert_eq!(oai.openai_max_context - oai.openai_max_tokens, 5904);
        route.config.context_limit = None;
        routing(route.clone()).configure_prompt(&mut oai);
        assert_eq!(oai.openai_max_context - oai.openai_max_tokens, 8192);
        assert_eq!(prepare_request(&request(), &route).unwrap().max_tokens, 4096);
    }
    #[test]
    fn frozen_input_is_rejected_by_smaller_fallback_without_mutation() {
        let req = request();
        let mut route = route("openai");
        route.config.input_limit = Some(1);
        assert!(matches!(prepare_request(&req, &route), Err(ProviderError::InputLimit { .. })));
        assert_eq!(req.messages[0].content, "hello");
        route.config.input_limit = Some(100);
        assert!(prepare_request(&req, &route).is_ok());
    }
    #[test]
    fn final_context_boundary_reports_numbers_and_never_retries() {
        let req = request();
        let mut route = route("openai");
        let input = estimated_input(&req, &route) as i64;
        route.config.context_limit = Some(input + req.max_tokens);
        assert!(prepare_request(&req, &route).is_ok());
        route.config.context_limit = Some(input + req.max_tokens - 1);
        let error = prepare_request(&req, &route).unwrap_err();
        assert!(!error.retryable());
        assert!(matches!(error, ProviderError::ContextLimit { input_tokens, output_tokens: 300, .. } if input_tokens == input));
        let value = serde_json::to_value(error).unwrap();
        assert_eq!(value["data"]["limit"], input + 299);
    }
    #[test]
    fn thinking_budgets_are_checked_against_effective_output() {
        let mut route = route("anthropic");
        route.config.parameters.insert("thinking".into(), json!({"type":"enabled","budget_tokens":1024}));
        assert!(model_catalog::validate_parameters(&route, None).is_ok());
        assert!(prepare_request(&request(), &route).is_err()); // inherited 300 is insufficient
        route.config.parameters.insert("max_tokens".into(), json!(2048));
        assert_eq!(prepare_request(&request(), &route).unwrap().max_tokens, 2048);
        route.config.parameters.insert("thinking".into(), json!({"type":"enabled","budget_tokens":2048}));
        assert!(model_catalog::validate_parameters(&route, None).is_err());
        route.protocol = "gemini".into();
        route.config.parameters.clear();
        route.config.parameters.insert("generationConfig".into(), json!({"maxOutputTokens":4096,"thinkingConfig":{"thinkingBudget":-1}}));
        assert_eq!(prepare_request(&request(), &route).unwrap().max_tokens, 4096);
        route.config.parameters.insert("generationConfig".into(), json!({"thinkingConfig":{"thinkingBudget":-1,"thinkingLevel":"HIGH"}}));
        assert!(model_catalog::validate_parameters(&route, None).is_err());
    }
}
