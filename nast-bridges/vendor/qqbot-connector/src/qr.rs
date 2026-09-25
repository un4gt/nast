//! 终端二维码渲染（`qr` feature）。
//!
//! 启用 `qr` feature（默认启用）时将扫码链接渲染为终端二维码打印；
//! 未启用时退化为直接打印链接本身。

/// 在终端展示扫码链接。
///
/// - 启用 `qr` feature：渲染二维码到 stdout（对应 npm 包 `qrcode-terminal` 的行为）。
/// - 未启用：直接打印链接，由调用方自行渲染。
#[cfg(feature = "qr")]
pub fn display_qr_code(url: &str) -> crate::error::Result<()> {
    print!("{}", render_qr_terminal(url)?);
    Ok(())
}

#[cfg(not(feature = "qr"))]
pub fn display_qr_code(url: &str) -> crate::error::Result<()> {
    println!("扫码链接: {url}");
    Ok(())
}

/// 将内容渲染为终端二维码字符串（半角两倍宽块字符，带 2 模块静区）。
#[cfg(feature = "qr")]
pub fn render_qr_terminal(data: &str) -> crate::error::Result<String> {
    use qrcode::QrCode;
    use qrcode::render::unicode;

    let code = QrCode::new(data.as_bytes()).map_err(|e| crate::error::Error::Qr(e.to_string()))?;
    // 默认自带 4 模块静区，符合 QR 规范；unicode 像素的 Image 类型是 String。
    Ok(code
        .render::<unicode::Dense1x2>()
        .dark_color(unicode::Dense1x2::Dark)
        .light_color(unicode::Dense1x2::Light)
        .build())
}

#[cfg(all(test, feature = "qr"))]
mod tests {
    use super::*;

    #[test]
    fn renders_qr_string() {
        let out = render_qr_terminal("https://q.qq.com/qqbot/openclaw/connect.html").unwrap();
        // 非空且由块字符/空格构成的多行输出。
        assert!(out.contains('█'));
        assert!(out.trim().lines().count() > 10);
    }
}
