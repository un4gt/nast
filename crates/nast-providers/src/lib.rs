//! nast-providers：模型接入层。
//!
//! 统一 StreamEvent 归一化三协议：
//! - OpenAI 兼容（自定义 baseURL，覆盖 OpenAI/OpenRouter/DeepSeek/本地 vLLM 等）
//! - Anthropic 原生 /v1/messages
//! - Google Gemini :streamGenerateContent?alt=sse
//!
//! 与 ST 的差异：ST 服务端原样透传上游 SSE、由客户端按来源分别解析；
//! nast 在 provider 层统一为 StreamEvent，生成状态机无需关心协议差异。

use futures_util::{Stream, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("http {status}: {body}")]
    Http { status: u16, body: String },
    #[error("network: {0}")]
    Network(String),
    #[error("aborted")]
    Aborted,
    #[error("config: {0}")]
    Config(String),
}

/// 统一流事件。
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// 增量文本
    Token(String),
    /// 推理增量
    Reasoning(String),
    /// 归一化的 usage（若上游提供）
    Usage { input: Option<i64>, output: Option<i64> },
    /// 上游报错（流中）
    Error(String),
    /// 流结束
    Done,
}

/// 生成请求参数（协议无关）。
#[derive(Debug, Clone)]
pub struct GenRequest {
    pub messages: Vec<ChatMessage>,
    pub model: String,
    pub temperature: f64,
    pub top_p: f64,
    pub frequency_penalty: f64,
    pub presence_penalty: f64,
    pub max_tokens: i64,
    pub stop: Vec<String>,
    pub stream: bool,
    /// Anthropic assistant prefill
    pub assistant_prefill: Option<String>,
    /// Anthropic use_sysprompt（system 提取开关）
    pub use_sysprompt: bool,
    /// 自定义源扩展头/体
    pub extra_headers: Vec<(String, String)>,
    pub extra_body: Value,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String, // system/user/assistant
    pub content: String,
    pub name: Option<String>,
}

/// 模型源配置。
#[derive(Debug, Clone)]
pub enum ProviderKind {
    /// OpenAI 兼容：base_url + api_key（支持 /v1/chat/completions 的任意服务）
    OpenAiCompat { base_url: String, api_key: String },
    Anthropic { api_key: String },
    Gemini { api_key: String },
}

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

pub struct Provider {
    kind: ProviderKind,
    client: reqwest::Client,
}

impl Provider {
    pub fn new(kind: ProviderKind) -> Self {
        let client = reqwest::Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .build()
            .expect("build reqwest client");
        Self { kind, client }
    }

    /// 非流式生成。
    pub async fn generate(&self, req: &GenRequest) -> Result<String, ProviderError> {
        let stream = match self.kind {
            ProviderKind::OpenAiCompat { .. } => {
                let (url, headers, body) = self.openai_request(req, false)?;
                self.send_stream(url, headers, body).await?
            }
            ProviderKind::Anthropic { .. } => {
                let (url, headers, body) = self.anthropic_request(req, false)?;
                self.send_stream(url, headers, body).await?
            }
            ProviderKind::Gemini { .. } => {
                let (url, headers, body) = self.gemini_request(req, false)?;
                self.send_stream(url, headers, body).await?
            }
        };
        tokio::pin!(stream);
        let mut text = String::new();
        while let Some(ev) = stream.next().await {
            match ev {
                Ok(StreamEvent::Token(t)) => text.push_str(&t),
                Ok(StreamEvent::Error(e)) => {
                    return Err(ProviderError::Http { status: 500, body: e })
                }
                Ok(StreamEvent::Done) => break,
                Err(ProviderError::Aborted) => return Err(ProviderError::Aborted),
                Err(e) => return Err(e),
                _ => {}
            }
        }
        Ok(text)
    }

    /// 流式生成：返回事件流。
    pub async fn generate_stream(
        &self,
        req: &GenRequest,
    ) -> Result<impl Stream<Item = Result<StreamEvent, ProviderError>>, ProviderError> {
        match &self.kind {
            ProviderKind::OpenAiCompat { .. } => {
                let (url, headers, body) = self.openai_request(req, req.stream)?;
                self.send_stream(url, headers, body).await
            }
            ProviderKind::Anthropic { .. } => {
                let (url, headers, body) = self.anthropic_request(req, req.stream)?;
                self.send_stream(url, headers, body).await
            }
            ProviderKind::Gemini { .. } => {
                let (url, headers, body) = self.gemini_request(req, req.stream)?;
                self.send_stream(url, headers, body).await
            }
        }
    }

    // ---------- 协议请求构造 ----------

    fn openai_request(
        &self,
        req: &GenRequest,
        stream: bool,
    ) -> Result<(String, Vec<(String, String)>, Value), ProviderError> {
        let ProviderKind::OpenAiCompat { base_url, api_key } = &self.kind else {
            return Err(ProviderError::Config("not openai-compat".into()));
        };
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let mut messages: Vec<Value> = req
            .messages
            .iter()
            .map(|m| {
                let mut o = json!({"role": m.role, "content": m.content});
                if let Some(n) = &m.name {
                    o["name"] = json!(n);
                }
                o
            })
            .collect();
        if let Some(prefill) = &req.assistant_prefill {
            if !prefill.trim().is_empty() {
                messages.push(json!({"role": "assistant", "content": prefill}));
            }
        }
        let mut body = json!({
            "messages": messages,
            "model": req.model,
            "temperature": req.temperature,
            "top_p": req.top_p,
            "frequency_penalty": req.frequency_penalty,
            "presence_penalty": req.presence_penalty,
            "max_tokens": req.max_tokens,
            "stream": stream,
        });
        if !req.stop.is_empty() {
            body["stop"] = json!(req.stop);
        }
        if let Some(extra) = req.extra_body.as_object() {
            for (k, v) in extra {
                body[k] = v.clone();
            }
        }
        let mut headers = vec![("Authorization".to_string(), format!("Bearer {api_key}"))];
        headers.extend(req.extra_headers.clone());
        Ok((url, headers, body))
    }

    fn anthropic_request(
        &self,
        req: &GenRequest,
        stream: bool,
    ) -> Result<(String, Vec<(String, String)>, Value), ProviderError> {
        let ProviderKind::Anthropic { api_key } = &self.kind else {
            return Err(ProviderError::Config("not anthropic".into()));
        };
        let url = "https://api.anthropic.com/v1/messages".to_string();
        // system = 前导 system 消息串（prompt-converters 语义：仅前导 run 提取）
        let mut system_parts: Vec<String> = Vec::new();
        let mut msgs: Vec<Value> = Vec::new();
        let mut first_non_system = true;
        for m in &req.messages {
            if m.role == "system" && first_non_system && req.use_sysprompt {
                system_parts.push(m.content.clone());
                continue;
            }
            first_non_system = false;
            let role = if m.role == "assistant" { "assistant" } else { "user" };
            msgs.push(json!({"role": role, "content": [{"type": "text", "text": m.content}]}));
        }
        // 合并连续同角色（merge 语义）
        let mut merged: Vec<Value> = Vec::new();
        for m in msgs {
            if let Some(last) = merged.last_mut() {
                if last["role"] == m["role"] {
                    let mut texts: Vec<String> = Vec::new();
                    for block in last["content"].as_array().unwrap() {
                        texts.push(block["text"].as_str().unwrap_or_default().to_string());
                    }
                    texts.push(m["content"][0]["text"].as_str().unwrap_or_default().to_string());
                    last["content"] =
                        json!([{"type": "text", "text": texts.join("\n\n")}]);
                    continue;
                }
            }
            merged.push(m);
        }
        // assistant prefill：末尾 assistant 消息（trimEnd）
        if let Some(prefill) = &req.assistant_prefill {
            if !prefill.trim().is_empty() {
                merged.push(json!({
                    "role": "assistant",
                    "content": [{"type": "text", "text": prefill.trim_end()}]
                }));
            }
        }
        // 空历史占位（prompt-converters：PROMPT_PLACEHOLDER）
        if merged.is_empty() {
            merged.push(json!({
                "role": "user",
                "content": [{"type": "text", "text": "Let's get started."}]
            }));
        }
        let mut body = json!({
            "model": req.model,
            "max_tokens": req.max_tokens,
            "messages": merged,
            "stream": stream,
        });
        if !system_parts.is_empty() {
            body["system"] = json!([{"type": "text", "text": system_parts.join("\n")}]);
        }
        if !req.stop.is_empty() {
            body["stop_sequences"] = json!(req.stop);
        }
        // Claude 不支持 frequency/presence penalty；temperature/top_p 直接传
        body["temperature"] = json!(req.temperature);
        if (req.top_p - 1.0).abs() > f64::EPSILON {
            body["top_p"] = json!(req.top_p);
        }
        let headers = vec![
            ("x-api-key".to_string(), api_key.clone()),
            ("anthropic-version".to_string(), "2023-06-01".to_string()),
        ];
        Ok((url, headers, body))
    }

    fn gemini_request(
        &self,
        req: &GenRequest,
        stream: bool,
    ) -> Result<(String, Vec<(String, String)>, Value), ProviderError> {
        let ProviderKind::Gemini { api_key } = &self.kind else {
            return Err(ProviderError::Config("not gemini".into()));
        };
        // 前导 system → systemInstruction；其余 user/model
        let mut system_parts: Vec<String> = Vec::new();
        let mut contents: Vec<Value> = Vec::new();
        let mut first_non_system = true;
        for m in &req.messages {
            if m.role == "system" && first_non_system {
                system_parts.push(m.content.clone());
                continue;
            }
            first_non_system = false;
            let role = if m.role == "assistant" { "model" } else { "user" };
            contents.push(json!({"role": role, "parts": [{"text": m.content}]}));
        }
        // 连续同角色合并
        let mut merged: Vec<Value> = Vec::new();
        for c in contents {
            if let Some(last) = merged.last_mut() {
                if last["role"] == c["role"] {
                    let mut texts: Vec<String> = Vec::new();
                    for part in last["parts"].as_array().unwrap() {
                        texts.push(part["text"].as_str().unwrap_or_default().to_string());
                    }
                    texts.push(c["parts"][0]["text"].as_str().unwrap_or_default().to_string());
                    last["parts"] = json!([{"text": texts.join("\n\n")}]);
                    continue;
                }
            }
            merged.push(c);
        }
        if merged.is_empty() {
            merged.push(json!({"role": "user", "parts": [{"text": "Let's get started."}]}));
        }
        let method =
            if stream { "streamGenerateContent?alt=sse" } else { "generateContent" };
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:{}?key={}",
            req.model, method, api_key
        );
        let mut body = json!({
            "contents": merged,
            "generationConfig": {
                "temperature": req.temperature,
                "topP": req.top_p,
                "maxOutputTokens": req.max_tokens,
            },
        });
        if !system_parts.is_empty() {
            body["systemInstruction"] = json!({"parts": [{"text": system_parts.join("\n")}]});
        }
        if !req.stop.is_empty() {
            // Gemini stopSequences 限 5 条
            let stops: Vec<String> = req.stop.iter().take(5).cloned().collect();
            body["generationConfig"]["stopSequences"] = json!(stops);
        }
        Ok((url, vec![], body))
    }

    // ---------- SSE 发送与解析 ----------

    async fn send_stream(
        &self,
        url: String,
        headers: Vec<(String, String)>,
        body: Value,
    ) -> Result<impl Stream<Item = Result<StreamEvent, ProviderError>>, ProviderError> {
        let mut request = self.client.post(&url).json(&body);
        for (k, v) in headers {
            request = request.header(&k, &v);
        }
        let response = request
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::Http { status: status.as_u16(), body });
        }
        let byte_stream = response.bytes_stream();
        Ok(sse_events(byte_stream))
    }
}

/// 字节流 → SSE data 行 → 归一化 StreamEvent。
fn sse_events(
    stream: impl Stream<Item = Result<bytes::Bytes, reqwest::Error>>,
) -> impl Stream<Item = Result<StreamEvent, ProviderError>> {
    futures_util::stream::unfold(
        (Box::pin(stream), String::new(), false),
        |(mut stream, mut buf, done)| async move {
            if done {
                return None;
            }
            loop {
                // 从 buffer 提取完整 SSE 事件（空行分隔）
                while let Some((event, sep_len)) = split_event(&buf) {
                    let event = event.to_string();
                    buf.drain(..event.len() + sep_len);
                    // 提取 data: 行（可多行拼接）
                    let mut data = String::new();
                    for line in event.lines() {
                        if let Some(d) = line.strip_prefix("data:") {
                            if !data.is_empty() {
                                data.push('\n');
                            }
                            data.push_str(d.trim_start_matches(' '));
                        }
                    }
                    if data.trim() == "[DONE]" {
                        return Some((Ok(StreamEvent::Done), (stream, buf, true)));
                    }
                    if data.trim().is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<Value>(data.trim()) {
                        Ok(v) => {
                            let events = normalize(v);
                            if let Some(first) = events.first() {
                                let is_done = matches!(first, StreamEvent::Done);
                                let ev = if is_done {
                                    first.clone()
                                } else {
                                    first.clone()
                                };
                                // 多事件时丢弃后续（一个 chunk 多事件极少见；
                                // Done 事件除外，其他情况每 chunk 单事件是常态）
                                let _ = events;
                                return Some((Ok(ev), (stream, buf, is_done)));
                            }
                        }
                        Err(_) => continue,
                    }
                }
                match stream.next().await {
                    Some(Ok(bytes)) => {
                        buf.push_str(&String::from_utf8_lossy(&bytes));
                    }
                    Some(Err(e)) => {
                        return Some((
                            Err(ProviderError::Network(e.to_string())),
                            (stream, buf, true),
                        ));
                    }
                    None => {
                        return Some((Ok(StreamEvent::Done), (stream, buf, true)));
                    }
                }
            }
        },
    )
}

/// 返回 (事件内容, 分隔符长度)。
fn split_event(buf: &str) -> Option<(&str, usize)> {
    for pat in ["\r\n\r\n", "\n\n", "\r\r"] {
        if let Some(p) = buf.find(pat) {
            return Some((&buf[..p], pat.len()));
        }
    }
    None
}

/// 上游 chunk → StreamEvent 列表（getStreamingReply 的服务端等价）。
fn normalize(v: Value) -> Vec<StreamEvent> {
    let mut out = Vec::new();
    // 错误传播
    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("upstream error");
        out.push(StreamEvent::Error(msg.to_string()));
        out.push(StreamEvent::Done);
        return out;
    }
    // OpenAI 兼容
    if let Some(choices) = v.get("choices").and_then(|c| c.as_array()) {
        for choice in choices {
            if let Some(delta) = choice.get("delta") {
                if let Some(text) = delta.get("content").and_then(|c| c.as_str()) {
                    if !text.is_empty() {
                        out.push(StreamEvent::Token(text.to_string()));
                    }
                }
                if let Some(r) = delta.get("reasoning_content").and_then(|c| c.as_str()) {
                    if !r.is_empty() {
                        out.push(StreamEvent::Reasoning(r.to_string()));
                    }
                }
                if let Some(r) = delta.get("reasoning").and_then(|c| c.as_str()) {
                    if !r.is_empty() {
                        out.push(StreamEvent::Reasoning(r.to_string()));
                    }
                }
            }
            // finish_reason 不检查（ST 行为），等待 [DONE] 或流关闭
        }
        if let Some(u) = v.get("usage") {
            out.push(StreamEvent::Usage {
                input: u.get("prompt_tokens").and_then(|t| t.as_i64()),
                output: u.get("completion_tokens").and_then(|t| t.as_i64()),
            });
        }
        return out;
    }
    // Anthropic
    if let Some(t) = v.get("type").and_then(|t| t.as_str()) {
        match t {
            "content_block_delta" => {
                if let Some(delta) = v.get("delta") {
                    match delta.get("type").and_then(|t| t.as_str()) {
                        Some("text_delta") => {
                            if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                                out.push(StreamEvent::Token(text.to_string()));
                            }
                        }
                        Some("thinking_delta") => {
                            if let Some(th) = delta.get("thinking").and_then(|t| t.as_str()) {
                                out.push(StreamEvent::Reasoning(th.to_string()));
                            }
                        }
                        _ => {}
                    }
                }
            }
            "message_delta" => {
                if let Some(u) = v.get("usage") {
                    out.push(StreamEvent::Usage {
                        input: None,
                        output: u.get("output_tokens").and_then(|t| t.as_i64()),
                    });
                }
            }
            "message_stop" => {
                out.push(StreamEvent::Done);
            }
            "error" => {
                let msg = v
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("anthropic error");
                out.push(StreamEvent::Error(msg.to_string()));
                out.push(StreamEvent::Done);
            }
            _ => {}
        }
        return out;
    }
    // Gemini（alt=sse）
    if let Some(candidates) = v.get("candidates").and_then(|c| c.as_array()) {
        if let Some(c0) = candidates.first() {
            if let Some(parts) = c0
                .get("content")
                .and_then(|c| c.get("parts"))
                .and_then(|p| p.as_array())
            {
                for part in parts {
                    if part.get("thought").and_then(|t| t.as_bool()) == Some(true) {
                        continue;
                    }
                    if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                        if !text.is_empty() {
                            out.push(StreamEvent::Token(text.to_string()));
                        }
                    }
                }
            }
            if c0.get("finishReason").is_some() {
                out.push(StreamEvent::Done);
            }
        }
        if let Some(u) = v.get("usageMetadata") {
            out.push(StreamEvent::Usage {
                input: u.get("promptTokenCount").and_then(|t| t.as_i64()),
                output: u.get("candidatesTokenCount").and_then(|t| t.as_i64()),
            });
        }
        return out;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_openai_delta() {
        let v = json!({"choices": [{"delta": {"content": "Hi"}}]});
        let evs = normalize(v);
        assert_eq!(evs.len(), 1);
        assert!(matches!(&evs[0], StreamEvent::Token(t) if t == "Hi"));
    }

    #[test]
    fn normalize_openai_reasoning() {
        let v = json!({"choices": [{"delta": {"reasoning_content": "think"}}]});
        let evs = normalize(v);
        assert!(matches!(&evs[0], StreamEvent::Reasoning(_)));
    }

    #[test]
    fn normalize_anthropic_text_delta() {
        let v = json!({"type": "content_block_delta", "delta": {"type": "text_delta", "text": "hey"}});
        let evs = normalize(v);
        assert!(matches!(&evs[0], StreamEvent::Token(t) if t == "hey"));
    }

    #[test]
    fn normalize_anthropic_thinking() {
        let v = json!({"type": "content_block_delta", "delta": {"type": "thinking_delta", "thinking": "hmm"}});
        let evs = normalize(v);
        assert!(matches!(&evs[0], StreamEvent::Reasoning(_)));
    }

    #[test]
    fn normalize_anthropic_stop() {
        let v = json!({"type": "message_stop"});
        assert!(matches!(normalize(v).last(), Some(StreamEvent::Done)));
    }

    #[test]
    fn normalize_gemini_parts() {
        let v = json!({"candidates": [{"content": {"parts": [{"text": "yo"}]}}]});
        let evs = normalize(v);
        assert!(matches!(&evs[0], StreamEvent::Token(t) if t == "yo"));
    }

    #[test]
    fn normalize_gemini_thought_skipped() {
        let v = json!({"candidates": [{"content": {"parts": [{"text": "t", "thought": true}]}}]});
        assert!(normalize(v).is_empty());
    }

    #[test]
    fn normalize_error() {
        let v = json!({"error": {"message": "quota"}});
        let evs = normalize(v);
        assert!(matches!(&evs[0], StreamEvent::Error(m) if m == "quota"));
        assert!(matches!(evs.last(), Some(StreamEvent::Done)));
    }

    #[test]
    fn anthropic_request_shape() {
        let p = Provider::new(ProviderKind::Anthropic { api_key: "k".into() });
        let req = GenRequest {
            messages: vec![
                ChatMessage { role: "system".into(), content: "sys prompt".into(), name: None },
                ChatMessage { role: "user".into(), content: "hello".into(), name: None },
                ChatMessage { role: "assistant".into(), content: "hi".into(), name: None },
                ChatMessage { role: "system".into(), content: "mid system".into(), name: None },
            ],
            model: "claude-3-5-sonnet-20241022".into(),
            temperature: 1.0,
            top_p: 1.0,
            frequency_penalty: 0.0,
            presence_penalty: 0.0,
            max_tokens: 300,
            stop: vec![],
            stream: false,
            assistant_prefill: Some("prefill text".into()),
            use_sysprompt: true,
            extra_headers: vec![],
            extra_body: json!({}),
        };
        let (_, _, body) = p.anthropic_request(&req, false).unwrap();
        // system 仅前导提取
        assert_eq!(body["system"][0]["text"], "sys prompt");
        // 中间 system → user 消息
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[2]["role"], "user"); // mid system 转为 user
        // prefill 追加末尾
        assert_eq!(msgs[3]["role"], "assistant");
        assert_eq!(msgs[3]["content"][0]["text"], "prefill text");
    }

    #[test]
    fn anthropic_merge_consecutive() {
        let p = Provider::new(ProviderKind::Anthropic { api_key: "k".into() });
        let req = GenRequest {
            messages: vec![
                ChatMessage { role: "user".into(), content: "a".into(), name: None },
                ChatMessage { role: "user".into(), content: "b".into(), name: None },
            ],
            model: "claude-3-5-sonnet-20241022".into(),
            temperature: 1.0,
            top_p: 1.0,
            frequency_penalty: 0.0,
            presence_penalty: 0.0,
            max_tokens: 100,
            stop: vec![],
            stream: false,
            assistant_prefill: None,
            use_sysprompt: false,
            extra_headers: vec![],
            extra_body: json!({}),
        };
        let (_, _, body) = p.anthropic_request(&req, false).unwrap();
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["content"][0]["text"], "a\n\nb");
    }

    #[test]
    fn gemini_request_shape() {
        let p = Provider::new(ProviderKind::Gemini { api_key: "g".into() });
        let req = GenRequest {
            messages: vec![
                ChatMessage { role: "system".into(), content: "be nice".into(), name: None },
                ChatMessage { role: "user".into(), content: "hi".into(), name: None },
                ChatMessage { role: "assistant".into(), content: "hey".into(), name: None },
            ],
            model: "gemini-1.5-pro".into(),
            temperature: 1.0,
            top_p: 1.0,
            frequency_penalty: 0.0,
            presence_penalty: 0.0,
            max_tokens: 200,
            stop: vec!["END".into()],
            stream: true,
            assistant_prefill: None,
            use_sysprompt: false,
            extra_headers: vec![],
            extra_body: json!({}),
        };
        let (url, _, body) = p.gemini_request(&req, true).unwrap();
        assert!(url.contains("streamGenerateContent?alt=sse"));
        assert!(url.contains("key=g"));
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], "be nice");
        assert_eq!(body["contents"][0]["role"], "user");
        assert_eq!(body["contents"][1]["role"], "model");
        assert_eq!(body["generationConfig"]["stopSequences"][0], "END");
    }

    #[test]
    fn openai_request_shape() {
        let p = Provider::new(ProviderKind::OpenAiCompat {
            base_url: "https://api.example.com/v1/".into(),
            api_key: "sk".into(),
        });
        let req = GenRequest {
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "q".into(),
                name: Some("User".into()),
            }],
            model: "gpt-4o".into(),
            temperature: 0.7,
            top_p: 1.0,
            frequency_penalty: 0.0,
            presence_penalty: 0.0,
            max_tokens: 128,
            stop: vec!["STOP".into()],
            stream: true,
            assistant_prefill: None,
            use_sysprompt: false,
            extra_headers: vec![("X-Custom".into(), "1".into())],
            extra_body: json!({}),
        };
        let (url, headers, body) = p.openai_request(&req, true).unwrap();
        assert_eq!(url, "https://api.example.com/v1/chat/completions");
        assert_eq!(headers[0].0, "Authorization");
        assert_eq!(headers[0].1, "Bearer sk");
        assert_eq!(headers[1].0, "X-Custom");
        assert_eq!(body["max_tokens"], 128);
        assert_eq!(body["stop"][0], "STOP");
        assert_eq!(body["messages"][0]["name"], "User");
    }
}
