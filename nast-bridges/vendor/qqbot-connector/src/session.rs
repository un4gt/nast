//! `q.qq.com` 扫码绑定底层接口（对应参考实现中的 qqbot-session.js / connector.go）。
//!
//! 流程：[`generate_bind_key`] → [`create_bind_task`] → 用户扫码
//! （[`build_connect_url`] 生成的链接渲染成二维码）→ 轮询 [`poll_bind_result`]
//! → [`crate::decrypt_secret`] 解出 AppSecret。

use serde::Deserialize;

use crate::base64;
use crate::error::{Error, Result};
use crate::http::post_json;

/// 单次 HTTP 请求超时（与参考实现一致的 10 秒）。
pub const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// 轮询间隔（与参考实现一致的 2 秒）。
pub const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// 生产环境 API 域名。
pub const PRODUCTION_HOST: &str = "q.qq.com";
/// 测试环境 API 域名（二维码 URL 始终使用生产域名）。
pub const TEST_HOST: &str = "test.q.qq.com";

/// API 环境，对应 npm 包的 `QQBotEnv`。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Env {
    /// `q.qq.com`（默认）。
    #[default]
    Production,
    /// `test.q.qq.com`。
    Test,
}

impl Env {
    /// 该环境对应的 API 域名。
    pub fn host(self) -> &'static str {
        match self {
            Env::Production => PRODUCTION_HOST,
            Env::Test => TEST_HOST,
        }
    }
}

/// 生成扫码绑定用的随机密钥：32 字节随机数的 base64 编码。
pub fn generate_bind_key() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| Error::Crypto(format!("generate key: {e}")))?;
    Ok(base64::encode(&bytes))
}

/// [`create_bind_task`] 的返回值。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindTask {
    /// 服务端分配的任务 ID。
    pub task_id: String,
    /// 本次任务对应的 base64 密钥，解密 `bot_encrypt_secret` 时需要。
    pub key: String,
}

/// [`poll_bind_result`] 返回的扫码状态，对应 npm 包的 `BindStatus`。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BindStatus {
    /// 等待扫码。
    #[default]
    None,
    /// 已扫码，等待用户在手机上确认。
    Pending,
    /// 绑定完成，可解密凭据。
    Completed,
    /// 二维码已过期，需要重新生成任务。
    Expired,
}

impl BindStatus {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(BindStatus::None),
            1 => Some(BindStatus::Pending),
            2 => Some(BindStatus::Completed),
            3 => Some(BindStatus::Expired),
            _ => None,
        }
    }
}

/// [`poll_bind_result`] 的返回值。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PollResult {
    /// 当前扫码状态。
    pub status: BindStatus,
    /// 绑定成功后的机器人 AppID。
    pub bot_app_id: String,
    /// 加密的 AppSecret，用任务密钥通过 [`crate::decrypt_secret`] 解密。
    pub bot_encrypt_secret: String,
    /// 扫码用户的 openid（若有）。
    pub user_openid: Option<String>,
}

#[derive(Deserialize)]
struct ApiResponse {
    #[serde(default)]
    retcode: i64,
    #[serde(default)]
    msg: String,
    data: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct CreateTaskData {
    #[serde(default)]
    task_id: String,
}

#[derive(Deserialize)]
struct PollData {
    #[serde(default)]
    status: u8,
    #[serde(default, deserialize_with = "deserialize_app_id")]
    bot_appid: String,
    #[serde(default)]
    bot_encrypt_secret: String,
    #[serde(default)]
    user_openid: String,
}

fn deserialize_app_id<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum AppId {
        String(String),
        Number(serde_json::Number),
    }

    // 字符串、数字及空值与 npm 的 String(bot_appid ?? "") 一致；整数不经过浮点数转换。
    Ok(match Option::<AppId>::deserialize(deserializer)? {
        Some(AppId::String(value)) => value,
        Some(AppId::Number(value)) => value.to_string(),
        None => String::new(),
    })
}

/// 调用 `/lite/create_bind_task` 创建绑定任务。
pub fn create_bind_task(env: Env) -> Result<BindTask> {
    let key = generate_bind_key()?;
    let url = format!("https://{}/lite/create_bind_task", env.host());
    let body = serde_json::json!({ "key": key });

    let resp: ApiResponse = post_json(&url, &body)?;
    if resp.retcode != 0 {
        return Err(Error::Api {
            retcode: resp.retcode,
            message: if resp.msg.is_empty() {
                "create_bind_task failed".into()
            } else {
                resp.msg
            },
        });
    }
    let task_id = match resp.data {
        Some(data) => serde_json::from_value::<CreateTaskData>(data)?.task_id,
        None => String::new(),
    };
    if task_id.is_empty() {
        return Err(Error::InvalidResponse(
            "create_bind_task: missing task_id".into(),
        ));
    }
    Ok(BindTask { task_id, key })
}

/// 调用 `/lite/poll_bind_result` 查询一次扫码状态。
pub fn poll_bind_result(task_id: &str, env: Env) -> Result<PollResult> {
    let url = format!("https://{}/lite/poll_bind_result", env.host());
    let body = serde_json::json!({ "task_id": task_id });

    let resp: ApiResponse = post_json(&url, &body)?;
    if resp.retcode != 0 {
        return Err(Error::Api {
            retcode: resp.retcode,
            message: if resp.msg.is_empty() {
                "poll_bind_result failed".into()
            } else {
                resp.msg
            },
        });
    }

    let data = match resp.data {
        Some(data) => serde_json::from_value::<PollData>(data)?,
        None => PollData {
            status: 0,
            bot_appid: String::new(),
            bot_encrypt_secret: String::new(),
            user_openid: String::new(),
        },
    };

    let status = BindStatus::from_u8(data.status).unwrap_or_default();
    Ok(PollResult {
        status,
        bot_app_id: data.bot_appid,
        bot_encrypt_secret: data.bot_encrypt_secret,
        user_openid: if data.user_openid.is_empty() {
            None
        } else {
            Some(data.user_openid)
        },
    })
}

/// 由 `task_id` 构造扫码链接（供业务自行渲染成二维码）。
///
/// 与参考实现一致：二维码 URL 始终使用生产域名 `q.qq.com`。
/// `source` 为接入平台标识，留空则扫码页面显示"第三方机器人"。
pub fn build_connect_url(task_id: &str, source: &str) -> String {
    format!(
        "https://{PRODUCTION_HOST}/qqbot/openclaw/connect.html?task_id={}&source={}&_wv=2",
        percent_encode(task_id),
        percent_encode(source),
    )
}

/// 与 JS `encodeURIComponent` 语义一致的查询参数编码。
fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for &b in input.as_bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(b as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts() {
        assert_eq!(Env::default().host(), "q.qq.com");
        assert_eq!(Env::Test.host(), "test.q.qq.com");
    }

    #[test]
    fn bind_key_is_32_bytes_base64() {
        let key = generate_bind_key().unwrap();
        let decoded = base64::decode(&key).unwrap();
        assert_eq!(decoded.len(), 32);
        // 两次生成应当不同。
        assert_ne!(key, generate_bind_key().unwrap());
    }

    #[test]
    fn connect_url_matches_reference() {
        assert_eq!(
            build_connect_url("TASK123", ""),
            "https://q.qq.com/qqbot/openclaw/connect.html?task_id=TASK123&source=&_wv=2"
        );
        assert_eq!(
            build_connect_url("a b&c", "我的平台"),
            "https://q.qq.com/qqbot/openclaw/connect.html?task_id=a%20b%26c&source=%E6%88%91%E7%9A%84%E5%B9%B3%E5%8F%B0&_wv=2"
        );
    }

    #[test]
    fn status_mapping() {
        assert_eq!(BindStatus::from_u8(0), Some(BindStatus::None));
        assert_eq!(BindStatus::from_u8(1), Some(BindStatus::Pending));
        assert_eq!(BindStatus::from_u8(2), Some(BindStatus::Completed));
        assert_eq!(BindStatus::from_u8(3), Some(BindStatus::Expired));
        assert_eq!(BindStatus::from_u8(4), None);
    }

    #[test]
    fn poll_data_accepts_string_and_numeric_app_ids() {
        for (app_id, expected) in [
            (serde_json::json!("1234567890"), "1234567890"),
            (serde_json::json!("001234567890"), "001234567890"),
            (serde_json::json!(1234567890_u64), "1234567890"),
            (serde_json::json!(u64::MAX), "18446744073709551615"),
        ] {
            let data: PollData = serde_json::from_value(serde_json::json!({
                "status": 2,
                "bot_appid": app_id,
                "bot_encrypt_secret": "encrypted-secret",
                "user_openid": "openid",
            }))
            .unwrap();
            assert_eq!(data.status, 2);
            assert_eq!(data.bot_appid, expected);
            assert_eq!(data.bot_encrypt_secret, "encrypted-secret");
            assert_eq!(data.user_openid, "openid");
        }
    }

    #[test]
    fn poll_data_defaults_missing_or_null_app_id() {
        for value in [
            serde_json::json!({}),
            serde_json::json!({"bot_appid": null}),
        ] {
            let data: PollData = serde_json::from_value(value).unwrap();
            assert!(data.bot_appid.is_empty());
            assert_eq!(data.status, 0);
        }
    }

    #[test]
    fn poll_data_rejects_invalid_app_id_types() {
        for app_id in [
            serde_json::json!(true),
            serde_json::json!([]),
            serde_json::json!({}),
        ] {
            assert!(
                serde_json::from_value::<PollData>(serde_json::json!({
                    "bot_appid": app_id,
                }))
                .is_err()
            );
        }
    }
}
