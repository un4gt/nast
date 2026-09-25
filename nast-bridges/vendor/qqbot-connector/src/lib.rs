//! # qqbot-connector
//!
//! QQ 机器人扫码连接 SDK —— [`@tencent-connect/qqbot-connector`](https://www.npmjs.com/package/@tencent-connect/qqbot-connector)
//! 的 Rust 实现（协议与 [qqbot-go/connector](https://github.com/libaibaia/qqbot-go) 交叉验证）。
//!
//! 在终端展示二维码（`qr` feature），用户使用手机 QQ 扫码完成机器人绑定后，
//! 自动获得机器人的 **AppID** 与 **AppSecret**。
//!
//! ## 快速开始
//!
//! ```no_run
//! use qqbot_connector::{connect, ConnectOptions};
//!
//! // 默认在终端打印二维码，阻塞直到扫码成功。
//! let creds = connect(&ConnectOptions::new())?;
//! println!("AppID: {}", creds.app_id);
//! println!("AppSecret: {}", creds.app_secret);
//! # Ok::<(), qqbot_connector::Error>(())
//! ```
//!
//! 自行渲染二维码（例如生成图片发给用户）：
//!
//! ```no_run
//! use qqbot_connector::{connect, ConnectOptions};
//!
//! let mut options = ConnectOptions::new();
//! options.display_qr_code_to_console = false;
//! options.source = "my-platform".into();
//! options.on_qr_url = Some(Box::new(|url| {
//!     // 拿到扫码链接，自行渲染成二维码
//!     println!("扫码链接: {url}");
//! }));
//! let creds = connect(&options)?;
//! # Ok::<(), qqbot_connector::Error>(())
//! ```
//!
//! 凭据本地缓存 + 自动刷新二维码：
//!
//! ```no_run
//! use qqbot_connector::{load_or_connect, ConnectOptions, FileStore};
//!
//! let store = FileStore::new("qqbot-credentials.json");
//! let creds = load_or_connect(&store, &ConnectOptions::new())?;
//! # Ok::<(), qqbot_connector::Error>(())
//! ```
//!
//! ## 取消
//!
//! [`connect`] 是阻塞调用，通过 [`CancelToken`] 协作式取消（从其他线程或
//! Ctrl-C 处理器调用 [`CancelToken::cancel`]）：
//!
//! ```no_run
//! use qqbot_connector::{connect, CancelToken, ConnectOptions};
//!
//! let cancel = CancelToken::new();
//! let mut options = ConnectOptions::new();
//! options.cancel = cancel.clone();
//! std::thread::spawn(move || {
//!     std::thread::sleep(std::time::Duration::from_secs(60));
//!     cancel.cancel();
//! });
//! let creds = connect(&options); // 60 秒后返回 Err(Error::Cancelled)
//! ```
//!
//! ## 底层接口
//!
//! 与 npm 包逐函数对应，可自由组装：
//!
//! | Rust | npm (`qqbot-session.js`) |
//! |---|---|
//! | [`generate_bind_key`] | `generateBindKey()` |
//! | [`create_bind_task`] | `createBindTask(env)` |
//! | [`poll_bind_result`] | `pollBindResult(taskId, env)` |
//! | [`build_connect_url`] | `buildConnectUrl(taskId, source)` |
//! | [`decrypt_secret`] | `decryptSecret(encrypted, key)` |
//!
//! ## Features
//!
//! - `qr`（默认）：终端二维码渲染，依赖 [`qrcode`] crate。
//!
//! ## 协议
//!
//! 1. `POST https://q.qq.com/lite/create_bind_task`，body `{"key": <32 字节随机数的 base64>}`
//!    → `data.task_id`；
//! 2. 扫码链接 `https://q.qq.com/qqbot/openclaw/connect.html?task_id=...&source=...&_wv=2`
//!    渲染成二维码，等待手机 QQ 扫码；
//! 3. 每 2 秒 `POST /lite/poll_bind_result`，body `{"task_id": ...}`，
//!    返回 `status`（0 等待 / 1 已扫码 / 2 完成 / 3 过期）；
//! 4. 完成后用第 1 步的密钥做 AES-256-GCM 解密
//!    `base64(IV[12] + ciphertext + tag[16])` 得到 AppSecret。

#![forbid(unsafe_code)]

mod base64;
mod connect;
mod crypto;
mod error;
mod http;
mod qr;
mod session;
mod store;

pub use connect::{CancelToken, ConnectOptions, Credentials, OnQrUrl, OnStatus, connect};
pub use crypto::decrypt_secret;
pub use error::{Error, Result};
pub use qr::display_qr_code;
pub use session::{
    BindStatus, BindTask, Env, POLL_INTERVAL, PRODUCTION_HOST, PollResult, REQUEST_TIMEOUT,
    TEST_HOST, build_connect_url, create_bind_task, generate_bind_key, poll_bind_result,
};
pub use store::{FileStore, load_or_connect};

/// 终端二维码渲染（需要 `qr` feature）。
#[cfg(feature = "qr")]
pub use qr::render_qr_terminal;
