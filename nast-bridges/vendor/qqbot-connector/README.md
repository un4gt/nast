# qqbot-connector

QQ 机器人扫码连接 SDK —— [`@tencent-connect/qqbot-connector`](https://www.npmjs.com/package/@tencent-connect/qqbot-connector) 的 Rust 实现（协议与 [qqbot-go/connector](https://github.com/libaibaia/qqbot-go) 交叉验证）。

在终端展示二维码，用户使用手机 QQ 扫码完成机器人绑定后，自动获得机器人的 **AppID** 与 **AppSecret**。

默认情况下扫码页面会将接入方统一显示为"第三方机器人"。如需在扫码页面展示你的平台名称，或有其他商务合作需求，请通过邮件与腾讯联系：qq_bot_api@tencent.com。

## 安装

```bash
cargo add qqbot-connector
```

## 快速开始

### 阻塞式（对应 npm 的 Promise 风格 `qrConnect`）

默认在终端打印二维码，阻塞直到扫码成功：

```rust
use qqbot_connector::{connect, ConnectOptions};

let creds = connect(&ConnectOptions::new())?;
println!("绑定成功！");
println!("AppID:     {}", creds.app_id);
println!("AppSecret: {}", creds.app_secret);
```

### 自行渲染二维码（对应回调风格 `startQrConnect` + `displayQrCodeToConsole: false`）

关闭终端打印，通过 `on_qr_url` 回调拿到扫码链接，自行生成图片或发送给用户：

```rust
use qqbot_connector::{connect, ConnectOptions};

let mut options = ConnectOptions::new();
options.display_qr_code_to_console = false;
options.source = "my-platform".into();
options.on_qr_url = Some(Box::new(|url| {
    // 始终在轮询开始前回调，可自行渲染二维码图片
    println!("扫码链接: {url}");
}));
options.on_status = Some(Box::new(|status, msg| {
    println!("[{status:?}] {msg}");
}));

let creds = connect(&options)?;
```

二维码过期后会自动刷新（再次触发 `on_qr_url`），直到扫码成功或取消。每个任务的首次轮询状态都会触发 `on_status`，包括刷新后的“等待扫码”；同一任务内的重复状态不会重复通知。

### 凭据本地缓存（对应 qqbot-go 的 `FileStore` / `LoadOrConnect`）

```rust
use qqbot_connector::{load_or_connect, ConnectOptions, FileStore};

let store = FileStore::new("qqbot-credentials.json");
let creds = load_or_connect(&store, &ConnectOptions::new())?;
```

文件格式与 qqbot-go 兼容：`{"app_id": "...", "app_secret": "..."}`（Unix 下权限 0600）。

### 取消

`connect` 是阻塞调用，通过 `CancelToken` 协作式取消（从其他线程或 Ctrl-C 处理器触发）：

```rust
use qqbot_connector::{connect, CancelToken, ConnectOptions};

let cancel = CancelToken::new();
let mut options = ConnectOptions::new();
options.cancel = cancel.clone();

std::thread::spawn(move || {
    std::thread::sleep(std::time::Duration::from_secs(60));
    cancel.cancel(); // 60 秒后 connect 返回 Err(Error::Cancelled)
});

let creds = connect(&options);
```

异步运行时中可在 `tokio::task::spawn_blocking` 等阻塞线程池里调用。

## API

底层接口与 npm 包 `qqbot-session.js` 逐函数对应：

| Rust | npm 对应 | 说明 |
|---|---|---|
| `generate_bind_key()` | `generateBindKey()` | 生成 32 字节随机密钥的 base64 |
| `create_bind_task(env)` | `createBindTask(env)` | 创建绑定任务，返回 `BindTask { task_id, key }` |
| `poll_bind_result(task_id, env)` | `pollBindResult(taskId, env)` | 查询一次扫码状态，返回 `PollResult` |
| `build_connect_url(task_id, source)` | `buildConnectUrl(taskId, source)` | 构造扫码链接（始终使用生产域名） |
| `decrypt_secret(key_b64, encrypted_b64)` | `decryptSecret(encrypted, key)` | AES-256-GCM 解出 AppSecret |
| `Env::Production / Env::Test` | `QQBotEnv` | API 环境（`q.qq.com` / `test.q.qq.com`） |
| `BindStatus::{None, Pending, Completed, Expired}` | `BindStatus` | 扫码状态（0/1/2/3） |

高层接口：

- `connect(&ConnectOptions) -> Result<Credentials>` —— 完整扫码绑定流程
- `load_or_connect(&FileStore, &ConnectOptions) -> Result<Credentials>` —— 带本地缓存的完整流程
- `render_qr_terminal(url)`（`qr` feature）—— 二维码渲染为终端字符串

## Features

| Feature | 默认 | 说明 |
|---|---|---|
| `qr` | ✅ | 终端二维码渲染（`qrcode` crate）。关闭后 `display_qr_code_to_console` 退化为打印链接 |

## 依赖

运行时依赖仅 5 个（不含 `qr` feature 时 4 个），已尽量精简：

- [`ureq`](https://crates.io/crates/ureq)（rustls）— 阻塞 HTTPS 客户端
- [`serde`](https://crates.io/crates/serde) + [`serde_json`](https://crates.io/crates/serde_json) — 协议编解码
- [`aes-gcm`](https://crates.io/crates/aes-gcm) — AppSecret 解密
- [`getrandom`](https://crates.io/crates/getrandom) — 绑定密钥
- [`qrcode`](https://crates.io/crates/qrcode)（可选，`qr` feature）— 终端二维码

base64 与 URL 百分号编码为内置实现（各自 RFC 4648 向量覆盖测试），未引入额外依赖。

## 协议

1. `POST https://q.qq.com/lite/create_bind_task`，body `{"key": <32 字节随机数的 base64>}` → `data.task_id`；
2. 扫码链接 `https://q.qq.com/qqbot/openclaw/connect.html?task_id=...&source=...&_wv=2` 渲染成二维码，等待手机 QQ 扫码；
3. 每 2 秒 `POST /lite/poll_bind_result`，body `{"task_id": ...}`，返回 `status`（0 等待 / 1 已扫码 / 2 完成 / 3 过期）、`bot_appid`、`bot_encrypt_secret`、`user_openid`；
4. 完成后用第 1 步的密钥做 AES-256-GCM 解密 `base64(IV[12] + ciphertext + tag[16])` 得到 AppSecret；
5. 过期自动重新创建任务刷新二维码；轮询期间瞬时网络错误静默重试。

请求超时 10 秒，二维码 URL 固定使用生产域名（`test` 环境仅切换 API 域名）。

## 测试

```bash
cargo test        # 单元测试与文档测试（含 RFC 4648 base64 向量、NIST GCM 向量）
cargo test --no-default-features   # 关闭二维码渲染的测试
cargo test --test live -- --ignored --nocapture   # 真实端点冒烟（会创建真实绑定任务）
cargo run --example connect   # 交互式体验
```
