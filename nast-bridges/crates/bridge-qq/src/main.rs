//! bridge-qq：QQ 机器人适配器。
//!
//! 平台职责：扫码凭据（qqbot-connector，FileStore 持久化）+ 官方 WebSocket 网关
//! （identify/心跳/resume）+ access token 管理 + 被动回复（v2 接口）。
//! 消息处理全部委托 bridge-core（命令层/会话/生成）。
//!
//! 环境变量：BRIDGE_QQ_CRED_FILE（默认 ./qqbot-credentials.json）；
//! 其余公共配置见 bridge-core::Config::from_env。

mod delivery;
use tracing::Instrument;
use bridge_core::{BridgeContext, Config, InboundMessage};
use futures_util::{SinkExt, StreamExt};
use qqbot_connector::{ConnectOptions, Credentials, FileStore};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

struct DeliveryState {
    outbox: Mutex<delivery::Outbox>,
    busy: tokio::sync::Mutex<()>,
    seen: Mutex<std::collections::VecDeque<String>>,
}
impl DeliveryState {
    fn first_delivery(&self, message: &str) -> bool {
        let mut seen=self.seen.lock().unwrap();
        if seen.iter().any(|id|id==message) { return false; }
        seen.push_back(message.into());
        if seen.len()>1024 { seen.pop_front(); }
        true
    }
}

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
    api_base: String,
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
            api_base: API_BASE.into(),
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
        self.token.lock().unwrap().expires_in = 0;
    }
}

// ---------- 消息清洗（QQ 特有） ----------

/// 剥离 @ 机器人提及（`<@!APPID>` / `<@APPID>`）与 markdown 转义。
pub fn clean_qq_content(raw: &str) -> String {
    let mut s = raw.to_string();
    for pat in ["\\<@!", "<@!", "\\<@", "<@"] {
        loop {
            let Some(start) = s.find(pat) else { break };
            let rest = &s[start + pat.len()..];
            let Some(end) = rest.find('>') else { break };
            let after = &rest[end + 1..];
            s = format!("{}{}", &s[..start], after);
        }
    }
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

enum ReplyTarget {
    Group(String),
    C2C(String),
}

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
            format!("{}/v2/groups/{group_openid}/messages",token.api_base),
            json!({"content": content, "msg_type": 0, "msg_id": msg_id, "msg_seq": seq}),
        ),
        ReplyTarget::C2C(openid) => (
            format!("{}/v2/users/{openid}/messages",token.api_base),
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
        let value:Value=serde_json::from_str(&text).unwrap_or(Value::Null);
        let code=value["code"].as_i64().unwrap_or(0);
        return Err(format!("QQ reply http_status={status} api_code={code}"));
    }
    Ok(())
}

async fn reply(
    token: Arc<TokenManager>,
    target: &ReplyTarget,
    content: &str,
    msg_id: &str,
    seq: u64,
) -> Result<(), String> {
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

// ---------- main ----------

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cfg = Config::from_env("/more — 查看尚未发完的回复（不重新生成）");
    let ctx = BridgeContext::new(cfg);

    // 1. 凭据（扫码绑定，二维码进 stdout；FileStore 缓存持久化在 volume）
    let cred_file = std::env::var("BRIDGE_QQ_CRED_FILE")
        .or_else(|_| std::env::var("NAST_QQBOT_CRED_FILE"))
        .unwrap_or_else(|_| "./qqbot-credentials.json".into());
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
    let path = std::env::var_os("BRIDGE_QQ_OUTBOX_PATH").map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("BRIDGE_SESSIONS_PATH").or_else(||std::env::var_os("BRIDGE_QQ_CRED_FILE"))
                .map(std::path::PathBuf::from).and_then(|path|path.parent().map(|p|p.join("qq-outbox.json")))
                .unwrap_or_else(|| "data/qq-outbox.json".into())
        });
    let outbox=delivery::Outbox::open(path).unwrap_or_else(|e| { tracing::error!("{e}");std::process::exit(1) });
    let delivery = Arc::new(DeliveryState { outbox:Mutex::new(outbox), busy:tokio::sync::Mutex::new(()), seen:Mutex::new(Default::default()) });

    // 2. 网关循环（断线自动重连）
    loop {
        let reason = run_gateway(ctx.clone(), token.clone(), delivery.clone(), GATEWAY_URL).await;
        tracing::warn!("网关连接结束（{reason}），5 秒后重连…");
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

/// 单次网关会话；返回结束原因。
async fn run_gateway(
    ctx: Arc<BridgeContext>,
    token: Arc<TokenManager>,
    delivery: Arc<DeliveryState>,
    gateway_url: &str,
) -> String {
    let (ws, _) = match tokio_tungstenite::connect_async(gateway_url).await {
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

    // Identify (op 2)：新版协议 token = "QQBot {access_token}"（不拼 AppID）
    let access_token = tokio::task::spawn_blocking({
        let t = token.clone();
        move || t.get_blocking()
    })
    .await
    .unwrap()
    .unwrap_or_default();
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

    // 心跳任务（d = 最近收到的 seq，服务端以此校验连接健康）
    let last_seq = Arc::new(AtomicU64::new(0));
    let (hb_tx, mut hb_rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
    let hb_task = tokio::spawn({
        let interval = heartbeat_interval;
        let last_seq = last_seq.clone();
        async move {
            loop {
                tokio::time::sleep(Duration::from_millis(interval)).await;
                let seq = last_seq.load(Ordering::Relaxed);
                let d = if seq > 0 { json!(seq) } else { Value::Null };
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
                        if let Some(seq) = v.get("s").and_then(|s| s.as_u64()) {
                            if seq > 0 {
                                last_seq.store(seq, Ordering::Relaxed);
                            }
                        }
                        let t = v.get("t").and_then(|t| t.as_str()).unwrap_or("");
                        let d = v.get("d").cloned().unwrap_or(Value::Null);
                        if t == "READY" {
                            let bot_name = d
                                .pointer("/user/username")
                                .and_then(|n| n.as_str())
                                .unwrap_or("?");
                            tracing::info!("QQ 机器人已上线：@{bot_name}");
                        } else if t == "RESUMED" {
                            tracing::info!("会话已恢复");
                        } else if t == "GROUP_AT_MESSAGE_CREATE" || t == "C2C_MESSAGE_CREATE" {
                            let ctx=ctx.clone(); let token=token.clone(); let delivery=delivery.clone(); let event=t.to_string();
                            // Keep reading and sending gateway heartbeats while a model is generating.
                            tokio::spawn(async move { handle_message(ctx, token, delivery, &event, d).await; });
                        } else if !t.is_empty() {
                            tracing::debug!("忽略事件 t={t}");
                        }
                    }
                    7 => break "server reconnect requested".into(),
                    9 => {
                        let err = v.get("d").cloned().unwrap_or(Value::Null);
                        tracing::error!("网关拒绝会话（OP9）：{err}");
                        token.invalidate();
                        break "invalid session (OP9)".into();
                    }
                    11 => {}
                    _ => {}
                }
            }
        }
    };

    hb_task.abort();
    result
}

/// 处理一条入站消息：清洗 → bridge-core → 平台回复。
async fn handle_message(
    ctx: Arc<BridgeContext>,
    token: Arc<TokenManager>,
    delivery: Arc<DeliveryState>,
    event: &str,
    d: Value,
) {
    let content = d.get("content").and_then(|c| c.as_str()).unwrap_or("");
    let msg_id = d
        .get("id")
        .and_then(|i| i.as_str())
        .unwrap_or_default()
        .to_string();
    let group_openid = d.get("group_openid").and_then(|g| g.as_str()).map(String::from);
    let openid = d
        .pointer("/author/user_openid")
        .or_else(|| d.pointer("/author/id"))
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
    let source_key = match &target {
        ReplyTarget::Group(g) => format!("qq-g-{g}"),
        ReplyTarget::C2C(o) => format!("qq-u-{o}"),
    };

    if msg_id.is_empty() || !delivery.first_delivery(&msg_id) {
        tracing::info!(event="qq_duplicate_ignored", message_id=%msg_id);
        return;
    }
    let span=tracing::info_span!("qq_message", message_id=%msg_id, source=%source_key);
    async {
        let Ok(_busy) = delivery.busy.try_lock() else {
            if let Err(error)=reply(token.clone(),&target,"正在处理上一条消息，请稍后再发。",&msg_id,1).await {
                tracing::warn!(event="qq_busy_reply_failed",%error);
            }
            return;
        };
        let text=clean_qq_content(content);
        if text=="/more" {
            if delivery.outbox.lock().unwrap().len(&source_key)==0 {
                if let Err(error)=reply(token.clone(),&target,"没有待发送的后续内容。",&msg_id,1).await {
                    tracing::warn!(event="qq_empty_reply_failed",%error);
                }
                return;
            }
        } else {
            let inbound=InboundMessage { source_key:source_key.clone(), text };
            let Some(answer)=ctx.handle_inbound(inbound).await else { return; };
            tracing::info!(event="qq_reply_ready", text_chars=answer.chars().count(), segment_chars=ctx.cfg.max_chars.clamp(128,1500));
            let saved = delivery.outbox.lock().unwrap().enqueue(&source_key,&answer,ctx.cfg.max_chars);
            if let Err(error)=saved {
                tracing::error!(event="qq_outbox_save_failed",%error);
                let _=reply(token.clone(),&target,"回复已生成，但保存待发送记录失败，请在网页查看完整结果。",&msg_id,1).await;
                return;
            }
        }
        // Official passive reply quotas: C2C 4, group 5. Remaining chunks survive restarts.
        let allowed=match target { ReplyTarget::C2C(_)=>4, ReplyTarget::Group(_)=>5 };
        let started=std::time::Instant::now();
        for seq in 1..=allowed {
            let part=delivery.outbox.lock().unwrap().front(&source_key,seq==allowed);
            let Some(part)=part else { break; };
            if seq>1 { tokio::time::sleep(Duration::from_millis(350)).await; }
            match reply(token.clone(),&target,&part,&msg_id,seq).await {
                Ok(())=> {
                    tracing::info!(event="qq_reply_segment_sent",seq,text_chars=part.chars().count());
                    if let Err(error)=delivery.outbox.lock().unwrap().acknowledge(&source_key) {
                        tracing::error!(event="qq_outbox_ack_failed",seq,%error);
                        break;
                    }
                }
                Err(error)=> {
                    // Delivery can be uncertain after a network failure. No automatic resend.
                    tracing::warn!(event="qq_reply_segment_failed",seq,%error,text_chars=part.chars().count(), recovery="send /more; full generation is in web history");
                    break;
                }
            }
        }
        tracing::info!(event="qq_reply_finished",remaining_segments=delivery.outbox.lock().unwrap().len(&source_key),elapsed_ms=started.elapsed().as_millis() as u64);
    }.instrument(span).await;

}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn slow_generation_keeps_heartbeats_and_sends_every_character() {
        use tokio::io::{AsyncReadExt,AsyncWriteExt};
        let dir=tempfile::tempdir().unwrap();
        let nast_listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api_listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let gateway_listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let nast_url=format!("ws://{}/ws",nast_listener.local_addr().unwrap());
        let gateway_url=format!("ws://{}",gateway_listener.local_addr().unwrap());
        let answer="中文🙂长回复".repeat(600);let expected=answer.clone();
        let session_path=dir.path().join("sessions.json");
        std::fs::write(&session_path,json!({"server":nast_url,"sessions":{"qq-u-user":{"avatar":"actor.png","chat_file":"chat.jsonl"}}}).to_string()).unwrap();
        let ctx=BridgeContext::with_session_path(Config { nast_server:nast_url,avatar:"".into(),max_chars:1500,gen_timeout_secs:240,platform_help:"".into() },session_path);
        let delivery=Arc::new(DeliveryState { outbox:Mutex::new(delivery::Outbox::open(dir.path().join("outbox.json")).unwrap()),busy:tokio::sync::Mutex::new(()),seen:Mutex::new(Default::default()) });
        let mut token=TokenManager::new(Credentials { app_id:"test".into(),app_secret:"test".into(),user_openid:None });
        token.api_base=format!("http://{}",api_listener.local_addr().unwrap());
        *token.token.lock().unwrap()=TokenState {access_token:"test-token".into(),fetched_at:std::time::Instant::now(),expires_in:7200};
        let (started_tx,started_rx)=tokio::sync::oneshot::channel();
        let nast=tokio::spawn(async move {
            let (stream,_)=nast_listener.accept().await.unwrap();let mut ws=tokio_tungstenite::accept_async(stream).await.unwrap();
            let req:Value=serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(req["method"],"generate.run");started_tx.send(()).unwrap();
            tokio::time::sleep(Duration::from_millis(500)).await;
            ws.send(Message::Text(json!({"id":req["id"],"result":{"text":answer,"routing":{"status":"complete"}}}).to_string())).await.unwrap();
            tokio::time::sleep(Duration::from_secs(2)).await;
        });
        let (delivered_tx,delivered_rx)=tokio::sync::oneshot::channel();
        let api=tokio::spawn(async move {
            let mut sent=String::new();
            for seq in 1..=3 {
                let (mut socket,_)=api_listener.accept().await.unwrap();let mut data=Vec::new();
                let header_end=loop {
                    let mut buf=[0;4096];let n=socket.read(&mut buf).await.unwrap();assert!(n>0);data.extend_from_slice(&buf[..n]);
                    if let Some(i)=data.windows(4).position(|w|w==b"\r\n\r\n") { break i+4; }
                };
                let headers=std::str::from_utf8(&data[..header_end]).unwrap().to_lowercase();
                assert!(headers.starts_with("post /v2/users/user/messages "));
                let len:usize=headers.lines().find_map(|line|line.strip_prefix("content-length: ")).unwrap().parse().unwrap();
                while data.len()<header_end+len { let mut buf=[0;4096];let n=socket.read(&mut buf).await.unwrap();assert!(n>0);data.extend_from_slice(&buf[..n]); }
                let body:Value=serde_json::from_slice(&data[header_end..header_end+len]).unwrap();
                assert_eq!(body["msg_seq"],seq);assert_eq!(body["msg_id"],"message-one");
                sent.push_str(body["content"].as_str().unwrap());
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
            }
            assert_eq!(sent,expected);delivered_tx.send(()).unwrap();
        });
        let gateway=tokio::spawn(async move {
            let (stream,_)=gateway_listener.accept().await.unwrap();let mut ws=tokio_tungstenite::accept_async(stream).await.unwrap();
            ws.send(Message::Text(json!({"op":10,"d":{"heartbeat_interval":30}}).to_string())).await.unwrap();
            let identify:Value=serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();assert_eq!(identify["op"],2);
            let message=json!({"op":0,"s":1,"t":"C2C_MESSAGE_CREATE","d":{"id":"message-one","author":{"user_openid":"user"},"content":"hello"}}).to_string();
            ws.send(Message::Text(message.clone())).await.unwrap();ws.send(Message::Text(message)).await.unwrap(); // duplicate event must not regenerate
            started_rx.await.unwrap();
            for _ in 0..3 {
                let heartbeat=tokio::time::timeout(Duration::from_millis(150),ws.next()).await.expect("generation must not block heartbeat").unwrap().unwrap();
                let body:Value=serde_json::from_str(heartbeat.to_text().unwrap()).unwrap();assert_eq!(body["op"],1);
            }
            tokio::time::timeout(Duration::from_secs(5),delivered_rx).await.unwrap().unwrap();
            ws.send(Message::Text(json!({"op":7}).to_string())).await.unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
        });
        assert_eq!(run_gateway(ctx,Arc::new(token),delivery.clone(),&gateway_url).await,"server reconnect requested");
        gateway.await.unwrap();api.await.unwrap();nast.await.unwrap();
        assert_eq!(delivery.outbox.lock().unwrap().len("qq-u-user"),0);
    }

    #[test]
    fn cleans_mentions_and_escapes() {
        assert_eq!(clean_qq_content("<@!123456> 你好\\*世界\\*"), "你好*世界*");
        assert_eq!(clean_qq_content("\\<@!123\\> /hi"), "/hi");
        assert_eq!(clean_qq_content("  普通消息  "), "普通消息");
        assert_eq!(clean_qq_content("路径 C:\\temp 保持"), "路径 C:\\temp 保持");
    }
}
