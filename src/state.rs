//! WS RPC 协议定义与 AppState。
//!
//! 协议（单端口 /ws，JSON 文本帧）：
//! - 客户端→服务端：{"id": "...", "method": "characters.all", "params": {...}}
//! - 服务端→客户端（响应）：{"id": "...", "result": ...} | {"id": "...", "error": {"code": "...", "message": "..."}}
//! - 服务端→客户端（广播）：{"event": "message_received", "data": ...}
//!   事件名对齐 ST event_types 字符串（见 events.rs）。

use nast_storage::UserData;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

/// 广播通道：server → 所有连接。
#[derive(Clone)]
pub struct EventHub {
    tx: tokio::sync::broadcast::Sender<String>,
}

impl EventHub {
    pub fn new() -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(256);
        Self { tx }
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<String> {
        self.tx.subscribe()
    }

    pub fn emit(&self, event: &str, data: Value) {
        let _ = self.tx.send(serde_json::json!({"event": event, "data": data}).to_string());
    }
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new()
    }
}

/// 生成控制：同一时刻一个生成（ST 语义），stop 触发 abort。
#[derive(Default)]
pub struct GenerationControl {
    pub abort: Option<tokio_util::sync::CancellationToken>,
}

pub struct AppState {
    pub user: UserData,
    pub hub: EventHub,
    pub settings: RwLock<Value>,
    pub generation: RwLock<GenerationControl>,
    /// 生成循环内 → 广播流的专用通道（流式增量走事件总线）
    pub stream_tx: mpsc::Sender<StreamCommand>,
    pub stream_rx: RwLock<mpsc::Receiver<StreamCommand>>,
}

/// 流式过程中的命令。
pub enum StreamCommand {
    Token(Value),
    Finish,
}

pub type SharedState = Arc<AppState>;

impl AppState {
    pub fn new(user: UserData, settings: Value) -> Self {
        let (stream_tx, stream_rx) = mpsc::channel(1024);
        Self {
            user,
            hub: EventHub::new(),
            settings: RwLock::new(settings),
            generation: RwLock::new(GenerationControl::default()),
            stream_tx,
            stream_rx: RwLock::new(stream_rx),
        }
    }
}
