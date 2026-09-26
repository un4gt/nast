//! token 计数：OpenAI 系用 tiktoken（o200k/cl100k 按模型），其余源用近似模型。
//! 对齐 refrence/SillyTavern/src/endpoints/tokenizers.js getTokenizerModel 的映射。

use tiktoken_rs::CoreBPE;

/// chat_completion_source → tokenizer 模型。
/// 返回 (是真实 OpenAI 模型, tiktoken 模型名)。
pub fn tokenizer_model_for_source(source: &str, model: &str) -> String {
    match source {
        "openai" => {
            // 真实模型名直接用；gpt-3.5/gpt-4 家族按前缀选编码
            if model.is_empty() {
                "gpt-4o".to_string()
            } else {
                model.to_string()
            }
        }
        "claude" => "claude".to_string(),
        "deepseek" => "deepseek".to_string(),
        "mistralai" => "mistral".to_string(),
        "makersuite" => "gemma".to_string(),
        "cohere" => "command-r".to_string(),
        "groq" | "openrouter" | "custom" => {
            // 按模型名嗅探
            let m = model.to_ascii_lowercase();
            if m.contains("llama-3") || m.contains("llama3") {
                "llama3".to_string()
            } else if m.contains("llama") {
                "llama".to_string()
            } else if m.contains("mistral") {
                "mistral".to_string()
            } else if m.contains("qwen") {
                "qwen2".to_string()
            } else if m.contains("gemini") || m.contains("gemma") {
                "gemma".to_string()
            } else {
                "gpt-4o".to_string()
            }
        }
        _ => "gpt-4o".to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tokenizer {
    O200k,
    Cl100k,
    /// 近似：claude/deepseek 等统一按 cl100k 全量计数
    Approx,
}

pub fn resolve_tokenizer(model: &str) -> Tokenizer {
    let m = model.to_ascii_lowercase();
    if m.starts_with("gpt-4o") || m.contains("o200k") || m.contains("o1") || m.contains("o3") {
        Tokenizer::O200k
    } else if m.starts_with("gpt-") || m == "gpt4" {
        Tokenizer::Cl100k
    } else {
        Tokenizer::Approx
    }
}


/// 单次缓存的全局 BPE（避免每次调用重建编码器）。
use std::sync::OnceLock;

static O200K: OnceLock<Option<CoreBPE>> = OnceLock::new();
static CL100K: OnceLock<Option<CoreBPE>> = OnceLock::new();

pub fn count_tokens(text: &str, tokenizer: Tokenizer) -> usize {
    if text.is_empty() {
        return 0;
    }
    let cached = match tokenizer {
        Tokenizer::O200k => O200K.get_or_init(|| tiktoken_rs::o200k_base().ok()),
        Tokenizer::Cl100k | Tokenizer::Approx => {
            CL100K.get_or_init(|| tiktoken_rs::cl100k_base().ok())
        }
    };
    match cached {
        Some(b) => b.encode_ordinary(text).len(),
        None => text.len() / 4, // 兜底粗估
    }
}

/// Use the same source mapping during assembly and frozen-request validation.
pub fn tokenizer_for_source(source: &str, model: &str) -> Tokenizer {
    resolve_tokenizer(&tokenizer_model_for_source(source, model))
}

/// Local estimate including message framing and optional name. Native model
/// tokenizers are approximations; this is not provider-reported usage.
pub fn count_message_tokens(content: &str, name: Option<&str>, tokenizer: Tokenizer) -> usize {
    if content.is_empty() { return 0; }
    count_tokens(content, tokenizer) + 4
        + name.map_or(0, |name| count_tokens(name, tokenizer) + 1)
}
