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
use std::collections::HashMap;
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

    /// 调用 RPC：传输层错误（连接重置/关闭/发送失败）作废连接并整体重试至多 3 次。
    /// 业务错误（响应里的 error）不重试，直接返回。
    async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
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
            let id = self.next_id.fetch_add(1, Ordering::Relaxed).to_string();
            let req = json!({"id": id, "method": method, "params": params});
            if let Err(e) = conn.ws.send(Message::Text(req.to_string())).await {
                last_err = format!("send: {e}");
                *guard = None;
                continue;
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
                    // 中途断开/重置：作废连接，外层重试整次调用
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
            // 连接被作废：重试整次调用
        }
        Err(format!("nast RPC unreachable: {last_err}"))
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

/// 每个来源（群/用户）的会话：绑定的角色 + 当前聊天文件。
#[derive(Clone)]
struct Session {
    avatar: String,
    chat_file: String,
}

const QQ_HELP: &str = "命令：
/help —— 本帮助
/chars —— 列出全部角色卡
/char <名字片段> —— 切换到该角色并开新聊天（/character 同义）
/newchat —— 开一个新聊天（同一角色）
/worlds —— 列出全部世界书
/world <名称|none> —— 绑定/解绑本会话的世界书
其余消息直接与当前角色对话；自定义命令见网页 /help。";

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
    let sessions: Arc<Mutex<HashMap<String, Session>>> = Arc::new(Mutex::new(HashMap::new()));

    // 2. 网关循环（断线自动重连）
    loop {
        let reason =
            run_gateway(cfg.clone(), token.clone(), nast.clone(), sessions.clone(), msg_seq.clone())
                .await;
        tracing::warn!("网关连接结束（{reason}），5 秒后重连…");
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

/// 单次网关会话；返回结束原因。
async fn run_gateway(
    cfg: Arc<Config>,
    token: Arc<TokenManager>,
    nast: Arc<NastClient>,
    sessions: Arc<Mutex<HashMap<String, Session>>>,
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
                                    cfg.clone(), token.clone(), nast.clone(), sessions.clone(),
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
    sessions: Arc<Mutex<HashMap<String, Session>>>,
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

    // ---- 命令层（不触发生成） ----
    if text.starts_with('/') {
        let (cmd, args) = match text[1..].split_once(' ') {
            Some((c, a)) => (c.to_lowercase(), a.trim().to_string()),
            None => (text[1..].to_lowercase(), String::new()),
        };
        match cmd.as_str() {
            "help" => {
                let _ = reply(token, &target, QQ_HELP, &msg_id, msg_seq).await;
                return;
            }
            "chars" | "characters" => {
                match nast.call("characters.all", json!({})).await {
                    Ok(list) => {
                        let names: Vec<String> = list
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter(|c| c.get("error").is_none())
                                    .map(|c| {
                                        format!(
                                            "{} {}",
                                            if c.get("fav").and_then(|f| f.as_bool()).unwrap_or(false) { "*" } else { "-" },
                                            c.get("name").and_then(|n| n.as_str()).unwrap_or("?")
                                        )
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        let out = if names.is_empty() { "（无角色卡）".to_string() } else { names.join("
") };
                        let _ = reply(token, &target, &out, &msg_id, msg_seq).await;
                    }
                    Err(e) => {
                        let _ = reply(token, &target, &format!("角色列表获取失败：{e}"), &msg_id, msg_seq).await;
                    }
                }
                return;
            }
            "worlds" => {
                match nast.call("worlds.list", json!({})).await {
                    Ok(list) => {
                        let names: Vec<String> = list
                            .as_array()
                            .map(|a| a.iter().filter_map(|w| w.as_str().map(String::from)).collect())
                            .unwrap_or_default();
                        let out = if names.is_empty() { "（无世界书）".to_string() } else { names.join("
") };
                        let _ = reply(token, &target, &out, &msg_id, msg_seq).await;
                    }
                    Err(e) => {
                        let _ = reply(token, &target, &format!("世界书列表获取失败：{e}"), &msg_id, msg_seq).await;
                    }
                }
                return;
            }
            "world" => {
                let q = args.trim().to_string();
                if q.is_empty() {
                    let cur = sessions
                        .lock()
                        .unwrap()
                        .get(&session_key)
                        .and_then(|sess| {
                            // 读取当前会话聊天的 metadata.world（缓存近期值代价高，直接说明用法）
                            None::<String>
                        });
                    let _ = reply(
                        token, &target,
                        &format!("用法：/world <名称|none>{}", cur.map(|c| format!("
当前绑定：{c}")).unwrap_or_default()),
                        &msg_id, msg_seq,
                    ).await;
                    return;
                }
                let unbind = q.eq_ignore_ascii_case("none");
                let valid = if unbind {
                    true
                } else {
                    nast.call("worlds.list", json!({}))
                        .await
                        .ok()
                        .and_then(|l| l.as_array().cloned())
                        .map(|a| a.iter().any(|w| w.as_str() == Some(q.as_str())))
                        .unwrap_or(false)
                };
                if !valid {
                    let _ = reply(token, &target, &format!("没有名为「{q}」的世界书（/worlds 查看）"), &msg_id, msg_seq).await;
                    return;
                }
                let sess = sessions.lock().unwrap().get(&session_key).cloned();
                let Some(sess) = sess else {
                    let _ = reply(token, &target, "会话尚未初始化，先发一条消息", &msg_id, msg_seq).await;
                    return;
                };
                match nast
                    .call(
                        "chats.set_world",
                        json!({"avatar": sess.avatar, "file_name": sess.chat_file, "world": if unbind { Value::Null } else { json!(q) }}),
                    )
                    .await
                {
                    Ok(_) => {
                        let msg = if unbind { "已解绑世界书".to_string() } else { format!("已绑定世界书：{q}") };
                        let _ = reply(token, &target, &msg, &msg_id, msg_seq).await;
                    }
                    Err(e) => {
                        let _ = reply(token, &target, &format!("绑定失败：{e}"), &msg_id, msg_seq).await;
                    }
                }
                return;
            }
            "newchat" => {
                match reset_session(&cfg, &nast, &sessions, &session_key, None).await {
                    Ok(sess) => {
                        let _ = reply(
                            token, &target,
                            &format!("已开新聊天（角色 {}）", sess.avatar),
                            &msg_id, msg_seq,
                        ).await;
                    }
                    Err(e) => {
                        tracing::error!("/newchat 失败：{e}");
                        let _ = reply(token, &target, &format!("开新聊天失败：{e}"), &msg_id, msg_seq).await;
                    }
                }
                return;
            }
            "char" | "character" => {
                if args.is_empty() {
                    let _ = reply(token, &target, "用法：/char <角色名片段>", &msg_id, msg_seq).await;
                    return;
                }
                match reset_session(&cfg, &nast, &sessions, &session_key, Some(&args)).await {
                    Ok(sess) => {
                        let _ = reply(
                            token, &target,
                            &format!("已切换角色：{}", sess.avatar),
                            &msg_id, msg_seq,
                        ).await;
                    }
                    Err(e) => {
                        tracing::error!("/char 失败：{e}");
                        let _ = reply(token, &target, &format!("切换失败：{e}"), &msg_id, msg_seq).await;
                    }
                }
                return;
            }
            _ => {
                // 自定义命令（power_user.custom_commands）：展开为文本走生成
                if let Some(text) = custom_command_text(&nast, &cmd).await {
                    if !text.is_empty() {
                        if let Some(sess) = sessions.lock().unwrap().get(&session_key).cloned() {
                            match nast.call("generate.run", json!({
                                "avatar": sess.avatar, "chat_file": sess.chat_file,
                                "type": "normal", "user_message": text,
                            })).await {
                                Ok(r) => {
                                    let out = r.get("text").and_then(|t| t.as_str()).unwrap_or_default();
                                    if !out.is_empty() {
                                        let _ = reply(token, &target, &truncate_chars(out, cfg.max_chars), &msg_id, msg_seq).await;
                                    }
                                }
                                Err(e) => {
                                    let _ = reply(token, &target, &format!("生成失败：{e}"), &msg_id, msg_seq).await;
                                }
                            }
                            return;
                        }
                    }
                }
                // 插件命令透传（服务端 Lua 执行）；未知命令拦截
                if !is_plugin_command(&nast, &cmd).await {
                    tracing::info!("[{session_key}] 拦截未知命令 /{cmd}");
                    let _ = reply(
                        token, &target,
                        &format!("未知命令 /{cmd}，发送 /help 查看可用命令。"),
                        &msg_id, msg_seq,
                    ).await;
                    return;
                }
            }
        }
    }

    // ---- 会话（已有）或新建 ----
    let existing = sessions.lock().unwrap().get(&session_key).cloned();
    let (avatar, chat_file) = match existing {
        Some(sess) => (sess.avatar, sess.chat_file),
        None => match reset_session(&cfg, &nast, &sessions, &session_key, None).await {
            Ok(sess) => (sess.avatar, sess.chat_file),
            Err(e) => {
                tracing::error!("会话初始化失败：{e}");
                let _ = reply(token, &target, &format!("会话初始化失败：{e}"), &msg_id, msg_seq).await;
                return;
            }
        },
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
    // 插件命令被服务端处理（无文本产出）时不回复
    if answer.is_empty() && text.starts_with('/') {
        return;
    }
    let answer = truncate_chars(&answer, cfg.max_chars);
    let _ = reply(token, &target, &answer, &msg_id, msg_seq).await;
}


/// 建立或重置会话：选定角色（None = 保持当前/默认）并开新聊天文件。
async fn reset_session(
    cfg: &Config,
    nast: &NastClient,
    sessions: &Arc<Mutex<HashMap<String, Session>>>,
    key: &str,
    avatar_hint: Option<&str>,
) -> Result<Session, String> {
    let avatar = match avatar_hint {
        Some(frag) => find_avatar_by_name(nast, frag).await?,
        None => {
            // /newchat：沿用当前会话角色；无会话则取配置/默认
            match sessions.lock().unwrap().get(key).map(|s| s.avatar.clone()) {
                Some(a) => a,
                None => ensure_avatar(nast, &cfg.avatar).await?,
            }
        }
    };
    let chat_file = fresh_chat(nast, &avatar, key).await?;
    let sess = Session { avatar: avatar.clone(), chat_file: chat_file.clone() };
    sessions.lock().unwrap().insert(key.to_string(), sess.clone());
    tracing::info!("[{key}] 会话重置：{avatar} / {chat_file}");
    Ok(sess)
}

/// 按名字片段找角色。
async fn find_avatar_by_name(nast: &NastClient, frag: &str) -> Result<String, String> {
    let chars = nast
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

/// 新建（而非复用）qq-<key> 聊天：已存在则顺延 -2/-3…。
async fn fresh_chat(nast: &NastClient, avatar: &str, key: &str) -> Result<String, String> {
    let list = nast
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
    let name = if max_n == 0 { key.to_string() } else { format!("{key}-{}", max_n + 1) };
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
        json!({"avatar": avatar, "original_file": file_name, "renamed_file": name}),
    )
    .await
    .map_err(|e| format!("chats.rename: {e}"))?;
    Ok(format!("{name}.jsonl"))
}

/// 是否为插件注册的命令（60s 缓存）。
async fn is_plugin_command(nast: &NastClient, cmd: &str) -> bool {
    use std::sync::OnceLock;
    static CACHE: OnceLock<tokio::sync::Mutex<(std::time::Instant, Vec<String>)>> =
        OnceLock::new();
    let cache = CACHE.get_or_init(|| tokio::sync::Mutex::new((std::time::Instant::now(), vec![])));
    let mut guard = cache.lock().await;
    if guard.0.elapsed() > Duration::from_secs(60) {
        if let Ok(r) = nast.call("plugins.list", json!({})).await {
            let list = (r.get("plugins").and_then(|p| p.as_array()).cloned().unwrap_or_default())
                .iter()
                .flat_map(|p| p.get("commands").and_then(|c| c.as_array()).cloned().unwrap_or_default())
                .filter_map(|c| c.as_str().map(String::from))
                .collect::<Vec<_>>();
            *guard = (std::time::Instant::now(), list);
        }
    }
    guard.1.iter().any(|c| c == cmd)
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

/// 查询自定义命令（power_user.custom_commands，60s 缓存）：命中返回展开文本。
async fn custom_command_text(nast: &NastClient, cmd: &str) -> Option<String> {
    use std::sync::OnceLock;
    static CACHE: OnceLock<tokio::sync::Mutex<(std::time::Instant, Vec<(String, String)>)>> =
        OnceLock::new();
    let cache = CACHE.get_or_init(|| {
        tokio::sync::Mutex::new((std::time::Instant::now(), Vec::new()))
    });
    let mut guard = cache.lock().await;
    if guard.0.elapsed() > Duration::from_secs(60) {
        if let Ok(settings) = nast.call("settings.get", json!({})).await {
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
            *guard = (std::time::Instant::now(), list);
        }
    }
    guard
        .1
        .iter()
        .find(|(n, _)| n == cmd)
        .map(|(_, t)| t.clone())
}
