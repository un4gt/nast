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

#[derive(Debug, Clone, serde::Serialize, thiserror::Error)]
#[serde(tag="type", content="data", rename_all="snake_case")]
pub enum ProviderError {
    #[error("http {status}: {body}")]
    Http { status: u16, body: String, retry_after: Option<u64> },
    #[error("timeout: {0}")]
    Timeout(String),
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    #[error("stream disconnected before completion")]
    Disconnected,
    #[error("upstream {kind}: {message}")]
    Upstream { kind: String, message: String, status: Option<u16>, retry_after: Option<u64> },
    #[error("network: {0}")]
    Network(String),
    #[error("aborted")]
    Aborted,
    #[error("config: {0}")]
    Config(String),
    #[error("达到最大输出 Token 限制（{0}），回复未完成；请调高模型的最大输出或续写")]
    OutputLimit(String),
    #[error("模型 {model}：估算输入 {input_tokens} Token，超过最大输入 {limit}；请调整模型参数或减少提示词。备用模型不会重新裁剪本次请求")]
    InputLimit { model: String, input_tokens: i64, limit: i64 },
    #[error("模型 {model}：估算输入 {input_tokens} + 预留输出 {output_tokens} Token，超过上下文上限 {limit}；请调整模型参数。备用模型不会重新裁剪本次请求")]
    ContextLimit { model: String, input_tokens: i64, output_tokens: i64, limit: i64 },
}

impl ProviderError {
    fn redact(&mut self, api_key: &str) {
        match self {
            Self::Upstream { kind,message,.. }=> { *kind=redact_text(kind,api_key); *message=redact_text(message,api_key); },
            Self::Http { body,.. }=>*body=redact_text(body,api_key),
            Self::Network(s)|Self::Timeout(s)|Self::Config(s)|Self::InvalidResponse(s)=>*s=redact_text(s,api_key),
            _=>{},
        }
    }

    pub fn retryable(&self) -> bool {
        match self {
            Self::Http { status, .. } => matches!(status, 429 | 502 | 503 | 504),
            Self::Timeout(_) | Self::Disconnected => true,
            Self::Network(kind) => matches!(kind.as_str(), "connect" | "reset" | "temporary"),
            Self::Upstream { kind, status, .. } => matches!(status, Some(429 | 502 | 503 | 504)) || matches!(kind.as_str(), "overloaded_error" | "rate_limit_error" | "RESOURCE_EXHAUSTED" | "UNAVAILABLE" | "DEADLINE_EXCEEDED"),
            _ => false,
        }
    }
    pub fn retry_after(&self) -> Option<u64> { match self { Self::Http { retry_after, .. } | Self::Upstream { retry_after, .. } => *retry_after, _ => None } }
    fn network(error: reqwest::Error) -> Self {
        if error.is_timeout() { return Self::Timeout("upstream".into()); }
        if error.is_builder() { return Self::Config("invalid request configuration".into()); }
        // The error chain exposes IO error kinds; never classify by error prose.
        let mut source = std::error::Error::source(&error);
        while let Some(e) = source {
            if let Some(io) = e.downcast_ref::<std::io::Error>() {
                if matches!(io.kind(), std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotConnected | std::io::ErrorKind::AddrNotAvailable) { return Self::Network("connect".into()); }
                if matches!(io.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) { return Self::Network("temporary".into()); }
                if matches!(io.kind(), std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted | std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::UnexpectedEof) { return Self::Network("reset".into()); }
            }
            source = e.source();
        }
        Self::Network("unclassified transport error".into())
    }
}
fn upstream_error(value: &Value) -> ProviderError {
    let error = value.get("error").unwrap_or(value);
    ProviderError::Upstream { kind: error.get("type").or_else(|| error.get("status")).or_else(|| error.get("code")).and_then(Value::as_str).unwrap_or("unknown").to_string(), message: error["message"].as_str().unwrap_or("upstream error").to_string(), status: error["code"].as_u64().and_then(|n| u16::try_from(n).ok()), retry_after:None }
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
    Error(ProviderError),
    /// 上游结束原因；不代替协议的流结束标记。
    FinishReason(String),
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
    endpoint: Option<String>,
    remove_parameters: Vec<String>,
}

impl Provider {
    fn api_key(&self)->&str { match &self.kind { ProviderKind::OpenAiCompat { api_key,.. } | ProviderKind::Anthropic { api_key } | ProviderKind::Gemini { api_key }=>api_key } }

    pub fn new(kind: ProviderKind) -> Self {
        let client = reqwest::Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("build reqwest client");
        Self { kind, client, endpoint: None, remove_parameters: vec![] }
    }

    pub fn configured(kind: ProviderKind, endpoint: String, connect_secs: u64, remove_parameters: Vec<String>) -> Self {
        Self { kind, client: reqwest::Client::builder().connect_timeout(Duration::from_secs(connect_secs)).build().expect("HTTP client"), endpoint: Some(endpoint), remove_parameters }
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
                    return Err(e)
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

    /// 拉取模型列表（OpenAI 兼容 GET {base}/models；ST /status 路由同源）。
    /// 返回模型 id 列表（data[].id，缺失时降级取 id 字符串数组）。
    pub async fn list_models(&self) -> Result<Vec<String>, ProviderError> {
        let ProviderKind::OpenAiCompat { base_url, api_key } = &self.kind else {
            return Err(ProviderError::Config(
                "model list is only supported for the OpenAI-compatible source".into(),
            ));
        };
        let url = format!("{}/models", base_url.trim_end_matches('/'));
        let resp = self
            .client
            .get(&url)
            .bearer_auth(api_key)
            .send()
            .await
            .map_err(ProviderError::network)?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(ProviderError::network)?;
        if !status.is_success() {
            return Err(ProviderError::Http {
                status: status.as_u16(),
                body: diagnostic_body(&body,api_key), retry_after: None,
            });
        }
        let parsed: Value = serde_json::from_str(&body)
            .map_err(|e| ProviderError::InvalidResponse(format!("invalid json: {e}")))?;
        let mut out = Vec::new();
        if let Some(arr) = parsed.get("data").and_then(|d| d.as_array()) {
            for m in arr {
                if let Some(id) = m.get("id").and_then(|i| i.as_str()) {
                    out.push(id.to_string());
                }
            }
        } else if let Some(arr) = parsed.as_array() {
            // 个别兼容实现直接返回字符串数组
            for m in arr {
                if let Some(id) = m.as_str() {
                    out.push(id.to_string());
                }
            }
        }
        out.sort();
        out.dedup();
        Ok(out)
    }

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
        merge_parameters(&mut body, &req.extra_body)?;
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
        let url = format!("{}/messages", self.endpoint.as_deref().unwrap_or("https://api.anthropic.com/v1").trim_end_matches('/'));
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
        let mut headers = vec![
            ("x-api-key".to_string(), api_key.clone()),
            ("anthropic-version".to_string(), "2023-06-01".to_string()),
        ];
        headers.extend(req.extra_headers.clone());
        merge_parameters(&mut body, &req.extra_body)?;
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
        let url = format!("{}/models/{}:{}", self.endpoint.as_deref().unwrap_or("https://generativelanguage.googleapis.com/v1beta").trim_end_matches('/'), req.model, method);
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
        merge_parameters(&mut body, &req.extra_body)?;
        let mut headers = vec![("x-goog-api-key".into(), api_key.clone())];
        headers.extend(req.extra_headers.clone());
        Ok((url, headers, body))
    }

    // ---------- SSE 发送与解析 ----------

    async fn send_stream(
        &self,
        url: String,
        headers: Vec<(String, String)>,
        mut body: Value,
    ) -> Result<std::pin::Pin<Box<dyn Stream<Item = Result<StreamEvent, ProviderError>> + Send>>, ProviderError> {
        for key in &self.remove_parameters {
            if protected(key) { return Err(ProviderError::Config(format!("reserved parameter {key}"))); }
            if let Some(map) = body.as_object_mut() { map.remove(key); }
        }
        let mut request = self.client.post(&url).json(&body);
        for (k, v) in headers {
            request = request.header(&k, &v);
        }
        let response = request
            .send()
            .await
            .map_err(ProviderError::network)?;
        let status = response.status();
        if !status.is_success() {
            let retry_after = response.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(parse_retry_after);
            let body = response.text().await.unwrap_or_default();
            if status.as_u16() == 529 && matches!(self.kind,ProviderKind::Anthropic { .. }) {
                return Err(ProviderError::Upstream { kind:"overloaded_error".into(),message:diagnostic_body(&body,self.api_key()),status:Some(529),retry_after });
            }
            return Err(ProviderError::Http { status: status.as_u16(), body: diagnostic_body(&body,self.api_key()), retry_after });
        }
        let is_json = response.headers().get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()).is_some_and(|v| v.contains("application/json"));
        if is_json {
            let mut value: Value = response.json().await.map_err(|_| ProviderError::InvalidResponse("invalid JSON".into()))?;
            if value.get("error").is_some() { let mut error=upstream_error(&value); error.redact(self.api_key()); return Err(error); }
            let valid = match &self.kind {
                ProviderKind::OpenAiCompat { .. } => value["choices"].as_array().is_some_and(|c| c.iter().any(|v| v["message"].is_object() && v["finish_reason"].is_string())),
                ProviderKind::Anthropic { .. } => value["type"] == "message" && value["content"].is_array() && value["stop_reason"].is_string(),
                ProviderKind::Gemini { .. } => value["candidates"].as_array().is_some_and(|c| c.iter().any(|v| v["finishReason"].is_string())),
            };
            if !valid { return Err(ProviderError::InvalidResponse("missing response or completion marker".into())); }
            if let Some(choices) = value.get_mut("choices").and_then(Value::as_array_mut) {
                for choice in choices {
                    if let Some(message) = choice.get("message").cloned() { choice["delta"] = message; }
                }
            }
            let mut events = if value["type"].as_str() == Some("message") {
                let mut events = Vec::new();
                for block in value["content"].as_array().into_iter().flatten() {
                    if let Some(text) = block["text"].as_str() { events.push(StreamEvent::Token(text.into())); }
                    if let Some(text) = block["thinking"].as_str() { events.push(StreamEvent::Reasoning(text.into())); }
                }
                if let Some(reason) = value["stop_reason"].as_str() { events.push(StreamEvent::FinishReason(reason.into())); }
                events.push(StreamEvent::Usage {
                    input: value["usage"]["input_tokens"].as_i64(),
                    output: value["usage"]["output_tokens"].as_i64(),
                });
                events
            } else { normalize(value) };
            events.push(StreamEvent::Done);
            return Ok(Box::pin(futures_util::stream::iter(events.into_iter().map(Ok))));
        }
        let api_key=self.api_key().to_string();
        Ok(Box::pin(sse_events(response.bytes_stream()).map(move |event| match event {
            Ok(StreamEvent::Error(mut error)) => { error.redact(&api_key); Ok(StreamEvent::Error(error)) },
            Err(mut error) => { error.redact(&api_key); Err(error) },
            other => other,
        })))
    }
}

fn protected(key: &str) -> bool { matches!(key, "model" | "messages" | "stream" | "contents" | "system" | "systemInstruction") }
fn merge_parameters(body: &mut Value, extra: &Value) -> Result<(), ProviderError> {
    if let Some(extra) = extra.as_object() { for (k,v) in extra {
        if protected(k) { return Err(ProviderError::Config(format!("reserved parameter {k}"))); }
        if let (Some(target), Some(source)) = (body[k].as_object_mut(), v.as_object()) { target.extend(source.clone()); }
        else { body[k] = v.clone(); }
    } } Ok(())
}
fn redact_text(text: &str, api_key: &str) -> String {
    let redacted=if api_key.is_empty() { text.to_string() } else { text.replace(api_key,"[REDACTED]") };
    redacted.chars().take(2000).collect()
}
fn diagnostic_body(body: &str, api_key: &str) -> String {
    // Keep actionable upstream details, but never the route credential or arbitrary echoed request bodies.
    serde_json::from_str::<Value>(body).ok().map(|v| {
        let e = v.get("error").unwrap_or(&v);
        ["type","code","status","param","message"].iter().filter_map(|k| e.get(k).map(|v| format!("{k}={}", v.as_str().map(|text|redact_text(text,api_key)).unwrap_or_else(||v.to_string())))).collect::<Vec<_>>().join(", ")
    }).filter(|s| !s.is_empty()).map(|s| redact_text(&s,api_key)).unwrap_or_else(|| "upstream rejected request".into())
}
fn parse_retry_after(value: &str) -> Option<u64> {
    value.parse().ok().or_else(|| httpdate::parse_http_date(value).ok().map(|t| t.duration_since(std::time::SystemTime::now()).unwrap_or_default().as_secs()))
}

/// 字节流 → SSE data 行 → 归一化 StreamEvent。
fn sse_events(
    stream: impl Stream<Item = Result<bytes::Bytes, reqwest::Error>>,
) -> impl Stream<Item = Result<StreamEvent, ProviderError>> {
    futures_util::stream::unfold(
        (Box::pin(stream), Vec::<u8>::new(), std::collections::VecDeque::new(), false),
        |(mut stream, mut buf, mut pending, mut done)| async move {
            loop {
                if let Some(event) = pending.pop_front() {
                    return Some((Ok(event), (stream, buf, pending, done)));
                }
                if done { return None; }
                // Decode only complete frames, never partial UTF-8 chunks.
                let boundary = [b"\r\n\r\n".as_slice(), b"\n\n".as_slice(), b"\r\r".as_slice()]
                    .iter().filter_map(|sep| buf.windows(sep.len()).position(|w| w == *sep)
                        .map(|pos| (pos, sep.len()))).min_by_key(|(pos, _)| *pos);
                if let Some((pos, length)) = boundary {
                    let bytes: Vec<u8> = buf.drain(..pos + length).collect();
                    let event = match std::str::from_utf8(&bytes[..pos]) {
                        Ok(event) => event,
                        Err(error) => return Some((Err(ProviderError::InvalidResponse(error.to_string())),
                            (stream, buf, pending, true))),
                    };
                    let data = event.lines().filter_map(|line| line.strip_prefix("data:")
                        .map(|v| v.trim_start_matches(' '))).collect::<Vec<_>>().join("\n");
                    if data.trim() == "[DONE]" {
                        pending.push_back(StreamEvent::Done);
                        done = true;
                    } else if !data.trim().is_empty() {
                        match serde_json::from_str::<Value>(&data) {
                            Ok(value) => { let events = normalize(value); done = events.iter().any(|e| matches!(e, StreamEvent::Done | StreamEvent::Error(_))); pending.extend(events); },
                            Err(error) => return Some((Err(ProviderError::InvalidResponse(format!("invalid SSE JSON: {error}"))),
                                (stream, buf, pending, true))),
                        }
                    }
                    continue;
                }
                match stream.next().await {
                    Some(Ok(bytes)) => buf.extend_from_slice(&bytes),
                    Some(Err(error)) => return Some((Err(ProviderError::network(error)),
                        (stream, buf, pending, true))),
                    None => return Some((Err(ProviderError::Disconnected), (stream, buf, pending, true))),
                }
            }
        },
    )
}

/// 上游 chunk → StreamEvent 列表（getStreamingReply 的服务端等价）。
fn normalize(v: Value) -> Vec<StreamEvent> {
    let mut out = Vec::new();
    // 错误传播
    if let Some(err) = v.get("error") {
        out.push(StreamEvent::Error(upstream_error(err)));
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
            if let Some(reason) = choice["finish_reason"].as_str() {
                out.push(StreamEvent::FinishReason(reason.into()));
            }
            // Only [DONE] terminates an OpenAI SSE stream.
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
                if let Some(reason) = v["delta"]["stop_reason"].as_str() { out.push(StreamEvent::FinishReason(reason.into())); }
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
                out.push(StreamEvent::Error(upstream_error(&v)));
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
                        if let Some(text) = part["text"].as_str() { out.push(StreamEvent::Reasoning(text.into())); }
                        continue;
                    }
                    if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                        if !text.is_empty() {
                            out.push(StreamEvent::Token(text.to_string()));
                        }
                    }
                }
            }
            if let Some(reason) = c0["finishReason"].as_str() {
                out.push(StreamEvent::FinishReason(reason.into()));
            }
        }
        if let Some(u) = v.get("usageMetadata") {
            out.push(StreamEvent::Usage {
                input: u.get("promptTokenCount").and_then(|t| t.as_i64()),
                output: u.get("candidatesTokenCount").and_then(|t| t.as_i64()),
            });
        }
        if candidates.first().is_some_and(|c| c["finishReason"].is_string()) {
            out.push(StreamEvent::Done);
        }
        return out;
    }
    vec![StreamEvent::Error(ProviderError::InvalidResponse("unknown event format".into()))]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_variant_has_serializable_diagnostics() {
        for error in [ProviderError::InvalidResponse("bad JSON".into()),ProviderError::Timeout("budget".into()),ProviderError::Network("reset".into()),ProviderError::Aborted,ProviderError::Disconnected,ProviderError::Config("limit".into()),ProviderError::Http{status:503,body:String::new(),retry_after:Some(2)},upstream_error(&json!({"error":{"type":"overloaded_error"}}))] {
            let value=serde_json::to_value(error).unwrap(); assert!(value["type"].is_string());
        }
    }

    #[tokio::test]
    async fn eof_without_protocol_end_is_failure_even_after_reasoning() {
        let wire = format!("data: {}\n\n",json!({"choices":[{"delta":{"reasoning_content":"thinking"}}]}));
        let stream=sse_events(futures_util::stream::iter(vec![Ok::<_,reqwest::Error>(bytes::Bytes::from(wire))]));
        tokio::pin!(stream);
        assert!(matches!(stream.next().await,Some(Ok(StreamEvent::Reasoning(_)))));
        assert!(matches!(stream.next().await,Some(Err(ProviderError::Disconnected))));
    }
    #[test]
    fn retry_classification_is_structured_and_terminal_by_default() {
        for status in [400,401,403,404,413,422,500] { assert!(!ProviderError::Http{status,body:"temporarily unavailable".into(),retry_after:None}.retryable()); }
        for status in [429,502,503,504] { assert!(ProviderError::Http{status,body:String::new(),retry_after:None}.retryable()); }
        assert!(upstream_error(&json!({"error":{"type":"overloaded_error"}})).retryable());
        assert!(!upstream_error(&json!({"error":{"message":"overloaded_error"}})).retryable());
    }

    #[tokio::test]
    async fn fragmented_utf8_and_all_events_in_one_frame_survive() {
        let wire = format!("data: {}\r\n\r\ndata: [DONE]\n\n", json!({
            "choices":[{"delta":{"content":"中文😀", "reasoning_content":"推理"}}],
            "usage":{"prompt_tokens":4,"completion_tokens":3}
        }));
        let bytes = wire.into_bytes().into_iter().map(|b| Ok::<_, reqwest::Error>(bytes::Bytes::from(vec![b])));
        let events = sse_events(futures_util::stream::iter(bytes));
        tokio::pin!(events);
        let mut text = String::new();
        let mut reasoning = String::new();
        let mut usage = None;
        while let Some(event) = events.next().await {
            match event.unwrap() {
                StreamEvent::Token(value) => text.push_str(&value),
                StreamEvent::Reasoning(value) => reasoning.push_str(&value),
                StreamEvent::Usage { input, output } => usage = Some((input,output)),
                _ => {},
            }
        }
        assert_eq!(text,"中文😀");
        assert_eq!(reasoning,"推理");
        assert_eq!(usage,Some((Some(4),Some(3))));
    }

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
    fn gemini_usage_is_delivered_before_done() {
        let events = normalize(json!({"candidates":[{"finishReason":"MAX_TOKENS"}],
            "usageMetadata":{"promptTokenCount":123,"candidatesTokenCount":45}}));
        assert!(matches!(events[0], StreamEvent::FinishReason(_)));
        assert!(matches!(events[1], StreamEvent::Usage { input:Some(123), output:Some(45) }));
        assert!(matches!(events[2], StreamEvent::Done));
    }

    #[test]
    fn normalize_gemini_thought_skipped() {
        let v = json!({"candidates": [{"content": {"parts": [{"text": "t", "thought": true}]}}]});
        assert!(matches!(&normalize(v)[0], StreamEvent::Reasoning(t) if t == "t"));
    }

    #[test]
    fn normalize_error() {
        let v = json!({"error": {"message": "quota"}});
        let evs = normalize(v);
        assert!(matches!(&evs[0], StreamEvent::Error(ProviderError::Upstream { .. })));
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
        assert!(!url.contains("key="));
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
