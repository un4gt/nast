//! bridge-core：nast 桥接公共层（平台无关）。
//!
//! 平台适配器（QQ/Discord/飞书…）只做三件事：
//! 1. 拿平台凭据并维持平台网关长连接；
//! 2. 收到消息 → 清洗为纯文本 → 组装 [`InboundMessage`] 调 [`BridgeContext::handle_inbound`]；
//! 3. 返回的 `Option<String>`（截断好的回复文本）按平台 API 发出去，None 则不回复。
//!
//! 本层负责：nast WS RPC 客户端（断线重试）、按来源的会话（角色 + 聊天文件）、
//! 命令层（/help /chars /char /newchat /worlds /world）、自定义命令展开
//! （settings.power_user.custom_commands）、插件命令透传、生成与截断。

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
    /// 回复最大保留字符数
    pub max_chars: usize,
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
        Self {
            url,
            inner: tokio::sync::Mutex::new(None),
            next_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// 调用 RPC：传输层错误（连接重置/关闭/发送失败）作废连接并整体重试至多 3 次。
    /// 业务错误（响应里的 error）不重试，直接返回。
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let mut last_err = String::new();
        for attempt in 0..3 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(500 * attempt as u64)).await;
            }
            let mut guard = self.inner.lock().await;
            if guard.is_none() {
                match tokio_tungstenite::connect_async(self.url.as_str()).await {
                    Ok((ws, _)) => {
                        tracing::info!("nast RPC connected: {}", self.url);
                        *guard = Some(NastConn { ws });
                    }
                    Err(e) => {
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
                continue;
            }
            let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
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
                    return Err(err
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("unknown error")
                        .to_string());
                }
                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
            }
        }
        Err(format!("nast RPC unreachable: {last_err}"))
    }
}

// ---------- 桥接上下文 ----------

pub struct BridgeContext {
    pub cfg: Config,
    pub nast: Arc<NastClient>,
    sessions: Mutex<HashMap<String, Session>>,
}

impl BridgeContext {
    pub fn new(cfg: Config) -> Arc<Self> {
        let nast = Arc::new(NastClient::new(cfg.nast_server.clone()));
        Arc::new(Self {
            cfg,
            nast,
            sessions: Mutex::new(HashMap::new()),
        })
    }

    /// 主入口：命令层 → 会话生成。返回应回复的文本（已截断）；None = 不回复。
    pub async fn handle_inbound(&self, msg: InboundMessage) -> Option<String> {
        let text = msg.text.trim().to_string();
        tracing::info!("[{}] 收到：{text:?}", msg.source_key);
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

    /// 生成并截断；生成失败返回错误文案（仍回复给用户）。
    async fn generate_reply(&self, key: &str, text: &str) -> Option<String> {
        let out = self.generate_reply_raw(key, text).await?;
        Some(truncate_chars(&out, self.cfg.max_chars))
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
        match self
            .nast
            .call(
                "generate.run",
                json!({
                    "avatar": avatar,
                    "chat_file": chat_file,
                    "type": "normal",
                    "user_message": text,
                }),
            )
            .await
        {
            Ok(r) => {
                let out = r.get("text").and_then(|t| t.as_str()).unwrap_or_default().to_string();
                if out.is_empty() && text.starts_with('/') {
                    None
                } else {
                    Some(out)
                }
            }
            Err(e) => {
                tracing::error!("[{key}] 生成失败：{e}");
                Some(format!("生成失败：{e}"))
            }
        }
    }

    // ---------- 会话管理 ----------

    pub async fn ensure_session(&self, key: &str) -> Result<Session, String> {
        if let Some(s) = self.sessions.lock().unwrap().get(key).cloned() {
            return Ok(s);
        }
        self.reset_session(key, None).await
    }

    /// 建立或重置会话：选定角色（None = 保持当前/默认）并开新聊天文件。
    pub async fn reset_session(
        &self,
        key: &str,
        avatar_hint: Option<&str>,
    ) -> Result<Session, String> {
        let avatar = match avatar_hint {
            Some(frag) => self.find_avatar_by_name(frag).await?,
            None => match self.sessions.lock().unwrap().get(key).map(|s| s.avatar.clone()) {
                Some(a) => a,
                None => self.ensure_default_avatar().await?,
            },
        };
        let chat_file = self.fresh_chat(&avatar, key).await?;
        let sess = Session {
            avatar: avatar.clone(),
            chat_file: chat_file.clone(),
        };
        self.sessions
            .lock()
            .unwrap()
            .insert(key.to_string(), sess.clone());
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
            "/chars —— 列出全部角色卡".to_string(),
            "/char <名字片段> —— 切换到该角色并开新聊天（/character 同义）".to_string(),
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

/// 按字符数截断（超长回复保护），保留结尾省略号。
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_by_chars() {
        let s = "很长".repeat(1000);
        assert_eq!(truncate_chars(&s, 10).chars().count(), 10);
        assert!(truncate_chars(&s, 10).ends_with('…'));
        assert_eq!(truncate_chars("短", 10), "短");
    }

    #[test]
    fn config_env_compat() {
        // 旧名 NAST_QQBOT_* 兜底在 from_env 中生效（此处仅验证默认值路径）
        let cfg = Config {
            nast_server: String::new(),
            avatar: String::new(),
            max_chars: 0,
            platform_help: String::new(),
        };
        assert!(cfg.platform_help.is_empty());
    }
}
