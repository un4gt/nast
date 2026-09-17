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
use tokio::sync::RwLock;

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
    /// 插件宿主（Lua 插件 + Rust 钩子）；Mutex 因 mlua 非线程安全句柄
    pub plugins: std::sync::Mutex<nast_plugin::PluginHost>,
    pub settings: RwLock<Value>,
    /// secrets.json（密钥；值不随 settings 广播）
    pub secrets: RwLock<Value>,
    pub generation: RwLock<GenerationControl>,
}


pub type SharedState = Arc<AppState>;

impl AppState {
    pub fn new(user: UserData, settings: Value, secrets: Value) -> Self {
        // 加载 plugins/ 目录
        let mut host = nast_plugin::PluginHost::new(std::path::PathBuf::from("plugins"));
        match host.load_dir() {
            Ok(names) if !names.is_empty() => {
                tracing::info!("plugins loaded: {:?}", names);
            }
            Err(e) => tracing::warn!("plugin load failed: {e}"),
            _ => {}
        }
        Self {
            user,
            hub: EventHub::new(),
            plugins: std::sync::Mutex::new(host),
            settings: RwLock::new(settings),
            secrets: RwLock::new(secrets),
            generation: RwLock::new(GenerationControl::default()),
        }
    }
}
