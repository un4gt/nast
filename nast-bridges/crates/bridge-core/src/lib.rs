//! bridge-core：nast 桥接公共层（平台无关）。
//!
//! 平台适配器（QQ/Discord/飞书…）只做三件事：
//! 1. 拿平台凭据并维持平台网关长连接；
//! 2. 收到消息 → 清洗为纯文本 → 组装 [`InboundMessage`] 调 [`BridgeContext::handle_inbound`]；
//! 3. 返回的 `Option<String>`（完整回复文本）按平台 API 发出去，None 则不回复。
//!
//! 本层负责：nast WS RPC 客户端（断线重试）、按来源的会话（角色 + 聊天文件）、
//! 命令层（/help /chars /char /newchat /worlds /world）、自定义命令展开
//! （settings.power_user.custom_commands）、插件命令透传、生成与完成状态。

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

// ---------- 配置 ----------

#[derive(Debug, Clone)]
pub struct Config {
    /// nast WS RPC 地址（如 ws://nast:8000/ws）
    pub nast_server: String,
    /// 默认角色卡文件名（空 = 第一个角色）
    pub avatar: String,
    /// 平台单条消息的分段字符数（不截断总回复）
    pub max_chars: usize,
    /// 生成超时（秒）：超时后主动 generate.stop 解锁并回复错误
    pub gen_timeout_secs: u64,
    /// 平台附加帮助行（适配器自定义命令说明）
    pub platform_help: String,
}

impl Config {
    /// 环境变量读取：BRIDGE_NAST_SERVER / BRIDGE_AVATAR / BRIDGE_MAX_CHARS。
    pub fn from_env(platform_help: &str) -> Self {
        Self {
            nast_server: std::env::var("BRIDGE_NAST_SERVER")
                .or_else(|_| std::env::var("NAST_QQBOT_SERVER")) // 旧名兼容
                .unwrap_or_else(|_| "ws://127.0.0.1:8000/ws".into()),
            avatar: std::env::var("BRIDGE_AVATAR")
                .or_else(|_| std::env::var("NAST_QQBOT_AVATAR"))
                .unwrap_or_default(),
            max_chars: std::env::var("BRIDGE_MAX_CHARS")
                .or_else(|_| std::env::var("NAST_QQBOT_MAX_TOK"))
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1500),
            gen_timeout_secs: std::env::var("BRIDGE_GEN_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|v| *v >= 10)
                .unwrap_or(240),
            platform_help: platform_help.to_string(),
        }
    }
}

// ---------- 消息形状 ----------

/// 平台适配器送进来的标准化消息。
#[derive(Debug, Clone)]
pub struct InboundMessage {
    /// 会话来源键（适配器自定，如 "qq-g-<groupid>" / "dm-<channelid>"）
    pub source_key: String,
    /// 清洗后的纯文本（已去 @提及/平台转义）
    pub text: String,
}

// ---------- 会话 ----------

/// 每个来源的会话：绑定的角色 + 当前聊天文件。
#[derive(Debug, Clone)]
pub struct Session {
    pub avatar: String,
    pub chat_file: String,
}

// ---------- nast WS RPC 客户端 ----------

pub struct NastClient {
    url: String,
    token: Option<String>,
    inner: tokio::sync::Mutex<Option<NastConn>>,
    next_id: std::sync::atomic::AtomicU64,
}

struct NastConn {
    ws: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
}

impl NastClient {
    pub fn new(url: String) -> Self {
        Self::with_token(url, std::env::var("BRIDGE_NAST_TOKEN").ok().filter(|s|!s.is_empty()))
    }
    pub fn with_token(url: String, token: Option<String>) -> Self {
        Self {
            url,
            token,
            inner: tokio::sync::Mutex::new(None),
            next_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// 调用 RPC：建立连接可重试；发送后的传输错误仅允许只读方法重试。
    /// 业务错误（响应里的 error）不重试，直接返回。
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let mut last_err = String::new();
        // Only explicitly read-only RPCs may be replayed after a possibly successful send.
        let replayable = matches!(method, "characters.all" | "characters.chats" | "chats.get" | "settings.get" | "worlds.list" | "plugins.list" | "generate.status" | "model_catalog.get" | "conversation_model.get");
        for attempt in 0..3 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(500 * attempt as u64)).await;
            }
            let mut guard = self.inner.lock().await;
            if guard.is_none() {
                use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::header};
                let mut request = self.url.as_str().into_client_request().map_err(|_|"无效 BRIDGE_NAST_SERVER".to_string())?;
                if request.uri().authority().is_some_and(|a| a.as_str().contains('@')) {
                    return Err("不要把凭据放入 URL，请使用 BRIDGE_NAST_TOKEN".into());
                }
                if let Some(token) = &self.token {
                    let mut value = header::HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_|"BRIDGE_NAST_TOKEN 包含无效字符".to_string())?;
                    value.set_sensitive(true);
                    request.headers_mut().insert(header::AUTHORIZATION, value);
                }
                match tokio_tungstenite::connect_async(request).await {
                    Ok((ws, _)) => {
                        tracing::info!("nast RPC connected");
                        *guard = Some(NastConn { ws });
                    }
                    Err(e) => {
                        if matches!(&e, tokio_tungstenite::tungstenite::Error::Http(response) if matches!(response.status().as_u16(), 401 | 403)) {
                            return Err("NAST 认证失败：请检查 BRIDGE_NAST_TOKEN 与服务端 NAST_BRIDGE_TOKEN 是否一致".into());
                        }
                        last_err = format!("connect nast: {e}");
                        drop(guard);
                        continue;
                    }
                }
            }
            let conn = guard.as_mut().unwrap();
            let id = self
                .next_id
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                .to_string();
            let req = json!({"id": id, "method": method, "params": params});
            if let Err(e) = conn.ws.send(Message::Text(req.to_string())).await {
                last_err = format!("send: {e}");
                *guard = None;
                if !replayable { return Err(format!("请求可能已提交，不自动重发：{last_err}")); }
                continue;
            }
            let rpc_budget = if method.starts_with("generate.") && !matches!(method, "generate.stop" | "generate.status") {
                params["time_budget_secs"].as_u64().unwrap_or(600).clamp(1,600) + 15
            } else { 30 };
            let deadline = tokio::time::Instant::now() + Duration::from_secs(rpc_budget);
            loop {
                let msg = tokio::select! {
                    m = conn.ws.next() => m,
                    _ = tokio::time::sleep_until(deadline) => {
                        *guard = None;
                        return Err("nast RPC timeout".into());
                    }
                };
                let Some(msg) = msg else {
                    last_err = "stream ended".into();
                    *guard = None;
                    break;
                };
                let text = match msg {
                    Ok(Message::Text(t)) => t,
                    Ok(Message::Ping(p)) => {
                        let _ = conn.ws.send(Message::Pong(p)).await;
                        continue;
                    }
                    Ok(Message::Close(_)) => {
                        last_err = "connection closed".into();
                        *guard = None;
                        break;
                    }
                    Ok(_) => continue,
                    Err(e) => {
                        last_err = format!("nast RPC read: {e}");
                        *guard = None;
                        break;
                    }
                };
                let v: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if v.get("event").is_some() {
                    continue;
                }
                if v.get("id").and_then(|i| i.as_str()) != Some(id.as_str()) {
                    continue;
                }
                if let Some(err) = v.get("error") {
                    let detail = &err["diagnostic"]["detail"];
                    tracing::warn!(event="bridge_rpc_failed", method, task_id=params["task_id"].as_str().unwrap_or(""),
                        code=err["code"].as_str().unwrap_or("unknown"),
                        error_kind=detail["type"].as_str().unwrap_or("unknown"),
                        http_status=?detail["data"]["status"].as_u64(),
                        input_tokens=?detail["data"]["input_tokens"].as_i64(),
                        output_tokens=?detail["data"]["output_tokens"].as_i64(), limit=?detail["data"]["limit"].as_i64());
                    return Err(err
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("unknown error")
                        .to_string());
                }
                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
            }
            if !replayable { return Err(format!("请求已提交，连接中断；不会自动重发：{last_err}")); }
        }
        Err(format!("nast RPC unreachable: {last_err}"))
    }
}

// ---------- 桥接上下文 ----------

pub struct BridgeContext {
    pub cfg: Config,
    pub nast: Arc<NastClient>,
    /// 控制通道（独立连接）：generate.run 占住主连接时仍能发 generate.stop
    pub ctrl: Arc<NastClient>,
    sessions: Mutex<HashMap<String, Session>>,
    session_path: std::path::PathBuf,
    session_error: Option<String>,
    session_operation: tokio::sync::Mutex<()>,
}

impl BridgeContext {
    pub fn new(cfg: Config) -> Arc<Self> {
        let session_path = std::env::var_os("BRIDGE_SESSIONS_PATH").map(std::path::PathBuf::from).unwrap_or_else(|| "data/bridge-sessions.json".into());
        Self::with_session_path(cfg, session_path)
    }
    pub fn with_session_path(cfg: Config, session_path: std::path::PathBuf) -> Arc<Self> {
        let nast = Arc::new(NastClient::new(cfg.nast_server.clone()));
        let ctrl = Arc::new(NastClient::new(cfg.nast_server.clone()));
        let (sessions, session_error) = match read_sessions(&session_path, &cfg.nast_server) {
            Ok(sessions) => (sessions,None), Err(e) => (HashMap::new(),Some(e)),
        };
        Arc::new(Self {
            session_path, session_error, session_operation:tokio::sync::Mutex::new(()),
            cfg,
            nast,
            ctrl,
            sessions: Mutex::new(sessions),
        })
    }

    /// 主入口：命令层 → 会话生成。返回应回复的完整文本；None = 不回复。
    pub async fn handle_inbound(&self, msg: InboundMessage) -> Option<String> {
        let text = msg.text.trim().to_string();
        tracing::info!(event="bridge_inbound", source=%msg.source_key, input_chars=text.chars().count(), command=text.starts_with('/'));
        if text.is_empty() {
            return Some("（空消息）".into());
        }

        // ---- 命令层（不触发生成） ----
        if text.starts_with('/') {
            let body = &text[1..];
            let (cmd, args) = match body.split_once(' ') {
                Some((c, a)) => (c.to_lowercase(), a.trim().to_string()),
                None => (body.to_lowercase(), String::new()),
            };
            match cmd.as_str() {
                "model" => {
                    let result = match self.ensure_session(&msg.source_key).await {
                        Ok(session) => self.nast.call("model.command",json!({"conversation":{"kind":"private","avatar":session.avatar,"chat_file":session.chat_file},"argument":args})).await.map(|r|r["text"].as_str().unwrap_or_default().to_string()),
                        Err(e) => Err(e),
                    };
                    return Some(result.unwrap_or_else(|e|format!("模型操作失败：{e}")));
                }
                "help" => return Some(self.help_text()),
                "chars" | "characters" => return Some(self.list_characters().await),
                "character" | "char" => {
                    return Some(if args.is_empty() {
                        "用法：/char <角色名片段>".into()
                    } else {
                        match self.reset_session(&msg.source_key, Some(&args)).await {
                            Ok(sess) => format!("已切换角色：{}", sess.avatar),
                            Err(e) => format!("切换失败：{e}"),
                        }
                    });
                }
                "newchat" => {
                    return Some(
                        match self.reset_session(&msg.source_key, None).await {
                            Ok(sess) => format!("已开新聊天（角色 {}）", sess.avatar),
                            Err(e) => format!("开新聊天失败：{e}"),
                        },
                    );
                }
                "worlds" => return Some(self.list_worlds().await),
                "world" => {
                    let q = args.trim().to_string();
                    if q.is_empty() {
                        return Some("用法：/world <名称|none>".into());
                    }
                    return Some(self.bind_world(&msg.source_key, &q).await);
                }
                _ => {
                    // 自定义命令：展开为文本走生成
                    if let Some(expansion) = self.custom_command_text(&cmd).await {
                        if !expansion.is_empty() {
                            return self.generate_reply(&msg.source_key, &expansion).await;
                        }
                    }
                    // 插件命令：透传服务端 Lua 执行（无文本产出则不回复）
                    if self.is_plugin_command(&cmd).await {
                        return self.generate_reply_raw(&msg.source_key, &text).await;
                    }
                    tracing::info!("[{}] 拦截未知命令 /{cmd}", msg.source_key);
                    return Some(format!("未知命令 /{cmd}，发送 /help 查看可用命令。"));
                }
            }
        }

        // ---- 会话生成 ----
        self.generate_reply(&msg.source_key, &text).await
    }

    /// Preserve the full result. Platform adapters split messages without discarding text.
    async fn generate_reply(&self, key: &str, text: &str) -> Option<String> {
        self.generate_reply_raw(key, text).await
    }

    /// 生成不截断版本：None = 无文本产出（如插件命令被服务端吞掉）。
    async fn generate_reply_raw(&self, key: &str, text: &str) -> Option<String> {
        let (avatar, chat_file) = match self.ensure_session(key).await {
            Ok(s) => (s.avatar, s.chat_file),
            Err(e) => {
                tracing::error!("[{key}] 会话初始化失败：{e}");
                return Some(format!("会话初始化失败：{e}"));
            }
        };
        // 生成 + 超时守卫：超时则经控制通道 generate.stop 中止（释放 nast 的单生成锁）
        let timeout = Duration::from_secs(self.cfg.gen_timeout_secs);
        let task_id = uuid::Uuid::new_v4().to_string();
        let started = std::time::Instant::now();
        tracing::info!(event="bridge_generation_started", source=key, task_id, timeout_secs=self.cfg.gen_timeout_secs);
        let gen_fut = self.nast.call(
            "generate.run",
            json!({
                "avatar": avatar,
                "chat_file": chat_file,
                "type": "normal",
                "user_message": text,
                "task_id": task_id,
                "time_budget_secs": self.cfg.gen_timeout_secs.saturating_sub(5).max(1),
            }),
        );
        match tokio::time::timeout(timeout, gen_fut).await {
            Ok(Ok(r)) => {
                let mut out = r.get("text").and_then(|t| t.as_str()).unwrap_or_default().to_string();
                let incomplete = r["routing"]["status"] == "incomplete";
                tracing::info!(event="bridge_generation_result", source=key, task_id,
                    status=r["routing"]["status"].as_str().unwrap_or("unknown"),
                    finish_reason=r["routing"]["finish_reason"].as_str().unwrap_or("unknown"),
                    error_kind=r["routing"]["error"]["detail"]["type"].as_str().unwrap_or("none"),
                    text_chars=out.chars().count(), reasoning_chars=r["reasoning"].as_str().unwrap_or("").chars().count(),
                    elapsed_ms=started.elapsed().as_millis() as u64);
                if incomplete {
                    if out.is_empty() && r["reasoning"].as_str().is_some_and(|v|!v.is_empty()) {
                        out.push_str("仅收到思考内容，已保存在网页聊天记录中。");
                    }
                    if r["routing"]["error"]["detail"]["type"] == "output_limit" {
                        out.push_str("\n（未完成：达到最大输出 Token，请在模型参数中调高最大输出，或在网页续写。）");
                    } else { out.push_str("\n（未完成：生成中断，已保留部分结果）"); }
                }
                if out.is_empty() && text.starts_with('/') {
                    None
                } else {
                    Some(out)
                }
            }
            Ok(Err(e)) => {
                tracing::error!(event="bridge_generation_failed", source=key, task_id, elapsed_ms=started.elapsed().as_millis() as u64);
                Some(format!("生成失败：{e}"))
            }
            Err(_) => {
                tracing::error!(event="bridge_generation_timeout", source=key, task_id, timeout_secs=self.cfg.gen_timeout_secs);
                let ctrl = self.ctrl.clone();
                tokio::spawn(async move {
                    match ctrl.call("generate.stop", json!({"task_id":task_id})).await {
                        Ok(result) => tracing::info!(event="bridge_cancel_result", task_id, cancelled=result["ok"].as_bool().unwrap_or(false)),
                        Err(_) => tracing::warn!(event="bridge_cancel_failed", task_id),
                    }
                });
                Some(format!("生成超时（超过 {} 秒），已请求中止本次生成，请稍后重试。", self.cfg.gen_timeout_secs))
            }
        }
    }

    // ---------- 会话管理 ----------

    pub async fn ensure_session(&self, key: &str) -> Result<Session, String> {
        let _operation = self.session_operation.lock().await;
        if let Some(error) = &self.session_error { return Err(error.clone()); }
        if let Some(s) = self.sessions.lock().unwrap().get(key).cloned() {
            return Ok(s);
        }
        self.reset_session_inner(key, None, false).await
    }

    /// 建立或重置会话：选定角色（None = 保持当前/默认）并开新聊天文件。
    pub async fn reset_session(
        &self,
        key: &str,
        avatar_hint: Option<&str>,
    ) -> Result<Session, String> {
        let _operation = self.session_operation.lock().await;
        if let Some(error) = &self.session_error { return Err(error.clone()); }
        self.reset_session_inner(key, avatar_hint, avatar_hint.is_none()).await
    }
    async fn reset_session_inner(&self, key: &str, avatar_hint: Option<&str>, force_new: bool) -> Result<Session,String> {
        let existing_avatar = self.sessions.lock().unwrap().get(key).map(|s|s.avatar.clone());
        let avatar = match avatar_hint {
            Some(frag) => self.find_avatar_by_name(frag).await?,
            None => match existing_avatar {
                Some(a) => a,
                None => self.ensure_default_avatar().await?,
            },
        };
        let cached = self.sessions.lock().unwrap().get(&format!("{key}::{avatar}")).cloned();
        let chat_file = if force_new { self.fresh_chat(&avatar,key).await? }
        else if let Some(session) = cached { session.chat_file }
        else {
            // Upgrade recovery: reuse the newest source thread, never create one just because the process restarted.
            let list = self.nast.call("characters.chats",json!({"avatar":avatar})).await?;
            let latest = list.as_array().into_iter().flatten().filter_map(Value::as_str).filter_map(|name| {
                let stem=name.strip_suffix(".jsonl").unwrap_or(name);
                if stem == key { Some((1,name.to_string())) }
                else { stem.strip_prefix(&format!("{key}-")).and_then(|s|s.parse::<u64>().ok()).map(|n|(n,name.to_string())) }
            }).max_by_key(|(n,_)|*n).map(|(_,name)|name);
            match latest { Some(name)=>name, None=>self.fresh_chat(&avatar,key).await? }
        };
        let sess = Session {
            avatar: avatar.clone(),
            chat_file: chat_file.clone(),
        };
        {
            let mut sessions = self.sessions.lock().unwrap();
            let mut updated = sessions.clone();
            updated.insert(key.to_string(),sess.clone());
            updated.insert(format!("{key}::{avatar}"),sess.clone());
            save_sessions(&self.session_path, &self.cfg.nast_server, &updated)?;
            *sessions = updated;
        }
        tracing::info!("[{key}] 会话重置：{avatar} / {chat_file}");
        Ok(sess)
    }

    /// 默认角色：配置指定的或第一个可用角色。
    pub async fn ensure_default_avatar(&self) -> Result<String, String> {
        let chars = self
            .nast
            .call("characters.all", json!({}))
            .await
            .map_err(|e| format!("服务端暂时不可用（{e}）"))?;
        let arr = chars.as_array().ok_or("角色列表异常")?;
        if !self.cfg.avatar.is_empty() {
            let hit = arr
                .iter()
                .any(|c| c.get("avatar").and_then(|a| a.as_str()) == Some(self.cfg.avatar.as_str()));
            if hit {
                return Ok(self.cfg.avatar.clone());
            }
            return Err(format!("BRIDGE_AVATAR 指定的角色不存在：{}", self.cfg.avatar));
        }
        arr.iter()
            .filter(|c| c.get("error").is_none())
            .find_map(|c| c.get("avatar"))
            .and_then(|a| a.as_str())
            .map(String::from)
            .ok_or_else(|| "服务端没有任何角色卡".into())
    }

    /// 按名字片段找角色。
    pub async fn find_avatar_by_name(&self, frag: &str) -> Result<String, String> {
        let chars = self
            .nast
            .call("characters.all", json!({}))
            .await
            .map_err(|e| format!("服务端暂时不可用（{e}）"))?;
        let arr = chars.as_array().ok_or("角色列表异常")?;
        let frag_lc = frag.to_lowercase();
        arr.iter()
            .filter(|c| c.get("error").is_none())
            .find(|c| {
                c.get("name")
                    .and_then(|n| n.as_str())
                    .map(|n| n.to_lowercase().contains(&frag_lc))
                    .unwrap_or(false)
            })
            .and_then(|c| c.get("avatar"))
            .and_then(|a| a.as_str())
            .map(String::from)
            .ok_or_else(|| format!("没有名字包含「{frag}」的角色"))
    }

    /// 新建（而非复用）<key> 聊天：已存在则顺延 -2/-3…。
    async fn fresh_chat(&self, avatar: &str, key: &str) -> Result<String, String> {
        let list = self
            .nast
            .call("characters.chats", json!({"avatar": avatar}))
            .await
            .map_err(|e| format!("服务端暂时不可用（{e}）"))?;
        let mut max_n = 0;
        if let Some(arr) = list.as_array() {
            for f in arr.iter().filter_map(|f| f.as_str()) {
                let stem = f.strip_suffix(".jsonl").unwrap_or(f);
                if let Some(suffix) = stem.strip_prefix(&format!("{key}-")) {
                    if let Ok(n) = suffix.parse::<u32>() {
                        max_n = max_n.max(n);
                    }
                } else if stem == key {
                    max_n = max_n.max(1);
                }
            }
        }
        let name = if max_n == 0 {
            key.to_string()
        } else {
            format!("{key}-{}", max_n + 1)
        };
        let created = self
            .nast
            .call("chats.new", json!({"avatar": avatar, "greeting_index": -1}))
            .await
            .map_err(|e| format!("chats.new: {e}"))?;
        let file_name = created
            .get("file_name")
            .and_then(|f| f.as_str())
            .ok_or("chats.new 缺少 file_name")?
            .to_string();
        self.nast
            .call(
                "chats.rename",
                json!({"avatar": avatar, "original_file": file_name, "renamed_file": name}),
            )
            .await
            .map_err(|e| format!("chats.rename: {e}"))?;
        Ok(format!("{name}.jsonl"))
    }

    // ---------- 列表/世界书 ----------

    async fn list_characters(&self) -> String {
        match self.nast.call("characters.all", json!({})).await {
            Ok(list) => {
                let names: Vec<String> = list
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter(|c| c.get("error").is_none())
                            .map(|c| {
                                format!(
                                    "{} {}",
                                    if c.get("fav").and_then(|f| f.as_bool()).unwrap_or(false) {
                                        "*"
                                    } else {
                                        "-"
                                    },
                                    c.get("name").and_then(|n| n.as_str()).unwrap_or("?")
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if names.is_empty() { "（无角色卡）".into() } else { names.join("\n") }
            }
            Err(e) => format!("角色列表获取失败：{e}"),
        }
    }

    async fn list_worlds(&self) -> String {
        match self.nast.call("worlds.list", json!({})).await {
            Ok(list) => {
                let names: Vec<String> = list
                    .as_array()
                    .map(|a| a.iter().filter_map(|w| w.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                if names.is_empty() { "（无世界书）".into() } else { names.join("\n") }
            }
            Err(e) => format!("世界书列表获取失败：{e}"),
        }
    }

    async fn bind_world(&self, key: &str, q: &str) -> String {
        let unbind = q.eq_ignore_ascii_case("none");
        let valid = if unbind {
            true
        } else {
            self.nast
                .call("worlds.list", json!({}))
                .await
                .ok()
                .and_then(|l| l.as_array().cloned())
                .map(|a| a.iter().any(|w| w.as_str() == Some(q)))
                .unwrap_or(false)
        };
        if !valid {
            return format!("没有名为「{q}」的世界书（/worlds 查看）");
        }
        let Some(sess) = self.sessions.lock().unwrap().get(key).cloned() else {
            return "会话尚未初始化，先发一条消息".into();
        };
        match self
            .nast
            .call(
                "chats.set_world",
                json!({
                    "avatar": sess.avatar,
                    "file_name": sess.chat_file,
                    "world": if unbind { Value::Null } else { json!(q) }
                }),
            )
            .await
        {
            Ok(_) => {
                if unbind { "已解绑世界书".into() } else { format!("已绑定世界书：{q}") }
            }
            Err(e) => format!("绑定失败：{e}"),
        }
    }

    // ---------- 自定义/插件命令 ----------

    /// 自定义命令（power_user.custom_commands，60s 缓存）：命中返回展开文本。
    pub async fn custom_command_text(&self, cmd: &str) -> Option<String> {
        static CACHE: tokio::sync::Mutex<Option<(std::time::Instant, Vec<(String, String)>)>> =
            tokio::sync::Mutex::const_new(None);
        let mut guard = CACHE.lock().await;
        let expired = guard
            .as_ref()
            .map(|(at, _)| at.elapsed() > Duration::from_secs(60))
            .unwrap_or(true);
        if expired {
            if let Ok(settings) = self.nast.call("settings.get", json!({})).await {
                let list = settings
                    .pointer("/power_user/custom_commands")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|c| {
                        Some((
                            c.get("name")?.as_str()?.to_lowercase(),
                            c.get("text")?.as_str()?.to_string(),
                        ))
                    })
                    .collect::<Vec<_>>();
                *guard = Some((std::time::Instant::now(), list));
            }
        }
        guard
            .as_ref()
            .and_then(|(_, list)| list.iter().find(|(n, _)| n == cmd).map(|(_, t)| t.clone()))
    }

    /// 是否为插件注册的命令（60s 缓存）。
    pub async fn is_plugin_command(&self, cmd: &str) -> bool {
        static CACHE: tokio::sync::Mutex<Option<(std::time::Instant, Vec<String>)>> =
            tokio::sync::Mutex::const_new(None);
        let mut guard = CACHE.lock().await;
        let expired = guard
            .as_ref()
            .map(|(at, _)| at.elapsed() > Duration::from_secs(60))
            .unwrap_or(true);
        if expired {
            if let Ok(r) = self.nast.call("plugins.list", json!({})).await {
                let list = r
                    .get("plugins")
                    .and_then(|p| p.as_array())
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .flat_map(|p| {
                        p.get("commands").and_then(|c| c.as_array()).cloned().unwrap_or_default()
                    })
                    .filter_map(|c| c.as_str().map(String::from))
                    .collect::<Vec<_>>();
                *guard = Some((std::time::Instant::now(), list));
            }
        }
        guard
            .as_ref()
            .map(|(_, list)| list.iter().any(|c| c == cmd))
            .unwrap_or(false)
    }

    // ---------- 帮助 ----------

    pub fn help_text(&self) -> String {
        let mut lines = vec![
            "命令：".to_string(),
            "/help —— 本帮助".to_string(),
            "/model [id|info] —— 当前会话模型与线路信息".to_string(),
            "/chars —— 列出全部角色卡".to_string(),
            "/char <名字片段> —— 切换角色并恢复该来源会话（/character 同义）".to_string(),
            "/newchat —— 开一个新聊天（同一角色）".to_string(),
            "/worlds —— 列出全部世界书".to_string(),
            "/world <名称|none> —— 绑定/解绑本会话的世界书".to_string(),
        ];
        if !self.cfg.platform_help.is_empty() {
            lines.push(self.cfg.platform_help.clone());
        }
        lines.push("其余消息直接与当前角色对话；自定义命令见网页端 /help。".to_string());
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn bridge_token_is_sent_on_every_reconnect() {
        use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for attempt in 0..2 {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_hdr_async(stream, |request: &Request, response: Response| {
                    assert_eq!(request.headers()["authorization"], "Bearer isolated-test-token");
                    assert_eq!(request.uri().path(), "/ws");
                    assert!(request.uri().query().is_none());
                    Ok(response)
                }).await.unwrap();
                let request: Value = serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                if attempt == 1 {
                    socket.send(Message::Text(json!({"id":request["id"],"result":[]}).to_string())).await.unwrap();
                }
            }
        });
        let client = NastClient::with_token(format!("ws://{address}/ws"), Some("isolated-test-token".into()));
        assert_eq!(client.call("characters.all",json!({})).await.unwrap(),json!([]));
        server.await.unwrap();
    }

    #[test]
    fn session_mapping_survives_restart_and_rejects_wrong_server() {
        let directory = tempfile::tempdir().unwrap(); let path = directory.path().join("sessions.json");
        let mut sessions = HashMap::new(); sessions.insert("qq-g-1".into(), Session { avatar:"a.png".into(),chat_file:"thread-2.jsonl".into() });
        save_sessions(&path,"ws://test/ws",&sessions).unwrap();
        save_sessions(&path,"ws://test/ws",&sessions).unwrap();
        assert_eq!(read_sessions(&path,"ws://test/ws").unwrap()["qq-g-1"].chat_file,"thread-2.jsonl");
        assert!(read_sessions(&path,"ws://other/ws").is_err());
        std::fs::write(&path,b"broken").unwrap(); assert!(read_sessions(&path,"ws://test/ws").is_err());
    }
    #[tokio::test]
    async fn submitted_generation_is_never_replayed() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let server=tokio::spawn(async move {
            let (stream,_)=listener.accept().await.unwrap();
            let mut ws=tokio_tungstenite::accept_async(stream).await.unwrap();
            let request=ws.next().await.unwrap().unwrap();
            assert!(request.to_text().unwrap().contains("generate.run"));
            ws.close(None).await.unwrap();
            assert!(tokio::time::timeout(Duration::from_millis(1800),listener.accept()).await.is_err());
        });
        let client=NastClient::new(format!("ws://{address}/ws"));
        let error=client.call("generate.run",json!({"task_id":"one"})).await.unwrap_err();
        assert!(error.contains("不会自动重发")); server.await.unwrap();
    }

    fn test_context(url: String, directory: &std::path::Path, timeout: u64) -> BridgeContext {
        let mut sessions=HashMap::new(); sessions.insert("qq-g-1".into(),Session {avatar:"actor.png".into(),chat_file:"thread.jsonl".into()});
        BridgeContext { cfg:Config { nast_server:url.clone(),avatar:String::new(),max_chars:1500,gen_timeout_secs:timeout,platform_help:String::new() },
            nast:Arc::new(NastClient::new(url.clone())),ctrl:Arc::new(NastClient::new(url)),sessions:Mutex::new(sessions),
            session_path:directory.join("sessions.json"),session_error:None,session_operation:tokio::sync::Mutex::new(()) }
    }
    #[tokio::test]
    async fn model_commands_use_current_conversation_without_generating() {
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap(); let address=listener.local_addr().unwrap();
        let server=tokio::spawn(async move {
            let (stream,_)=listener.accept().await.unwrap(); let mut ws=tokio_tungstenite::accept_async(stream).await.unwrap();
            for argument in ["","second","info"] {
                let request:Value=serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                assert_eq!(request["method"],"model.command"); assert_eq!(request["params"]["argument"],argument);
                assert_eq!(request["params"]["conversation"],json!({"kind":"private","avatar":"actor.png","chat_file":"thread.jsonl"}));
                ws.send(Message::Text(json!({"id":request["id"],"result":{"text":"model reply"}}).to_string())).await.unwrap();
            }
        });
        let directory=tempfile::tempdir().unwrap();let context=test_context(format!("ws://{address}/ws"),directory.path(),240);
        for command in ["/model","/model second","/model info"] { assert_eq!(context.handle_inbound(InboundMessage {source_key:"qq-g-1".into(),text:command.into()}).await.as_deref(),Some("model reply")); }
        server.await.unwrap();
    }
    #[tokio::test]
    async fn timeout_cancel_uses_the_submitted_task_identity() {
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let address=listener.local_addr().unwrap();
        let server=tokio::spawn(async move {
            let (stream,_)=listener.accept().await.unwrap();let mut ws=tokio_tungstenite::accept_async(stream).await.unwrap();
            let generate:Value=serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(generate["method"],"generate.run");assert_eq!(generate["params"]["time_budget_secs"],1);
            let (stream,_)=tokio::time::timeout(Duration::from_secs(5),listener.accept()).await.unwrap().unwrap();
            let mut ctrl=tokio_tungstenite::accept_async(stream).await.unwrap();
            let cancel:Value=serde_json::from_str(ctrl.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(cancel["method"],"generate.stop");assert_eq!(cancel["params"]["task_id"],generate["params"]["task_id"]);
            ctrl.send(Message::Text(json!({"id":cancel["id"],"result":{"ok":true}}).to_string())).await.unwrap();
        });
        let directory=tempfile::tempdir().unwrap();let context=test_context(format!("ws://{address}/ws"),directory.path(),1);
        let reply=context.handle_inbound(InboundMessage {source_key:"qq-g-1".into(),text:"hello".into()}).await.unwrap();
        assert!(reply.contains("生成超时"));server.await.unwrap();
    }

    #[tokio::test]
    async fn long_and_output_limited_replies_are_not_truncated() {
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let address=listener.local_addr().unwrap();
        let text="完整中文回复🙂".repeat(1000);let original=text.clone();
        let server=tokio::spawn(async move {
            let (stream,_)=listener.accept().await.unwrap();let mut ws=tokio_tungstenite::accept_async(stream).await.unwrap();
            for limited in [false,true] {
                let req:Value=serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                ws.send(Message::Text(json!({"id":req["id"],"result":{"text":text,"routing":{"status":if limited {"incomplete"} else {"complete"},"error":{"detail":{"type":"output_limit"}}}}}).to_string())).await.unwrap();
            }
        });
        let directory=tempfile::tempdir().unwrap();let context=test_context(format!("ws://{address}/ws"),directory.path(),240);
        let msg=InboundMessage {source_key:"qq-g-1".into(),text:"hello".into()};
        assert_eq!(context.handle_inbound(msg.clone()).await.unwrap(),original);
        let limited=context.handle_inbound(msg).await.unwrap();
        assert!(limited.starts_with(&original));assert!(limited.contains("达到最大输出 Token"));
        server.await.unwrap();
    }

    #[test]
    fn config_env_compat() {
        // 旧名 NAST_QQBOT_* 兜底在 from_env 中生效（此处仅验证默认值路径）
        let cfg = Config {
            nast_server: String::new(),
            avatar: String::new(),
            max_chars: 0,
            gen_timeout_secs: 240,
            platform_help: String::new(),
        };
        assert!(cfg.platform_help.is_empty());
    }
}


fn read_sessions(path: &std::path::Path, server: &str) -> Result<HashMap<String,Session>,String> {
    let bytes = match std::fs::read(path) { Ok(b)=>b, Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(HashMap::new()), Err(e)=>return Err(format!("无法读取会话映射：{e}")) };
    let value:Value=serde_json::from_slice(&bytes).map_err(|e|format!("会话映射损坏，拒绝自动新建聊天：{e}"))?;
    if value["server"] != server { return Err("会话映射属于其他 NAST 服务，请使用不同 BRIDGE_SESSIONS_PATH".into()); }
    value["sessions"].as_object().ok_or("无效会话映射")?.iter().map(|(key,v)|Ok((key.clone(),Session { avatar:v["avatar"].as_str().ok_or("缺少 avatar")?.into(),chat_file:v["chat_file"].as_str().ok_or("缺少 chat_file")?.into() }))).collect()
}
fn save_sessions(path: &std::path::Path, server: &str, sessions: &HashMap<String,Session>) -> Result<(),String> {
    use std::io::Write;
    let parent=path.parent().filter(|p|!p.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e|e.to_string())?;
    let entries:serde_json::Map<String,Value>=sessions.iter().map(|(key,s)|(key.clone(),json!({"avatar":s.avatar,"chat_file":s.chat_file}))).collect();
    let mut file=tempfile::NamedTempFile::new_in(parent).map_err(|e|e.to_string())?;
    file.write_all(serde_json::to_string_pretty(&json!({"server":server,"sessions":entries})).unwrap().as_bytes()).map_err(|e|e.to_string())?;
    file.as_file().sync_all().map_err(|e|e.to_string())?;
    file.persist(path).map_err(|e|e.to_string())?;
    Ok(())
}
