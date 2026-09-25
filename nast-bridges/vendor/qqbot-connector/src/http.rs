//! 内部 HTTP 辅助：带全局超时的 JSON POST。

use std::sync::OnceLock;

use serde::de::DeserializeOwned;

use crate::error::{Error, Result};

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_global(Some(crate::session::REQUEST_TIMEOUT))
            .http_status_as_error(false)
            .build()
            .new_agent()
    })
}

/// POST 一个 JSON 请求体并解析 JSON 响应。
///
/// 与参考实现一致：非 200 状态码直接报错；业务错误通过响应中的
/// `retcode != 0` 表达，由调用方检查。
pub(super) fn post_json<T: DeserializeOwned>(url: &str, body: &serde_json::Value) -> Result<T> {
    let payload = serde_json::to_vec(body)?;

    let response = agent()
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .send(payload)
        .map_err(Error::from)?;

    let status = response.status().as_u16();
    if status != 200 {
        return Err(Error::HttpStatus(status));
    }

    let text = response.into_body().read_to_string()?;
    Ok(serde_json::from_str(&text)?)
}
