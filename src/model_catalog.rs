//! User model directory. Secret revisions are immutable: publishing models.json is the commit point.
use crate::{
    connection,
    rpc::{RpcError, RpcResult},
    state::SharedState,
};
use nast_model::{chat::ChatFile, preset::OaiSettings};
use nast_providers::{Provider, ProviderKind};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Catalog {
    pub version: u64,
    pub default_model: String,
    pub models: Vec<Model>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub display_name: String,
    pub routes: Vec<Route>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Route {
    pub id: String,
    pub provider: String,
    pub protocol: String,
    pub upstream_model: String,
    pub priority: i64,
    pub enabled: bool,
    pub config: RouteConfig,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct RouteConfig {
    pub endpoint: String,
    pub credential_ref: Option<String>,
    pub context_limit: Option<i64>,
    pub input_limit: Option<i64>,
    pub output_limit: Option<i64>,
    pub connect_timeout_secs: u64,
    pub first_token_timeout_secs: u64,
    pub idle_timeout_secs: u64,
    pub headers: BTreeMap<String, String>,
    pub parameters: BTreeMap<String, Value>,
    pub remove_parameters: Vec<String>,
}
impl Default for RouteConfig {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            credential_ref: None,
            context_limit: None,
            input_limit: None,
            output_limit: None,
            connect_timeout_secs: 10,
            first_token_timeout_secs: 60,
            idle_timeout_secs: 60,
            headers: BTreeMap::new(),
            parameters: BTreeMap::new(),
            remove_parameters: vec![],
        }
    }
}
fn bad(s: impl ToString) -> RpcError {
    RpcError::BadRequest(s.to_string())
}
pub fn protected(key: &str) -> bool {
    matches!(
        key,
        "model" | "messages" | "stream" | "contents" | "system" | "systemInstruction"
    )
}
/// Validate explicit budgets at save time and the effective output again before sending.
/// Omitted limits inherit the active preset, so that relationship is checked at runtime.
pub fn validate_parameters(route: &Route, effective_output: Option<i64>) -> Result<(), String> {
    let params = &route.config.parameters;
    let get = |key: &str| params.get(key).filter(|_| !route.config.remove_parameters.iter().any(|k| k == key));
    let explicit = if route.protocol == "gemini" {
        get("generationConfig").and_then(|v| v.get("maxOutputTokens"))
    } else if route.protocol == "openai" {
        get("max_completion_tokens").or_else(|| get("max_tokens"))
    } else {
        get("max_tokens")
    };
    let output = match explicit {
        Some(value) => Some(value.as_i64().filter(|v| *v > 0).ok_or("最大输出 Token 必须为正整数")?),
        None => effective_output,
    };
    if let Some(output) = output {
        if output <= 0 || route.config.output_limit.is_some_and(|limit| output > limit) {
            return Err("最大输出超过模型配置的输出限制".into());
        }
        if route.config.context_limit.is_some_and(|limit| output >= limit) {
            return Err("最大输出必须小于上下文窗口，为输入留出空间".into());
        }
    }
    if route.protocol == "anthropic" {
        if let Some(thinking) = get("thinking") {
            match thinking["type"].as_str() {
                Some("enabled") => {
                    let budget = thinking["budget_tokens"].as_i64().filter(|v| *v >= 1024)
                        .ok_or("Anthropic 思考预算必须为至少 1024 的整数")?;
                    if output.or(route.config.output_limit).is_some_and(|limit| budget >= limit) {
                        return Err("Anthropic 思考预算必须小于最大输出 Token".into());
                    }
                }
                Some("adaptive" | "disabled") => {
                    if thinking.get("budget_tokens").is_some() {
                        return Err("仅指定思考预算模式可设置 budget_tokens".into());
                    }
                }
                _ => return Err("无效的 Anthropic 思考方式".into()),
            }
        }
    }
    if route.protocol == "gemini" {
        if let Some(thinking) = get("generationConfig").and_then(|v| v.get("thinkingConfig")) {
            if let Some(budget) = thinking.get("thinkingBudget") {
                if budget.as_i64().is_none_or(|v| v < -1) {
                    return Err("Gemini 思考预算须为非负整数，或用 -1 表示自动".into());
                }
                if thinking.get("thinkingLevel").is_some() {
                    return Err("Gemini 思考级别与思考预算不能同时设置".into());
                }
            }
        }
    }
    Ok(())
}

impl Catalog {
    pub fn validate(&self) -> Result<(), String> {
        let mut ids = HashSet::new();
        let mut routes = HashSet::new();
        for m in &self.models {
            if !valid_id(&m.id) || !ids.insert(&m.id) || m.display_name.trim().is_empty() {
                return Err("模型 ID 必须唯一且名称不能为空".into());
            }
            for r in &m.routes {
                if !valid_id(&r.id) || !routes.insert(&r.id) {
                    return Err("路由 ID 必须全局唯一".into());
                }
                if !["openai", "anthropic", "gemini"].contains(&r.protocol.as_str()) {
                    return Err("未知协议".into());
                }
                let url = reqwest::Url::parse(&r.config.endpoint).map_err(|_| "无效端点")?;
                if !["http", "https"].contains(&url.scheme())
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.query().is_some()
                    || url.fragment().is_some()
                {
                    return Err("端点须为不含凭据、查询或片段的 HTTP(S) URL".into());
                }
                if r.upstream_model.trim().is_empty() {
                    return Err("请填写上游模型 ID".into());
                }
                if r.config.context_limit.is_some_and(|v| v <= 0)
                    || r.config.input_limit.is_some_and(|v| v <= 0)
                    || r.config.output_limit.is_some_and(|v| v <= 0)
                    || [
                        r.config.connect_timeout_secs,
                        r.config.first_token_timeout_secs,
                        r.config.idle_timeout_secs,
                    ]
                    .iter()
                    .any(|v| *v == 0 || *v > 600)
                {
                    return Err("限制与超时须为正数，超时最多 600 秒".into());
                }
                if r.config.context_limit.zip(r.config.output_limit).is_some_and(|(context, output)| output >= context) {
                    return Err("最大输出必须小于上下文窗口，为输入留出空间".into());
                }
                validate_parameters(r, None)?;
                for k in r
                    .config
                    .parameters
                    .keys()
                    .chain(r.config.remove_parameters.iter())
                {
                    if protected(k) {
                        return Err(format!("不能覆盖或移除路由字段 {k}"));
                    }
                }
                for (k, v) in &r.config.headers {
                    if reqwest::header::HeaderName::from_bytes(k.as_bytes()).is_err()
                        || reqwest::header::HeaderValue::from_str(v).is_err()
                        || ["authorization", "x-api-key", "x-goog-api-key", "host"]
                            .contains(&k.to_lowercase().as_str())
                    {
                        return Err("凭据须使用密钥字段，附加请求头无效".into());
                    }
                }
            }
        }
        if !ids.contains(&self.default_model) {
            return Err("默认模型不存在".into());
        }
        Ok(())
    }
    pub fn model(&self, id: &str) -> Result<&Model, String> {
        self.models
            .iter()
            .find(|m| m.id == id)
            .ok_or_else(|| format!("模型 {id} 已删除，请重新选择模型"))
    }
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn initialize(
    user: &nast_storage::UserData,
    settings: &Value,
    secrets: &mut Value,
) -> Result<Catalog, String> {
    let path = user.root.join("models.json");
    if path.exists() {
        let c: Catalog = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        c.validate()?;
        return Ok(c);
    }
    let oai: OaiSettings =
        serde_json::from_value(settings["oai_settings"].clone()).unwrap_or_default();
    let (protocol, endpoint, key) = match connection::provider_kind(&oai, secrets) {
        ProviderKind::OpenAiCompat { base_url, api_key } => ("openai", base_url, api_key),
        ProviderKind::Anthropic { api_key } => {
            ("anthropic", "https://api.anthropic.com/v1".into(), api_key)
        }
        ProviderKind::Gemini { api_key } => (
            "gemini",
            "https://generativelanguage.googleapis.com/v1beta".into(),
            api_key,
        ),
    };
    // Never overwrite the original migration backup, including after an interrupted migration.
    for (name, value) in [("settings", settings), ("secrets", &*secrets)] {
        let backup = user
            .root
            .join("backups")
            .join(format!("pre-models-{name}.json"));
        if !backup.exists() {
            let original_path = user.root.join(format!("{name}.json"));
            let bytes = if original_path.exists() {
                std::fs::read(original_path).map_err(|e| e.to_string())?
            } else {
                serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?
            };
            nast_storage::atomic_write(&backup, &bytes).map_err(|e| e.to_string())?;
        }
    }
    let mut config = RouteConfig {
        endpoint,
        context_limit: Some(oai.openai_max_context),
        output_limit: Some(oai.openai_max_tokens),
        ..Default::default()
    };
    config.parameters = connection::extra_body(&oai)
        .as_object()
        .map(|o| {
            o.iter()
                .filter(|(k, _)| !protected(k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
        .unwrap_or_default();
    config.headers = connection::extra_headers(&oai)
        .into_iter()
        .filter(|(k, _)| {
            !["authorization", "x-api-key", "x-goog-api-key", "host"]
                .contains(&k.to_lowercase().as_str())
        })
        .collect();
    if !key.is_empty() {
        let reference = format!("api_key_route_{}", uuid::Uuid::new_v4());
        secrets[&reference] =
            json!([{"id":uuid::Uuid::new_v4().to_string(),"value":key,"active":true}]);
        config.credential_ref = Some(reference);
        user.save_secrets(secrets).map_err(|e| e.to_string())?;
    }
    let catalog = Catalog {
        version: 1,
        default_model: "default".into(),
        models: vec![Model {
            id: "default".into(),
            display_name: "默认模型".into(),
            routes: vec![Route {
                id: "legacy".into(),
                provider: oai.chat_completion_source.clone(),
                protocol: protocol.into(),
                upstream_model: connection::model_for(&oai),
                priority: 0,
                enabled: true,
                config,
            }],
        }],
    };
    catalog.validate()?;
    nast_storage::atomic_write(&path, &serde_json::to_vec_pretty(&catalog).unwrap())
        .map_err(|e| e.to_string())?;
    Ok(catalog)
}

pub async fn get(state: SharedState) -> RpcResult {
    let secrets = state.secrets.read().await;
    let catalog = state.catalog.lock().unwrap();
    let mut value = json!(*catalog);
    for m in value["models"].as_array_mut().unwrap() {
        for r in m["routes"].as_array_mut().unwrap() {
            r["credential_configured"] = json!(
                r["config"]["credential_ref"]
                    .as_str()
                    .and_then(|k| connection::active_secret(&secrets, k))
                    .is_some()
            );
        }
    }
    Ok(value)
}
pub async fn save(state: SharedState, params: Value) -> RpcResult {
    let mut next: Catalog = serde_json::from_value(params["catalog"].clone()).map_err(bad)?;
    next.validate().map_err(bad)?;
    let mut secrets = state.secrets.write().await;
    let mut current = state.catalog.lock().unwrap();
    if current.version != next.version {
        return Err(RpcError::Conflict(
            "模型目录已更新，请重新加载后保存；草稿尚未保存".into(),
        ));
    }
    let mut updated = secrets.clone();
    for model in &mut next.models {
        for route in &mut model.routes {
            let old = current
                .models
                .iter()
                .flat_map(|m| &m.routes)
                .find(|r| r.id == route.id);
            // Clients cannot bind another route's credential or the legacy global key.
            route.config.credential_ref = old.and_then(|r| r.config.credential_ref.clone());
            if let Some(key) = params["credentials"].get(&route.id) {
                let key = key.as_str().ok_or_else(|| bad("密钥必须是字符串"))?.trim();
                route.config.credential_ref = if key.is_empty() {
                    None
                } else {
                    let reference = format!("api_key_route_{}", uuid::Uuid::new_v4());
                    updated[&reference] =
                        json!([{"id":uuid::Uuid::new_v4().to_string(),"value":key,"active":true}]);
                    Some(reference)
                };
            }
        }
    }
    next.version = current
        .version
        .checked_add(1)
        .ok_or_else(|| bad("目录版本溢出"))?;
    state.user.save_secrets(&updated)?;
    nast_storage::atomic_write(
        &state.user.root.join("models.json"),
        &serde_json::to_vec_pretty(&next).map_err(bad)?,
    )?;
    *secrets = updated;
    *current = next;
    state
        .hub
        .emit("model_catalog_changed", json!({"version":current.version}));
    Ok(json!({"version":current.version}))
}

pub fn bind(chat: &mut ChatFile, default_model: &str, imported: bool) {
    if chat.0.is_empty() {
        chat.0.push(json!({"chat_metadata":{}}));
    }
    if !chat.0[0]["chat_metadata"].is_object() {
        chat.0[0]["chat_metadata"] = json!({});
    }
    let metadata = &mut chat.0[0]["chat_metadata"];
    if metadata["nast_model"]["selected_model"].as_str().is_none() {
        metadata["nast_model"] = json!({"selected_model":default_model});
    }
    if imported {
        let selection = metadata["nast_model"].as_object_mut().unwrap();
        selection.remove("active_route");
        selection.remove("active_route_info");
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Conversation {
    Private { avatar: String, chat_file: String },
    Group { group_id: String, chat_id: String },
}
impl Conversation {
    pub fn read(&self, state: &SharedState) -> Result<ChatFile, RpcError> {
        match self {
            Self::Private { avatar, chat_file } => {
                crate::library::leaf(avatar)?;
                crate::library::leaf(chat_file)?;
                Ok(state.user.read_chat(avatar, chat_file)?)
            }
            Self::Group { group_id, chat_id } => {
                crate::library::leaf(chat_id)?;
                if !state.user.list_groups()?.iter().any(|g| {
                    &g.id == group_id && (g.chats.contains(chat_id) || &g.chat_id == chat_id)
                }) {
                    return Err(bad("聊天不属于此群组"));
                }
                Ok(state.user.read_group_chat(chat_id)?)
            }
        }
    }
    pub fn write(&self, state: &SharedState, chat: &ChatFile) -> Result<(), RpcError> {
        match self {
            Self::Private { avatar, chat_file } => {
                state.user.save_chat(avatar, chat_file, chat, false)?
            }
            Self::Group { chat_id, .. } => state.user.save_group_chat(chat_id, chat, false)?,
        };
        Ok(())
    }
}
pub async fn conversation(state: SharedState, params: Value, set: bool) -> RpcResult {
    let reference: Conversation =
        serde_json::from_value(params["conversation"].clone()).map_err(bad)?;
    let generation = state.generation.read().await;
    if set && generation.abort.is_some() {
        return Err(bad("生成期间不能切换模型"));
    }
    let mut chat = reference.read(&state)?;
    let catalog = state.catalog.lock().unwrap();
    let needs_binding = chat
        .0
        .first()
        .and_then(|h| h.pointer("/chat_metadata/nast_model/selected_model"))
        .is_none();
    bind(&mut chat, &catalog.default_model, false);
    if set {
        let id = params["model_id"]
            .as_str()
            .ok_or_else(|| bad("缺少 model_id"))?;
        catalog.model(id).map_err(bad)?;
        if chat.0[0]["chat_metadata"]["nast_model"]["selected_model"] != id {
            chat.0[0]["chat_metadata"]["nast_model"] = json!({"selected_model":id});
        }
    }
    let selection = chat.0[0]["chat_metadata"]["nast_model"].clone();
    if set || needs_binding {
        reference.write(&state, &chat)?;
    }
    if set {
        state.hub.emit(
            "conversation_model_changed",
            json!({"conversation":reference,"state":selection}),
        );
    }
    let id = selection["selected_model"].as_str().unwrap_or_default();
    let model = catalog.model(id);
    let ordered = model
        .as_ref()
        .map(|m| ordered_routes(m, selection["active_route"].as_str()))
        .unwrap_or_default();
    Ok(
        json!({"conversation":reference,"state":selection,"model":model.ok(),"candidate_route":ordered.first().map(|r| &r.id),"error": if ordered.is_empty() { Some("模型已删除或没有启用的路由，请在模型设置中修复或重新选择") } else { None }}),
    )
}
pub fn ordered_routes(model: &Model, active: Option<&str>) -> Vec<Route> {
    let mut routes: Vec<_> = model.routes.iter().filter(|r| r.enabled).cloned().collect();
    routes.sort_by(|a, b| b.priority.cmp(&a.priority));
    if let Some(index) = routes.iter().position(|r| Some(r.id.as_str()) == active) {
        let r = routes.remove(index);
        routes.insert(0, r);
    }
    routes
}
pub fn provider(route: &Route, secrets: &Value) -> Provider {
    let key = route
        .config
        .credential_ref
        .as_deref()
        .and_then(|k| connection::active_secret(secrets, k))
        .unwrap_or_default();
    let kind = match route.protocol.as_str() {
        "anthropic" => ProviderKind::Anthropic { api_key: key },
        "gemini" => ProviderKind::Gemini { api_key: key },
        _ => ProviderKind::OpenAiCompat {
            api_key: key,
            base_url: route.config.endpoint.clone(),
        },
    };
    Provider::configured(
        kind,
        route.config.endpoint.clone(),
        route.config.connect_timeout_secs,
        route.config.remove_parameters.clone(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn route(id: &str, priority: i64) -> Route {
        Route {
            id: id.into(),
            provider: id.into(),
            protocol: "openai".into(),
            upstream_model: "test".into(),
            priority,
            enabled: true,
            config: RouteConfig {
                endpoint: "http://localhost/v1".into(),
                ..Default::default()
            },
        }
    }
    #[test]
    fn priority_order_is_stable_and_only_enabled_sticky_routes_win() {
        let mut model = Model {
            id: "model".into(),
            display_name: "Model".into(),
            routes: vec![route("a", 1), route("b", 1), route("c", 5)],
        };
        let ids = |routes: Vec<Route>| routes.into_iter().map(|r| r.id).collect::<Vec<_>>();
        assert_eq!(ids(ordered_routes(&model, None)), ["c", "a", "b"]);
        assert_eq!(ids(ordered_routes(&model, Some("b"))), ["b", "c", "a"]);
        model.routes[1].enabled = false;
        assert_eq!(ids(ordered_routes(&model, Some("b"))), ["c", "a"]);
    }
    #[test]
    fn new_and_imported_chat_bindings_do_not_follow_later_defaults() {
        let mut chat = ChatFile(vec![json!({"chat_metadata":{}})]);
        bind(&mut chat, "original", false);
        bind(&mut chat, "new-default", false);
        chat.0[0]["chat_metadata"]["nast_model"]["active_route"] = json!("a");
        bind(&mut chat, "new-default", true);
        assert_eq!(
            chat.0[0]["chat_metadata"]["nast_model"],
            json!({"selected_model":"original"})
        );
    }
    #[test]
    fn catalog_rejects_duplicate_ids_and_reserved_overrides() {
        let mut c = Catalog {
            version: 1,
            default_model: "m".into(),
            models: vec![Model {
                id: "m".into(),
                display_name: "M".into(),
                routes: vec![route("a", 0)],
            }],
        };
        assert!(c.validate().is_ok());
        c.models[0].routes.push(route("a", 2));
        assert!(c.validate().is_err());
        c.models[0].routes.pop();
        c.models[0].routes[0]
            .config
            .parameters
            .insert("stream".into(), json!(false));
        assert!(c.validate().is_err());
    }
}
