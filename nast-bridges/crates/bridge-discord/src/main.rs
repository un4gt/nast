//! bridge-discord：Discord 适配器占位。
//!
//! 接入时需要实现：BRIDGE_DISCORD_TOKEN 环境变量 → Discord Gateway（wss://gateway.discord.gg）
//! WebSocket（identify/心跳/resume、消息事件 MESSAGE_CREATE / INTERACTION_CREATE）→
//! bridge_core::handle_inbound → REST `POST /channels/{id}/messages` 回复。
//! 凭据无需扫码，Token 即环境变量。

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    tracing::warn!("bridge-discord 尚未实现：等待 Discord Gateway 接入（见 crates/bridge-discord 模块注释）。");
}
