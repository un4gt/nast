//! 交互式示例：终端展示二维码，扫码后打印凭据。
//!
//! ```bash
//! cargo run --example connect [-- 平台标识]
//! ```
//!
//! Ctrl-C 直接结束进程即可（二维码过期会自动刷新，无需干预）。
//! 如需优雅取消，参考 `lib.rs` 文档中的 `CancelToken` 用法。

use qqbot_connector::{ConnectOptions, connect};

fn main() -> qqbot_connector::Result<()> {
    let source = std::env::args().nth(1).unwrap_or_default();

    let mut options = ConnectOptions::new();
    options.source = source;
    options.on_status = Some(Box::new(|_status, msg| {
        if msg != "等待扫码..." {
            println!("{msg}");
        }
    }));

    println!("正在生成二维码…");
    let creds = connect(&options)?;

    println!("绑定成功！");
    println!("AppID:     {}", creds.app_id);
    println!("AppSecret: {}", creds.app_secret);
    if let Some(openid) = &creds.user_openid {
        println!("扫码用户 openid: {openid}");
    }
    Ok(())
}
