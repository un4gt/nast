//! WebSocket 会话：单连接读循环 + 广播写循环。
//! 路径 GET /ws；协议见 state.rs 模块注释。

use crate::rpc;
use crate::state::SharedState;
use actix_web::web::{self, Payload};
use actix_web::HttpRequest;
use actix_ws::{AggregatedMessage, Session};
use futures_util::StreamExt;
use serde_json::Value;

pub async fn ws_route(
    req: HttpRequest,
    stream: Payload,
    state: web::Data<SharedState>,
) -> Result<actix_web::HttpResponse, actix_web::Error> {
    // 响应必须立即返回（101 握手），会话循环放入后台任务
    let (response, session, msg_stream) = actix_ws::handle(&req, stream)?;
    // 大消息（卡片 PNG base64 可达数 MB）：放宽 64KB 默认帧上限并聚合分片
    let mut msg_stream = msg_stream
        .max_frame_size(64 * 1024 * 1024)
        .aggregate_continuations();

    let mut hub_rx = state.hub.subscribe();
    let state: SharedState = state.as_ref().clone();

    // 广播任务：把事件推给本连接
    let mut broadcast_session = session.clone();
    let broadcast = actix_web::rt::spawn(async move {
        loop {
            match hub_rx.recv().await {
                Ok(text) => {
                    if broadcast_session.text(text).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // 会话任务
    actix_web::rt::spawn(async move {
        let mut session = session;
        // 读循环
        while let Some(Ok(msg)) = msg_stream.next().await {
            match msg {
                AggregatedMessage::Ping(bytes) => {
                    let _ = session.pong(&bytes).await;
                }
                AggregatedMessage::Text(text) => {
                    let state = state.clone();
                    let session = session.clone();
                    // 逐请求 spawn，避免一个慢 RPC 阻塞读循环
                    actix_web::rt::spawn(async move {
                        handle_rpc(state, session, text.to_string()).await;
                    });
                }
                AggregatedMessage::Close(_) => break,
                _ => {}
            }
        }

        broadcast.abort();
        let _ = session.close(None).await;
    });

    Ok(response)
}

async fn handle_rpc(state: SharedState, mut session: Session, raw: String) {
    let req: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            let _ = session
                .text(serde_json::json!({"error": {"code": "bad_request", "message": e.to_string()}}).to_string())
                .await;
            return;
        }
    };
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let params = req.get("params").cloned().unwrap_or(Value::Null);

    let result = rpc::dispatch(state, &method, params).await;
    let reply = match result {
        Ok(v) => serde_json::json!({"id": id, "result": v}),
        Err(e) => {
            let code = match &e {
                rpc::RpcError::NotFound(_) => "not_found",
                rpc::RpcError::BadRequest(_) => "bad_request",
                rpc::RpcError::Integrity => "integrity",
                rpc::RpcError::Internal(_) => "internal",
            };
            serde_json::json!({"id": id, "error": {"code": code, "message": e.to_string()}})
        }
    };
    let _ = session.text(reply.to_string()).await;
}
