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

/// 生成控制：同一时刻一个生成（ST 语义），stop 触发 abort；
/// progress 跟踪已流出文本（断线重连恢复流式气泡用）。
#[derive(Default)]
pub struct GenerationControl {
    pub abort: Option<tokio_util::sync::CancellationToken>,
    /// 已累积的流式文本
    pub text: std::sync::Arc<std::sync::Mutex<String>>,
    pub reasoning: std::sync::Arc<std::sync::Mutex<String>>,
    /// 本次生成目标（generate.status 展示）
    pub info: Option<GenerationInfo>,
    pub phase: Arc<std::sync::Mutex<Value>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GenerationInfo {
    pub task_id: String,
    pub kind: String,
    pub avatar: String,
    pub chat_file: String,
    pub is_group: bool,
}

pub struct AppState {
    pub catalog: std::sync::Mutex<crate::model_catalog::Catalog>,
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
    pub fn new(user: UserData, settings: Value, mut secrets: Value) -> Self {
        let catalog = crate::model_catalog::initialize(&user, &settings, &mut secrets).expect("load or migrate model catalog");
        let hub = EventHub::new();
        // 插件宿主：专用线程 + KV 落盘 + toast 接线到事件总线
        let host = nast_plugin::PluginHost::new(
            std::path::PathBuf::from("plugins"),
            user.plugin_kv_path(),
        );
        {
            let hub_cb = hub.clone();
            host.set_toast(std::sync::Arc::new(move |message: &str, kind: &str| {
                hub_cb.emit(
                    "toast",
                    serde_json::json!({"message": message, "type": kind}),
                );
            }));
        }
        match host.load_dir() {
            Ok(names) if !names.is_empty() => {
                tracing::info!("plugins loaded: {:?}", names);
            }
            Err(e) => tracing::warn!("plugin load failed: {e}"),
            _ => {}
        }
        Self {
            catalog: std::sync::Mutex::new(catalog),
            user,
            hub,
            plugins: std::sync::Mutex::new(host),
            settings: RwLock::new(settings),
            secrets: RwLock::new(secrets),
            generation: RwLock::new(GenerationControl::default()),
        }
    }
}
