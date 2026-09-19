//! nast-qqbot：QQ 机器人 ↔ nast 桥接（独立进程，容器友好的第二个可执行文件）。
//!
//! 流程：
//! 1. 凭据：qqbot-connector 扫码绑定（二维码打印到 stdout，docker logs 可见），
//!    FileStore 本地缓存（volume 持久化，容器重启免扫码）；
//! 2. 网关：QQ 官方 WebSocket（api.sgroup.qq.com）——identify / 心跳 / resume；
//! 3. 桥接：群 @ 消息与单聊消息 → 清理文本 → 经 nast WS RPC 走角色生成
//!    （每个发送者一个聊天文件 qq-<openid>）→ 被动回复（v2 接口，带 msg_id）。
//!
//! 环境变量：
//! - NAST_QQBOT_SERVER      nast WS 地址（默认 ws://127.0.0.1:8000/ws）
//! - NAST_QQBOT_AVATAR      使用的角色卡文件名（缺省 = 第一个角色）
//! - NAST_QQBOT_CRED_FILE   凭据缓存文件（默认 ./qqbot-credentials.json）
//! - NAST_QQBOT_MAX_TOK     回复最大保留字符（默认 1500）

use futures_util::{SinkExt, StreamExt};
use qqbot_connector::{ConnectOptions, Credentials, FileStore};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

const GATEWAY_URL: &str = "wss://api.sgroup.qq.com/websocket";
const TOKEN_URL: &str = "https://bots.qq.com/app/getAppAccessToken";
const API_BASE: &str = "https://api.sgroup.qq.com";
/// 群聊 + 单聊（C2C）事件
const INTENT_GROUP_C2C: u32 = 1 << 25;

fn http_agent() -> &'static ureq::Agent {
    use std::sync::OnceLock;
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .http_status_as_error(false)
            .build()
            .new_agent()
    })
}

// ---------- access token 管理 ----------

struct TokenManager {
    creds: Credentials,
    token: Mutex<TokenState>,
}

#[derive(Clone)]
struct TokenState {
    access_token: String,
    fetched_at: std::time::Instant,
    expires_in: u64,
}

impl TokenManager {
    fn new(creds: Credentials) -> Self {
        Self {
            creds,
            token: Mutex::new(TokenState {
                access_token: String::new(),
                fetched_at: std::time::Instant::now() - Duration::from_secs(10),
                expires_in: 0,
            }),
        }
    }

    /// 取有效 token（提前 120s 刷新；阻塞 HTTP，调用方放 spawn_blocking）。
    fn get_blocking(&self) -> Result<String, String> {
        let needs_refresh = {
            let t = self.token.lock().unwrap();
            t.access_token.is_empty()
                || t.fetched_at.elapsed()
                    > Duration::from_secs(t.expires_in.saturating_sub(120))
        };
        if needs_refresh {
            let body = json!({
                "appId": self.creds.app_id,
                "clientSecret": self.creds.app_secret,
            });
            let payload = serde_json::to_vec(&body).map_err(|e| e.to_string())?;
            let resp = http_agent()
                .post(TOKEN_URL)
                .header("Content-Type", "application/json")
                .send(&payload)
                .map_err(|e| format!("token request: {e}"))?;
            let status = resp.status().as_u16();
            let text = resp
                .into_body()
                .read_to_string()
                .map_err(|e| e.to_string())?;
            if status != 200 {
                return Err(format!("token http {status}: {text}"));
            }
            let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            // 腾讯接口已知坑：access_token 可能带尾部 \r\n 等空白，拼进 identify 会 OP9
            let access_token = v
                .get("access_token")
                .and_then(|t| t.as_str())
                .ok_or("token response missing access_token")?
                .trim()
                .to_string();
            let expires_in = v
                .get("expires_in")
                .and_then(|e| e.as_u64().or_else(|| e.as_str().and_then(|s| s.parse().ok())))
                .unwrap_or(7200);
            *self.token.lock().unwrap() = TokenState {
                access_token,
                fetched_at: std::time::Instant::now(),
                expires_in,
            };
            tracing::info!("access token refreshed (expires in {expires_in}s)");
        }
        Ok(self.token.lock().unwrap().access_token.clone())
    }

    /// 网关判定会话无效（OP9）时强制下次重新获取 token。
    fn invalidate(&self) {
        let mut t = self.token.lock().unwrap();
        t.expires_in = 0;
    }
}

// ---------- nast WS RPC 客户端 ----------

struct NastClient {
    url: String,
    inner: tokio::sync::Mutex<Option<NastConn>>,
    next_id: AtomicU64,
}

struct NastConn {
    ws: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    // 简化：一请求一等待（桥接是串行会话，不需要并发管道）
}

impl NastClient {
    fn new(url: String) -> Self {
        Self {
            url,
            inner: tokio::sync::Mutex::new(None),
            next_id: AtomicU64::new(1),
        }
    }

    /// 调用 RPC（自动重连一次）。返回 result；错误返回 Err(message)。
    async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        for attempt in 0..2 {
            let mut guard = self.inner.lock().await;
            if guard.is_none() {
                match tokio_tungstenite::connect_async(self.url.as_str()).await {
                    Ok((ws, _)) => {
                        tracing::info!("nast RPC connected: {}", self.url);
                        *guard = Some(NastConn { ws });
                    }
                    Err(e) => {
                        drop(guard);
                        if attempt == 0 {
                            tokio::time::sleep(Duration::from_secs(2)).await;
                            continue;
                        }
                        return Err(format!("connect nast: {e}"));
                    }
                }
            }
            let conn = guard.as_mut().unwrap();
            let id = self.next_id.fetch_add(1, Ordering::Relaxed).to_string();
            let req = json!({"id": id, "method": method, "params": params});
            if let Err(e) = conn.ws.send(Message::Text(req.to_string())).await {
                *guard = None;
                if attempt == 0 {
                    continue;
                }
                return Err(format!("send: {e}"));
            }
            // 读到对应 id 的响应（跳过事件帧）
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
                        *guard = None;
                        break;
                    }
                    Ok(_) => continue,
                    Err(e) => {
                        *guard = None;
                        return Err(format!("nast RPC read: {e}"));
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
            // 连接被关闭：重试一次
        }
        Err("nast RPC unreachable".into())
    }
}

// ---------- 消息内容清理 ----------

/// 剥离 @ 机器人提及（`<@!APPID>` / `<@APPID>`）与 markdown 转义。
pub fn clean_qq_content(raw: &str) -> String {
    let mut s = raw.to_string();
    // 提及标记（含可能的转义形态 \<@!xxx\>）
    for pat in ["\\<@!", "<@!", "\\<@", "<@"] {
        loop {
            let Some(start) = s.find(pat) else { break };
            let rest = &s[start + pat.len()..];
            let Some(end) = rest.find('>') else { break };
            let after = &rest[end + 1..];
            s = format!("{}{}", &s[..start], after);
        }
    }
    // QQ 会转义 markdown 特殊字符：去掉反斜杠
    let special = ['*', '_', '~', '@', '#', '>', '[', ']', '(', ')', '-', '\\'];
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                if special.contains(&next) {
                    out.push(next);
                    chars.next();
                    continue;
                }
            }
        }
        out.push(c);
    }
    out.trim().to_string()
}

// ---------- 回复发送（v2 被动） ----------

fn send_reply_blocking(
    token: &TokenManager,
    target: &ReplyTarget,
    content: &str,
    msg_id: &str,
    seq: u64,
) -> Result<(), String> {
    let access_token = token.get_blocking()?;
    let (url, body) = match target {
        ReplyTarget::Group(group_openid) => (
            format!("{API_BASE}/v2/groups/{group_openid}/messages"),
            json!({"content": content, "msg_type": 0, "msg_id": msg_id, "msg_seq": seq}),
        ),
        ReplyTarget::C2C(openid) => (
            format!("{API_BASE}/v2/users/{openid}/messages"),
            json!({"content": content, "msg_type": 0, "msg_id": msg_id, "msg_seq": seq}),
        ),
    };
    let payload = serde_json::to_vec(&body).map_err(|e| e.to_string())?;
    let resp = http_agent()
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("QQBot {access_token}"))
        .send(&payload)
        .map_err(|e| format!("send reply: {e}"))?;
    let status = resp.status().as_u16();
    let text = resp.into_body().read_to_string().map_err(|e| e.to_string())?;
    if status != 200 && status != 201 && status != 204 {
        return Err(format!("reply http {status}: {text}"));
    }
    Ok(())
}

enum ReplyTarget {
    Group(String),
    C2C(String),
}

// ---------- main ----------

struct Config {
    server: String,
    avatar: String,
    cred_file: String,
    max_chars: usize,
}

impl Config {
    fn from_env() -> Self {
        Self {
            server: std::env::var("NAST_QQBOT_SERVER")
                .unwrap_or_else(|_| "ws://127.0.0.1:8000/ws".into()),
            avatar: std::env::var("NAST_QQBOT_AVATAR").unwrap_or_default(),
            cred_file: std::env::var("NAST_QQBOT_CRED_FILE")
                .unwrap_or_else(|_| "./qqbot-credentials.json".into()),
            max_chars: std::env::var("NAST_QQBOT_MAX_TOK")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1500),
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cfg = Arc::new(Config::from_env());

    // 1. 凭据（扫码绑定，二维码进 stdout）
    let cred_file = cfg.cred_file.clone();
    let creds = tokio::task::spawn_blocking(move || {
        let store = FileStore::new(&cred_file);
        if let Some(c) = store.load().ok().flatten() {
            tracing::info!("已加载缓存的机器人凭据（{}）", c.app_id);
            return Ok(c);
        }
        tracing::info!("未找到凭据缓存 —— 请用手机 QQ 扫描下方二维码绑定机器人…");
        qqbot_connector::load_or_connect(&store, &ConnectOptions::new())
    })
    .await
    .unwrap()
    .unwrap_or_else(|e| {
        tracing::error!("凭据获取失败：{e}");
        std::process::exit(1);
    });
    tracing::info!("机器人已绑定：AppID {}", creds.app_id);

    let token = Arc::new(TokenManager::new(creds));
    let nast = Arc::new(NastClient::new(cfg.server.clone()));
    let msg_seq = Arc::new(AtomicU64::new(1));

    // 2. 网关循环（断线自动重连）
    loop {
        let reason = run_gateway(cfg.clone(), token.clone(), nast.clone(), msg_seq.clone()).await;
        tracing::warn!("网关连接结束（{reason}），5 秒后重连…");
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

/// 单次网关会话；返回结束原因。
async fn run_gateway(
    cfg: Arc<Config>,
    token: Arc<TokenManager>,
    nast: Arc<NastClient>,
    msg_seq: Arc<AtomicU64>,
) -> String {
    let (ws, _) = match tokio_tungstenite::connect_async(GATEWAY_URL).await {
        Ok(v) => v,
        Err(e) => return format!("connect: {e}"),
    };
    let (mut sink, mut stream) = ws.split();

    // 等 Hello (op 10)
    let hello = tokio::select! {
        m = stream.next() => match m {
            Some(Ok(Message::Text(t))) => serde_json::from_str::<Value>(&t).ok(),
            _ => return "hello failed".into(),
        },
        _ = tokio::time::sleep(Duration::from_secs(15)) => return "hello timeout".into(),
    };
    let heartbeat_interval = hello
        .as_ref()
        .and_then(|v| v.get("d"))
        .and_then(|d| d.get("heartbeat_interval"))
        .and_then(|i| i.as_u64())
        .unwrap_or(30000);
    let session_id = Arc::new(Mutex::new(String::new()));
    let last_seq = Arc::new(AtomicU64::new(0));

    // Identify (op 2)
    let access_token = tokio::task::spawn_blocking({
        let t = token.clone();
        move || t.get_blocking()
    })
    .await
    .unwrap()
    .unwrap_or_default();
    // 新版 QQ 开放平台协议：token = "QQBot {access_token}"（不再拼 AppID）
    let identify = json!({
        "op": 2,
        "d": {
            "token": format!("QQBot {access_token}"),
            "intents": INTENT_GROUP_C2C,
            "shard": [0, 1],
        }
    });
    if sink.send(Message::Text(identify.to_string())).await.is_err() {
        return "identify send failed".into();
    }

    // 心跳任务
    let (hb_tx, mut hb_rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
    let hb_task = tokio::spawn({
        let interval = heartbeat_interval;
        let last_seq = last_seq.clone();
        async move {
            loop {
                tokio::time::sleep(Duration::from_millis(interval)).await;
                let seq = last_seq.load(Ordering::Relaxed);
                let d: Value = if seq > 0 { json!(seq) } else { Value::Null };
                let hb = json!({"op": 1, "d": d});
                if hb_tx.send(Message::Text(hb.to_string())).is_err() {
                    break;
                }
            }
        }
    });

    let result: String = loop {
        tokio::select! {
            hb = hb_rx.recv() => {
                if let Some(m) = hb {
                    if sink.send(m).await.is_err() {
                        break "heartbeat send failed".into();
                    }
                }
            }
            msg = stream.next() => {
                let msg = match msg {
                    Some(Ok(m)) => m,
                    Some(Err(e)) => break format!("read: {e}"),
                    None => break "closed".into(),
                };
                let text = match msg {
                    Message::Text(t) => t,
                    Message::Ping(p) => {
                        let _ = sink.send(Message::Pong(p)).await;
                        continue;
                    }
                    Message::Close(_) => break "close frame".into(),
                    _ => continue,
                };
                let v: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let op = v.get("op").and_then(|o| o.as_u64()).unwrap_or(0);
                match op {
                    0 => {
                        let seq = v.get("s").and_then(|s| s.as_u64()).unwrap_or(0);
                        if seq > 0 {
                            last_seq.store(seq, Ordering::Relaxed);
                        }
                        let t = v.get("t").and_then(|t| t.as_str()).unwrap_or("");
                        let d = v.get("d").cloned().unwrap_or(Value::Null);
                        match t {
                            "READY" => {
                                let sid = d.get("session_id").and_then(|s| s.as_str()).unwrap_or("");
                                *session_id.lock().unwrap() = sid.to_string();
                                let bot_name = d
                                    .pointer("/user/username")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("?");
                                tracing::info!("QQ 机器人已上线：@{bot_name}");
                            }
                            "RESUMED" => tracing::info!("会话已恢复"),
                            "GROUP_AT_MESSAGE_CREATE" | "C2C_MESSAGE_CREATE" => {
                                handle_message(
                                    cfg.clone(), token.clone(), nast.clone(),
                                    msg_seq.clone(), t, d,
                                ).await;
                            }
                            _ => {}
                        }
                    }
                    7 => break "server reconnect requested".into(),
                    9 => {
                        // OP9：鉴权/intent 无效。作废 token 缓存（重连时强制重取）并记录服务端错误详情。
                        let err = v.get("d").cloned().unwrap_or(Value::Null);
                        tracing::error!("网关拒绝会话（OP9）：{err}");
                        token.invalidate();
                        break "invalid session (OP9)".into();
                    }
                    11 => {} // 心跳 ACK
                    _ => {}
                }
            }
        }
    };

    hb_task.abort();
    result
}

/// 处理一条入站消息：清理 → 定位/创建聊天 → 生成 → 回复。
async fn handle_message(
    cfg: Arc<Config>,
    token: Arc<TokenManager>,
    nast: Arc<NastClient>,
    msg_seq: Arc<AtomicU64>,
    event: &str,
    d: Value,
) {
    let content = d.get("content").and_then(|c| c.as_str()).unwrap_or("");
    let msg_id = d.get("id").and_then(|i| i.as_str()).unwrap_or_default().to_string();
    let group_openid = d.get("group_openid").and_then(|g| g.as_str()).map(String::from);
    let openid = d
        .pointer("/author/id")
        .or_else(|| d.pointer("/author/openid"))
        .or_else(|| d.pointer("/author/member_openid"))
        .and_then(|o| o.as_str())
        .unwrap_or("unknown")
        .to_string();

    let target = match (event, group_openid) {
        ("GROUP_AT_MESSAGE_CREATE", Some(g)) => ReplyTarget::Group(g),
        ("C2C_MESSAGE_CREATE", _) => ReplyTarget::C2C(openid),
        _ => return,
    };

    // 会话键：群 = 群 openid；单聊 = 用户 openid（每个来源独立聊天文件）
    let session_key = match &target {
        ReplyTarget::Group(g) => format!("qq-g-{g}"),
        ReplyTarget::C2C(o) => format!("qq-u-{o}"),
    };
    let text = clean_qq_content(content);
    tracing::info!("[{session_key}] 收到：{text:?}");
    if text.is_empty() {
        let _ = reply(token.clone(), &target, "（空消息）", &msg_id, msg_seq).await;
        return;
    }

    // 角色 + 聊天定位
    let avatar = match ensure_avatar(&nast, &cfg.avatar).await {
        Ok(a) => a,
        Err(e) => {
            tracing::error!("{e}");
            let _ = reply(token, &target, "服务端没有可用的角色卡，请先导入。", &msg_id, msg_seq).await;
            return;
        }
    };
    let chat_file = match ensure_chat(&nast, &avatar, &session_key).await {
        Ok(f) => f,
        Err(e) => {
            tracing::error!("聊天定位失败：{e}");
            let _ = reply(token, &target, "内部错误：聊天会话创建失败。", &msg_id, msg_seq).await;
            return;
        }
    };

    // 生成
    let gen_result = nast
        .call(
            "generate.run",
            json!({
                "avatar": avatar,
                "chat_file": chat_file,
                "type": "normal",
                "user_message": text,
            }),
        )
        .await;
    let answer = match gen_result {
        Ok(r) => r.get("text").and_then(|t| t.as_str()).unwrap_or_default().to_string(),
        Err(e) => {
            tracing::error!("生成失败：{e}");
            format!("生成失败：{e}")
        }
    };
    let answer = truncate_chars(&answer, cfg.max_chars);
    let _ = reply(token, &target, &answer, &msg_id, msg_seq).await;
}

async fn reply(
    token: Arc<TokenManager>,
    target: &ReplyTarget,
    content: &str,
    msg_id: &str,
    msg_seq: Arc<AtomicU64>,
) -> Result<(), String> {
    let seq = msg_seq.fetch_add(1, Ordering::Relaxed);
    let target_clone = match target {
        ReplyTarget::Group(g) => ReplyTarget::Group(g.clone()),
        ReplyTarget::C2C(o) => ReplyTarget::C2C(o.clone()),
    };
    let content = content.to_string();
    let msg_id = msg_id.to_string();
    tokio::task::spawn_blocking(move || {
        send_reply_blocking(&token, &target_clone, &content, &msg_id, seq)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 选定角色：环境变量指定 > 第一个可用角色。
async fn ensure_avatar(nast: &NastClient, preferred: &str) -> Result<String, String> {
    let chars = nast
        .call("characters.all", json!({}))
        .await
        .map_err(|e| format!("characters.all: {e}"))?;
    let Some(arr) = chars.as_array() else {
        return Err("characters.all 返回异常".into());
    };
    if !preferred.is_empty() {
        let found = arr
            .iter()
            .any(|c| c.get("avatar").and_then(|a| a.as_str()) == Some(preferred));
        if found {
            return Ok(preferred.to_string());
        }
        return Err(format!("NAST_QQBOT_AVATAR 指定的角色不存在：{preferred}"));
    }
    arr.iter()
        .find(|c| c.get("error").is_none())
        .and_then(|c| c.get("avatar"))
        .and_then(|a| a.as_str())
        .map(String::from)
        .ok_or_else(|| "服务端没有任何角色卡".into())
}

/// 定位（或创建并改名）桥接聊天 qq-<key>。
async fn ensure_chat(nast: &NastClient, avatar: &str, key: &str) -> Result<String, String> {
    let list = nast
        .call("characters.chats", json!({"avatar": avatar}))
        .await
        .map_err(|e| format!("characters.chats: {e}"))?;
    if let Some(arr) = list.as_array() {
        if let Some(f) = arr
            .iter()
            .filter_map(|f| f.as_str())
            .find(|f| f == &format!("{key}.jsonl"))
        {
            return Ok(f.to_string());
        }
    }
    let created = nast
        .call("chats.new", json!({"avatar": avatar, "greeting_index": -1}))
        .await
        .map_err(|e| format!("chats.new: {e}"))?;
    let file_name = created
        .get("file_name")
        .and_then(|f| f.as_str())
        .ok_or("chats.new 缺少 file_name")?
        .to_string();
    nast.call(
        "chats.rename",
        json!({"avatar": avatar, "original_file": file_name, "renamed_file": key}),
    )
    .await
    .map_err(|e| format!("chats.rename: {e}"))?;
    Ok(format!("{key}.jsonl"))
}

/// 按字符数截断（QQ 消息长度限制），保留结尾省略号。
fn truncate_chars(s: &str, max: usize) -> String {
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
    fn cleans_mentions_and_escapes() {
        assert_eq!(
            clean_qq_content("<@!123456> 你好\\*世界\\*"),
            "你好*世界*"
        );
        assert_eq!(clean_qq_content("\\<@!123\\> /hi"), "/hi");
        assert_eq!(clean_qq_content("  普通消息  "), "普通消息");
        assert_eq!(clean_qq_content("路径 C:\\temp 保持"), "路径 C:\\temp 保持");
    }

    #[test]
    fn truncates_by_chars() {
        let s = "很长".repeat(1000);
        assert_eq!(truncate_chars(&s, 10).chars().count(), 10);
        assert!(truncate_chars(&s, 10).ends_with('…'));
        assert_eq!(truncate_chars("短", 10), "短");
    }
}
