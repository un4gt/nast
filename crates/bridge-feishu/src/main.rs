//! bridge-feishu：飞书适配器占位。
//!
//! 接入时需要实现：BRIDGE_FEISHU_APP_ID / BRIDGE_FEISHU_APP_SECRET →
//! 飞书开放平台事件订阅（长连接模式 wss://open.feishu.cn/… 或 webhook 回调）→
//! tenant_access_token 管理 → bridge_core::handle_inbound →
//! REST `POST /open-apis/im/v1/messages` 回复。

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    tracing::warn!("bridge-feishu 尚未实现：等待飞书事件订阅接入（见 crates/bridge-feishu 模块注释）。");
}
