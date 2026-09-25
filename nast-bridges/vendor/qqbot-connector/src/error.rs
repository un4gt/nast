//! 错误类型。

use std::fmt;

/// SDK 统一错误类型。
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// 网络传输错误（连接失败、超时等）。
    Network(String),
    /// 服务端返回了非 200 的 HTTP 状态码。
    HttpStatus(u16),
    /// 服务端返回 `retcode != 0`。
    Api { retcode: i64, message: String },
    /// 响应不符合协议约定的结构。
    InvalidResponse(String),
    /// JSON 编解码错误。
    Json(serde_json::Error),
    /// 加解密错误（base64 解码失败、密钥长度错误、GCM 校验失败等）。
    Crypto(String),
    /// 二维码渲染错误。
    Qr(String),
    /// 本地文件读写错误。
    Io(std::io::Error),
    /// 流程被取消（通过 [`crate::CancelToken`]）。
    Cancelled,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Network(msg) => write!(f, "network error: {msg}"),
            Error::HttpStatus(code) => write!(f, "HTTP {code} from server"),
            Error::Api { retcode, message } => {
                write!(f, "api error: retcode={retcode} msg={message}")
            }
            Error::InvalidResponse(msg) => write!(f, "invalid response: {msg}"),
            Error::Json(err) => write!(f, "json error: {err}"),
            Error::Crypto(msg) => write!(f, "crypto error: {msg}"),
            Error::Qr(msg) => write!(f, "qr render error: {msg}"),
            Error::Io(err) => write!(f, "io error: {err}"),
            Error::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Json(err) => Some(err),
            Error::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Json(err)
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err)
    }
}

impl From<ureq::Error> for Error {
    fn from(err: ureq::Error) -> Self {
        match err {
            ureq::Error::StatusCode(code) => Error::HttpStatus(code),
            other => Error::Network(other.to_string()),
        }
    }
}

/// SDK 统一 [`Result`](std::result::Result) 别名。
pub type Result<T> = std::result::Result<T, Error>;
