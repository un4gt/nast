//! 模型连接：settings + secrets → provider / 请求参数。
//!
//! 优先级（对齐 ST 连接面板语义）：UI 保存的 settings/secrets → 环境变量 → 内置默认。
//! 现阶段 UI 仅暴露 OpenAI 兼容（custom）源；claude/makersuite 保留代码路径走 env。

use nast_model::preset::OaiSettings;
use nast_providers::{Provider, ProviderKind};
use serde_json::Value;

pub const SECRET_CUSTOM: &str = "api_key_custom";
pub const SECRET_CLAUDE: &str = "api_key_claude";
pub const SECRET_GOOGLE: &str = "api_key_makersuite";
pub const DEFAULT_OPENAI_BASE: &str = "https://api.openai.com/v1";

/// secrets blob 里某密钥当前激活条目的值。
pub fn active_secret(secrets: &Value, key: &str) -> Option<String> {
    let v = secrets
        .get(key)?
        .as_array()?
        .iter()
        .find(|e| e.get("active").and_then(|a| a.as_bool()).unwrap_or(false))
        .and_then(|e| e.get("value"))?
        .as_str()?;
    let v = v.trim();
    (!v.is_empty()).then(|| v.to_string())
}

fn secret_or_env(secrets: &Value, key: &str, env: &str) -> String {
    active_secret(secrets, key)
        .or_else(|| std::env::var(env).ok().filter(|v| !v.trim().is_empty()))
        .unwrap_or_default()
}

/// custom 源 baseURL：settings.custom_url（须含 /v1）→ NAST_OPENAI_BASE → OpenAI 官方。
pub fn custom_base_url(oai: &OaiSettings) -> String {
    let from_settings = oai.custom_url.trim();
    if !from_settings.is_empty() {
        return from_settings.trim_end_matches('/').to_string();
    }
    std::env::var("NAST_OPENAI_BASE")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().trim_end_matches('/').to_string())
        .unwrap_or_else(|| DEFAULT_OPENAI_BASE.to_string())
}

/// 按源选择 provider。
pub fn provider_kind(oai: &OaiSettings, secrets: &Value) -> ProviderKind {
    match oai.chat_completion_source.as_str() {
        "claude" => ProviderKind::Anthropic {
            api_key: secret_or_env(secrets, SECRET_CLAUDE, "ANTHROPIC_API_KEY"),
        },
        "makersuite" => ProviderKind::Gemini {
            api_key: secret_or_env(secrets, SECRET_GOOGLE, "GOOGLE_API_KEY"),
        },
        _ => ProviderKind::OpenAiCompat {
            base_url: custom_base_url(oai),
            api_key: secret_or_env(secrets, SECRET_CUSTOM, "OPENAI_API_KEY"),
        },
    }
}

pub fn provider(oai: &OaiSettings, secrets: &Value) -> Provider {
    Provider::new(provider_kind(oai, secrets))
}

/// 按源选择模型字段（ST openai.js：每源独立 model 字段；空则回落 openai_model）。
pub fn model_for(oai: &OaiSettings) -> String {
    let fallback = oai.openai_model.trim();
    let picked = match oai.chat_completion_source.as_str() {
        "claude" => oai.claude_model.trim(),
        "makersuite" => oai.google_model.trim(),
        _ => oai.custom_model.trim(),
    };
    if picked.is_empty() {
        fallback.to_string()
    } else {
        picked.to_string()
    }
}

/// custom_include_headers："Header-Name: value" 每行一对（ST cc-srv.js merge）。
pub fn extra_headers(oai: &OaiSettings) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in oai.custom_include_headers.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            let (k, v) = (k.trim(), v.trim());
            if !k.is_empty() && !v.is_empty() {
                out.push((k.to_string(), v.to_string()));
            }
        }
    }
    out
}

/// custom_include_body：JSON 对象整体覆盖合并进请求体。
pub fn extra_body(oai: &OaiSettings) -> Value {
    let raw = oai.custom_include_body.trim();
    if raw.is_empty() {
        return Value::Null;
    }
    match serde_json::from_str::<Value>(raw) {
        Ok(v) if v.is_object() => v,
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oai(custom_url: &str, custom_model: &str, source: &str) -> OaiSettings {
        OaiSettings {
            custom_url: custom_url.into(),
            custom_model: custom_model.into(),
            chat_completion_source: source.into(),
            openai_model: "gpt-4o".into(),
            custom_include_headers: "X-A: 1\n# comment\nX-B:2".into(),
            custom_include_body: r#"{"top_k": 5}"#.into(),
            ..Default::default()
        }
    }

    #[test]
    fn kind_and_model_selection() {
        let secrets = serde_json::json!({
            "api_key_custom": [{"id": "u", "value": "sk-x", "active": true}]
        });
        let ProviderKind::OpenAiCompat { base_url, api_key } =
            provider_kind(&oai("http://localhost:8001/v1/", "my-model", "custom"), &secrets)
        else {
            panic!("expected compat");
        };
        assert_eq!(base_url, "http://localhost:8001/v1");
        assert_eq!(api_key, "sk-x");
        assert_eq!(
            model_for(&oai("http://x/v1", "my-model", "custom")),
            "my-model"
        );
        // custom_model 空 → 回落 openai_model
        assert_eq!(model_for(&oai("http://x/v1", "", "custom")), "gpt-4o");
    }

    #[test]
    fn env_fallback_when_no_secret() {
        // 无 secrets：key 为空（env 未设置时），base 回落默认
        let ProviderKind::OpenAiCompat { base_url, api_key } =
            provider_kind(&oai("", "", "custom"), &serde_json::json!({}))
        else {
            panic!("expected compat");
        };
        assert_eq!(base_url, DEFAULT_OPENAI_BASE);
        assert!(api_key.is_empty() || std::env::var("OPENAI_API_KEY").is_ok());
    }

    #[test]
    fn header_and_body_parsing() {
        let o = oai("", "", "custom");
        assert_eq!(
            extra_headers(&o),
            vec![
                ("X-A".to_string(), "1".to_string()),
                ("X-B".to_string(), "2".to_string())
            ]
        );
        assert_eq!(extra_body(&o), serde_json::json!({"top_k": 5}));
    }
}
