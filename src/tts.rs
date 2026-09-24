//! TTS adapters, following SillyTavern 8172dcd's provider request contracts.
//! Credentials stay in secrets.json. Audio uses HTTP so a browser can cancel it
//! independently of the chat WebSocket. Local model servers are valid targets.

use actix_web::{HttpResponse, ResponseError, http::StatusCode, web};
use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::StreamExt;
use once_cell::sync::Lazy;
use reqwest::{Client, RequestBuilder, Url};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};

use crate::{connection::active_secret, state::SharedState};

const MAX_AUDIO: usize = 64 * 1024 * 1024;
static CATALOG: Lazy<Vec<Value>> = Lazy::new(|| {
    serde_json::from_str(include_str!("../resources/tts-providers.json")).expect("TTS catalog")
});
static CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(180))
        // Never forward provider-specific credentials across a redirect.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("TTS HTTP client")
});

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct TtsError {
    status: StatusCode,
    message: String,
}
impl ResponseError for TtsError {
    fn status_code(&self) -> StatusCode {
        self.status
    }
    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status).json(json!({"error": self.message}))
    }
}
fn bad(message: impl Into<String>) -> TtsError {
    TtsError {
        status: StatusCode::BAD_REQUEST,
        message: message.into(),
    }
}
fn upstream(message: impl Into<String>) -> TtsError {
    TtsError {
        status: StatusCode::BAD_GATEWAY,
        message: message.into(),
    }
}
fn network(error: reqwest::Error) -> TtsError {
    // reqwest's Display can contain URLs, including query credentials.
    upstream(if error.is_timeout() {
        "语音服务请求超时，请检查端点或稍后重试"
    } else {
        "无法连接语音服务，请检查端点和网络"
    })
}

#[derive(Deserialize, Default)]
pub struct TtsRequest {
    #[serde(default)]
    provider: String,
    #[serde(default)]
    settings: Option<Value>,
    #[serde(default)]
    text: String,
    #[serde(default)]
    voice: String,
    #[serde(default)]
    character: String,
    #[serde(default)]
    characters: Vec<String>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    action: String,
    #[serde(default)]
    model_id: String,
    #[serde(default)]
    value: bool,
}

struct Context {
    name: String,
    config: Value,
    catalog: Value,
    secrets: Value,
}
impl Context {
    async fn new(state: &SharedState, input: &TtsRequest) -> Result<Self, TtsError> {
        let saved = state.settings.read().await;
        let tts = &saved["extension_settings"]["tts"];
        let name = if input.provider.is_empty() {
            s(tts, "currentProvider", "System")
        } else {
            input.provider.clone()
        };
        let catalog = CATALOG
            .iter()
            .find(|v| v["name"] == name)
            .cloned()
            .ok_or_else(|| bad("未知的 TTS 服务商"))?;
        let mut config = catalog["defaults"].clone();
        if let Some(map) = tts[&name].as_object() {
            config.as_object_mut().unwrap().extend(map.clone());
        }
        if let Some(overrides) = &input.settings {
            let map = overrides
                .as_object()
                .ok_or_else(|| bad("TTS 配置必须是 JSON 对象"))?;
            config.as_object_mut().unwrap().extend(map.clone());
        }
        if matches!(name.as_str(), "Edge" | "Coqui")
            && input
                .settings
                .as_ref()
                .and_then(|v| v.get("provider_endpoint"))
                .is_none()
            && tts[&name].get("provider_endpoint").is_none()
        {
            if let Some(url) = saved["extension_settings"]["apiUrl"].as_str() {
                config["provider_endpoint"] = json!(url);
            }
        }
        drop(saved);
        Ok(Self {
            name,
            config,
            catalog,
            secrets: state.secrets.read().await.clone(),
        })
    }
    fn key(&self, key: &str, required: bool) -> Result<String, TtsError> {
        let value = active_secret(&self.secrets, key).unwrap_or_default();
        if required && value.is_empty() {
            return Err(bad(format!("请先保存 {} 的密钥：{key}", self.name)));
        }
        Ok(value)
    }
    fn url(&self, suffix: &str) -> Result<Url, TtsError> {
        let base = if self.name == "Azure" {
            let region = s(&self.config, "region", "");
            if region.is_empty()
                || !region
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-')
            {
                return Err(bad("请填写有效的 Azure 区域"));
            }
            s(
                &self.config,
                "provider_endpoint",
                &format!("https://{region}.tts.speech.microsoft.com"),
            )
        } else {
            s(
                &self.config,
                "provider_endpoint",
                &s(&self.config, "apiHost", ""),
            )
        };
        http_url(&format!("{}{suffix}", base.trim_end_matches('/')))
    }
    fn request(&self, method: reqwest::Method, url: Url) -> Result<RequestBuilder, TtsError> {
        let mut req = CLIENT.request(method, url);
        let key = self.catalog["secrets"]
            .as_array()
            .and_then(|v| v.first())
            .and_then(Value::as_str);
        if let Some(key) = key {
            let required = !matches!(
                self.name.as_str(),
                "OpenAI Compatible" | "Edge" | "Coqui" | "Silero" | "XTTSv2"
            );
            let value = self.key(key, required)?;
            if !value.is_empty() {
                req = match self.name.as_str() {
                    "ElevenLabs" => req.header("xi-api-key", value),
                    "Azure" => req.header("Ocp-Apim-Subscription-Key", value),
                    "Google Gemini TTS" => req.header("x-goog-api-key", value),
                    "Volcengine" => req
                        .header("X-Api-App-Id", value)
                        .header("X-Api-Access-Key", self.key("volcengine_access_key", true)?)
                        .header("X-Api-Resource-Id", s(&self.config, "resource_id", "")),
                    _ => req.bearer_auth(value),
                };
            }
        }
        Ok(req)
    }
    fn get(&self, suffix: &str) -> Result<RequestBuilder, TtsError> {
        self.request(reqwest::Method::GET, self.url(suffix)?)
    }
    fn post(&self, suffix: &str) -> Result<RequestBuilder, TtsError> {
        self.request(reqwest::Method::POST, self.url(suffix)?)
    }
}

fn s(v: &Value, key: &str, fallback: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or(fallback)
        .to_string()
}
fn n(v: &Value, key: &str, fallback: f64) -> f64 {
    v.get(key)
        .and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse::<f64>().ok()))
        .filter(|n| n.is_finite())
        .unwrap_or(fallback)
}
fn b(v: &Value, key: &str, fallback: bool) -> bool {
    v.get(key).and_then(Value::as_bool).unwrap_or(fallback)
}
fn http_url(value: &str) -> Result<Url, TtsError> {
    let url = Url::parse(value).map_err(|_| bad("语音服务端点必须是完整的 HTTP(S) 地址"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(bad("语音服务端点必须是 HTTP(S) 地址，密钥请单独保存"));
    }
    Ok(url)
}
async fn send(req: RequestBuilder) -> Result<reqwest::Response, TtsError> {
    let response = req.send().await.map_err(network)?;
    if !response.status().is_success() {
        // Deliberately do not reflect upstream bodies: proxies can echo secrets.
        return Err(upstream(format!(
            "语音服务返回 HTTP {}，请检查密钥、模型、音色和额度",
            response.status().as_u16()
        )));
    }
    Ok(response)
}
async fn bytes(response: reqwest::Response) -> Result<Vec<u8>, TtsError> {
    if response
        .content_length()
        .is_some_and(|v| v > MAX_AUDIO as u64)
    {
        return Err(upstream("语音响应过大，请缩短文本"));
    }
    let mut output = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(network)?;
        if output.len() + chunk.len() > MAX_AUDIO {
            return Err(upstream("语音响应过大，请缩短文本"));
        }
        output.extend_from_slice(&chunk);
    }
    Ok(output)
}
async fn json_response(req: RequestBuilder) -> Result<Value, TtsError> {
    serde_json::from_slice(&bytes(send(req).await?).await?)
        .map_err(|_| upstream("语音服务未返回有效 JSON"))
}
fn voice(id: impl Into<String>, name: impl Into<String>, lang: impl Into<String>) -> Value {
    json!({"voice_id": id.into(), "name": name.into(), "lang": lang.into()})
}
fn normalize_voices(value: &Value) -> Vec<Value> {
    let list = value
        .as_array()
        .or_else(|| value["voices"].as_array())
        .or_else(|| value["speakers"].as_array());
    list.into_iter()
        .flatten()
        .filter_map(|v| {
            if let Some(id) = v.as_str() {
                return Some(voice(id, id, ""));
            }
            let id = ["voice_id", "ShortName", "id", "value", "filename", "name"]
                .iter()
                .find_map(|key| v.get(key).filter(|x| x.is_string() || x.is_number()))?;
            let id = id
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| id.to_string());
            let name = ["display_name", "name", "label", "ShortName"]
                .iter()
                .find_map(|key| v.get(key).and_then(Value::as_str))
                .unwrap_or(&id);
            let lang = ["lang", "language", "Locale"]
                .iter()
                .find_map(|key| v.get(key).and_then(Value::as_str))
                .unwrap_or("");
            Some(voice(&id, name, lang))
        })
        .collect()
}
fn configured_voices(c: &Context) -> Vec<Value> {
    let mut result = normalize_voices(&c.catalog["voices"]);
    if c.config["available_voices"].is_array() {
        result = normalize_voices(&c.config["available_voices"]);
    }
    result.extend(normalize_voices(&c.config["customVoices"]));
    if c.name == "SpeechT5" {
        result = normalize_voices(&c.config["speakers"]);
    }
    if c.name == "Coqui" {
        result = c.config["customVoices"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(id, _)| voice(id, id, ""))
            .collect();
    }
    result
}

pub async fn voices(
    state: web::Data<SharedState>,
    input: web::Json<TtsRequest>,
) -> Result<HttpResponse, TtsError> {
    let c = Context::new(&state, &input).await?;
    let suffix = match c.name.as_str() {
        "ElevenLabs" => Some("/voices"),
        "Azure" => Some("/cognitiveservices/voices/list"),
        "AllTalk" => Some("/api/voices"),
        "Chatterbox" => Some("/get_predefined_voices"),
        "CosyVoice (Unofficial)"
        | "GPT-SoVITS-Adapter"
        | "GPT-SoVITS-V2 (Unofficial)"
        | "XTTSv2"
        | "Silero" => Some("/speakers"),
        "GSVI" => Some("/character_list"),
        "SBVits2" => Some("/models/info"),
        "VITS" => Some("/voice/speakers"),
        "Edge" => Some(if s(&c.config, "provider", "extras") == "plugin" {
            "/api/plugins/edge-tts/list"
        } else {
            "/api/edge-tts/list"
        }),
        "Pollinations" => Some("/text/models"),
        _ => None,
    };
    let mut result = if let Some(suffix) = suffix {
        let data = json_response(c.get(suffix)?).await?;
        match c.name.as_str() {
            "GSVI" => data
                .as_object()
                .into_iter()
                .flatten()
                .map(|(id, _)| voice(id, id, "zh-CN"))
                .collect(),
            "SBVits2" => {
                let mut out = Vec::new();
                for (model, config) in data.as_object().into_iter().flatten() {
                    for (speaker, id) in config["spk2id"].as_object().into_iter().flatten() {
                        for (style, _) in config["style2id"].as_object().into_iter().flatten() {
                            out.push(voice(
                                format!("{model}-{id}-{style}"),
                                format!("{speaker} ({style})"),
                                "",
                            ));
                        }
                    }
                }
                out
            }
            "VITS" => {
                let mut out = Vec::new();
                for (model, voices) in data.as_object().into_iter().flatten() {
                    for v in normalize_voices(voices) {
                        out.push(voice(
                            format!("{model}&{}", s(&v, "voice_id", "")),
                            format!("[{model}] {}", s(&v, "name", "")),
                            s(&v, "lang", ""),
                        ));
                    }
                }
                out
            }
            "Pollinations" => {
                let model = s(&c.config, "model", "openai-audio");
                let found = data
                    .as_array()
                    .and_then(|models| models.iter().find(|m| m["name"] == model))
                    .ok_or_else(|| bad("未找到指定模型的音色"))?;
                normalize_voices(&found["voices"])
            }
            _ => normalize_voices(&data),
        }
    } else if matches!(c.name.as_str(), "TTS WebUI" | "Electron Hub") {
        let mut url = c.url("")?;
        let path = if c.name == "TTS WebUI" {
            url.path().replace(
                "/speech",
                &format!("/voices/{}", s(&c.config, "model", "chatterbox")),
            )
        } else {
            url.path().replace("/audio/speech", "/models")
        };
        url.set_path(&path);
        let data = json_response(c.request(reqwest::Method::GET, url)?).await?;
        if c.name == "TTS WebUI" {
            normalize_voices(&data)
        } else {
            let model = s(&c.config, "model", "tts-1");
            let list = data["data"].as_array().or_else(|| data.as_array());
            list.and_then(|v| v.iter().find(|m| m["id"] == model))
                .map(|m| normalize_voices(&m["voices"]))
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| configured_voices(&c))
        }
    } else {
        configured_voices(&c)
    };
    if c.name == "Chatterbox" {
        if let Ok(data) = json_response(c.get("/get_reference_files")?).await {
            result.extend(
                data.as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(|v| voice(format!("ref_{v}"), format!("[Clone] {v}"), "")),
            );
        }
    }
    let mut seen = std::collections::HashSet::new();
    result.retain(|v| seen.insert(s(v, "voice_id", "")));
    Ok(HttpResponse::Ok().json(result))
}

pub async fn models(
    state: web::Data<SharedState>,
    input: web::Json<TtsRequest>,
) -> Result<HttpResponse, TtsError> {
    let c = Context::new(&state, &input).await?;
    let result = if matches!(
        c.name.as_str(),
        "ElevenLabs" | "Electron Hub" | "OpenAI Compatible" | "TTS WebUI"
    ) {
        let mut url = c.url(if c.name == "ElevenLabs" {
            "/models"
        } else {
            ""
        })?;
        if c.name != "ElevenLabs" {
            let path = url.path().replace("/audio/speech", "/models");
            url.set_path(&path);
        }
        let data = json_response(c.request(reqwest::Method::GET, url)?).await?;
        data.as_array()
            .or_else(|| data["data"].as_array())
            .into_iter()
            .flatten()
            .filter(|v| {
                v.get("can_do_text_to_speech")
                    .and_then(Value::as_bool)
                    .unwrap_or(true)
            })
            .filter_map(|v| {
                v.get("id")
                    .or_else(|| v.get("model_id"))
                    .or_else(|| v.get("name"))
                    .cloned()
            })
            .collect::<Vec<_>>()
    } else {
        let mut list = c.catalog["models"].as_array().cloned().unwrap_or_default();
        list.extend(
            c.config["customModels"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
        );
        list
    };
    Ok(HttpResponse::Ok().json(result))
}

fn pick(config: &Value, keys: &[&str]) -> Value {
    Value::Object(
        keys.iter()
            .filter_map(|key| config.get(key).map(|v| (key.to_string(), v.clone())))
            .collect(),
    )
}
fn form(value: &Value) -> BTreeMap<String, String> {
    value
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, v)| {
            (
                k.clone(),
                v.as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| v.to_string()),
            )
        })
        .collect()
}
fn escaped_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&apos;")
        .replace('"', "&quot;")
}
fn wav(pcm: Vec<u8>, sample_rate: u32, float: bool) -> Vec<u8> {
    let bits = if float { 32u16 } else { 16u16 };
    let mut out = Vec::with_capacity(pcm.len() + 44);
    out.extend(b"RIFF");
    out.extend((pcm.len() as u32 + 36).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend((if float { 3u16 } else { 1u16 }).to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(sample_rate.to_le_bytes());
    out.extend((sample_rate * u32::from(bits / 8)).to_le_bytes());
    out.extend((bits / 8).to_le_bytes());
    out.extend(bits.to_le_bytes());
    out.extend(b"data");
    out.extend((pcm.len() as u32).to_le_bytes());
    out.extend(pcm);
    out
}
// Streaming WAV servers use zero/unknown lengths until the stream ends.
// Finalize those lengths after buffering so HTMLAudioElement can seek and finish.
fn finalize_wav(data: &mut [u8]) {
    if data.len() < 44 || &data[..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return;
    }
    let total = data.len();
    let mut offset = 12;
    while offset + 8 <= total {
        let size = u32::from_le_bytes(data[offset + 4..offset + 8].try_into().unwrap()) as usize;
        if &data[offset..offset + 4] == b"data" {
            if size == 0 || size > total - offset - 8 {
                data[offset + 4..offset + 8]
                    .copy_from_slice(&((total - offset - 8) as u32).to_le_bytes());
            }
            data[4..8].copy_from_slice(&((total - 8) as u32).to_le_bytes());
            return;
        }
        let Some(next) = offset
            .checked_add(8)
            .and_then(|v| v.checked_add(size))
            .and_then(|v| v.checked_add(size % 2))
        else {
            return;
        };
        offset = next;
    }
}
fn audio(mut data: Vec<u8>, mime: &str) -> Result<HttpResponse, TtsError> {
    if data.is_empty() {
        return Err(upstream("语音服务返回了空音频"));
    }
    finalize_wav(&mut data);
    Ok(HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .insert_header(("X-Content-Type-Options", "nosniff"))
        .content_type(mime.to_string())
        .body(data))
}
fn decode64(value: &Value) -> Result<Vec<u8>, TtsError> {
    STANDARD
        .decode(
            value
                .as_str()
                .ok_or_else(|| upstream("语音响应缺少音频数据"))?,
        )
        .map_err(|_| upstream("语音响应包含无效的 Base64 音频"))
}
async fn raw_audio(req: RequestBuilder, fallback: &str) -> Result<HttpResponse, TtsError> {
    let response = send(req).await?;
    let mime = response
        .headers()
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .unwrap_or(fallback)
        .to_string();
    if !mime.starts_with("audio/") && !mime.starts_with("application/octet-stream") {
        return Err(upstream("语音服务返回了非音频内容"));
    }
    let data = bytes(response).await?;
    audio(
        data,
        if mime.starts_with("audio/") {
            &mime
        } else {
            fallback
        },
    )
}

pub async fn synthesize(
    state: web::Data<SharedState>,
    input: web::Json<TtsRequest>,
) -> Result<HttpResponse, TtsError> {
    if input.text.trim().is_empty() || input.text.chars().count() > 20_000 {
        return Err(bad("朗读文本必须为 1–20000 个字符"));
    }
    if input.voice.trim().is_empty() {
        return Err(bad("请先选择音色"));
    }
    let c = Context::new(&state, &input).await?;
    let cfg = &c.config;
    let text = &input.text;
    let v = &input.voice;
    let speed = n(cfg, "speed", 1.0);
    let model = s(cfg, "model", "tts-1");
    let request = match c.name.as_str() {
        "OpenAI" | "OpenAI Compatible" | "Electron Hub" | "TTS WebUI" => {
            let mut body = json!({"input": text, "voice": v, "model": model, "speed": speed, "response_format": "mp3"});
            if c.name == "OpenAI" {
                if let Some(instructions) = cfg["characterInstructions"].get(&input.character).and_then(Value::as_str) {
                    if model.starts_with("gpt-4o-mini-tts") { body["instructions"] = json!(instructions); }
                }
            } else if c.name == "Electron Hub" {
                let extra = pick(cfg, &["temperature", "top_p", "instructions", "speaker_transcript", "cfg_scale", "cfg_filter_top_k", "speech_rate", "pitch_adjustment", "emotional_style"]);
                body.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            } else if c.name == "TTS WebUI" {
                body["response_format"] = json!("wav");
                body["stream"] = json!(b(cfg, "streaming", false));
                body["params"] = pick(cfg, &["desired_length", "max_length", "halve_first_chunk", "exaggeration", "cfg_weight", "temperature", "device", "dtype", "cpu_offload", "chunked", "cache_voice", "tokens_per_slice", "remove_milliseconds", "remove_milliseconds_start", "chunk_overlap_method", "seed"]);
            }
            if let Some(extra) = cfg["extra_body"].as_object() { body.as_object_mut().unwrap().extend(extra.clone()); }
            c.post("")?.json(&body)
        }
        "ElevenLabs" => {
            if b(cfg, "reuse_history", true) {
                if let Ok(history) = json_response(c.get("/history")?).await {
                    if let Some(item) = history["history"].as_array().and_then(|items| items.iter().find(|item| item["text"] == *text && item["voice_id"] == *v && item["model_id"] == model)) {
                        if let Some(id) = item["history_item_id"].as_str() {
                            return raw_audio(c.get(&format!("/history/{}/audio", encode_path(id)))?, "audio/mpeg").await;
                        }
                    }
                }
            }
            c.post(&format!("/text-to-speech/{}", encode_path(v)))?.json(&json!({"text": text, "model_id": model,
                "voice_settings": {"stability": n(cfg,"stability",0.75), "similarity_boost":n(cfg,"similarity_boost",0.75), "speed":speed,
                "style":n(cfg,"style_exaggeration",0.0), "use_speaker_boost":b(cfg,"speaker_boost",true)}}))
        }
        "Azure" => {
            let lang = v.split('-').take(2).collect::<Vec<_>>().join("-");
            let ssml = format!("<speak version='1.0' xmlns='http://www.w3.org/2001/10/synthesis' xml:lang='{}'><voice name='{}'>{}</voice></speak>", escaped_xml(&lang), escaped_xml(v), escaped_xml(text));
            c.post("/cognitiveservices/v1")?.header("Content-Type", "application/ssml+xml")
                .header("X-Microsoft-OutputFormat", "riff-24khz-16bit-mono-pcm").body(ssml)
        }
        "Google Gemini TTS" => {
            if s(cfg,"apiType","makersuite") != "makersuite" { return Err(bad("目前 Gemini TTS 使用 AI Studio API 密钥；请将 API 类型设为 makersuite")); }
            let data = json_response(c.post(&format!("/models/{}:generateContent", encode_path(&model)))?.json(&json!({
                "contents":[{"role":"user","parts":[{"text":text}]}],
                "generationConfig":{"responseModalities":["AUDIO"],"speechConfig":{"voiceConfig":{"prebuiltVoiceConfig":{"voiceName":v}}}}
            }))).await?;
            let part = data["candidates"][0]["content"]["parts"].as_array().and_then(|p| p.iter().find_map(|p| p.get("inlineData"))).ok_or_else(|| upstream("Gemini 未返回音频"))?;
            let pcm = decode64(&part["data"])?;
            let mime = s(part,"mimeType","audio/L16;rate=24000");
            return if mime.to_lowercase().contains("audio/l16") {
                let rate = mime.split(';').find_map(|v| v.trim().strip_prefix("rate=").and_then(|v| v.parse().ok())).unwrap_or(24000);
                audio(wav(pcm, rate, false), "audio/wav")
            } else { audio(pcm, &mime) };
        }
        "Google Translate" => c.get("")?.query(&[("ie","UTF-8"),("client","tw-ob"),("tl",v),("q",text)]),
        "MiniMax" => {
            let voice_id = if v == "customVoice" { s(cfg,"customVoiceId","") } else { v.clone() };
            let mut body = json!({"model":model,"text":text,"stream":false,
                "voice_setting":{"voice_id":voice_id,"speed":speed.clamp(0.5,2.0),"vol":n(cfg,"volume",1.0).clamp(0.0,10.0),"pitch":n(cfg,"pitch",0.0).clamp(-12.0,12.0)},
                "audio_setting":{"sample_rate":n(cfg,"audioSampleRate",32000.0) as u32,"bitrate":n(cfg,"bitrate",128000.0) as u32,"format":s(cfg,"format","mp3"),"channel":1}});
            if !s(cfg,"language","").is_empty() { body["lang"] = cfg["language"].clone(); }
            let data = json_response(c.post("/v1/t2a_v2")?.query(&[("GroupId",c.key("minimax_group_id",true)?)]).json(&body)).await?;
            if data["base_resp"]["status_code"].as_i64().is_some_and(|v| v != 0) { return Err(upstream("MiniMax 合成失败，请检查密钥、Group ID、音色及额度")); }
            let format = s(cfg,"format","mp3");
            let mime = match format.as_str() { "wav" | "pcm" => "audio/wav", "flac" => "audio/flac", "aac" => "audio/aac", _ => "audio/mpeg" };
            if let Some(hex) = data["data"]["audio"].as_str() {
                let data = decode_hex(hex)?;
                return audio(if format == "pcm" { wav(data,n(cfg,"audioSampleRate",32000.0) as u32,false) } else { data },mime);
            }
            if let Some(url) = data["data"]["url"].as_str() { return raw_audio(CLIENT.get(http_url(url)?),mime).await; }
            return Err(upstream("MiniMax 未返回音频"));
        }
        "Volcengine" => {
            if s(cfg,"resource_id","").trim().is_empty() { return Err(bad("请填写火山引擎 Resource ID")); }
            let response = send(c.post("")?.json(&json!({"req_params":{"text":text,"speaker":v,
                "audio_params":{"format":"mp3","speech_rate":speed as i32},
                "additions":json!({"mute_cut_threshold":"400","mute_cut_remain_ms":"1","explicit_language":"crosslingual","enable_language_detector":true,"disable_markdown_filter":true,"cache_config":{"use_cache":true,"text_type":1}}).to_string()}}))).await?;
            let raw = bytes(response).await?;
            let mut data = Vec::new();
            for line in raw.split(|v| *v == b'\n').filter(|v| !v.iter().all(u8::is_ascii_whitespace)) {
                let item: Value = serde_json::from_slice(line).map_err(|_| upstream("火山引擎返回了无效的音频分段"))?;
                if !matches!(item["code"].as_i64(), Some(0 | 20000000)) { return Err(upstream("火山引擎合成失败，请检查音色和 Resource ID")); }
                if item["data"].as_str().is_some_and(|s| !s.is_empty()) { data.extend(decode64(&item["data"])?); }
            }
            return audio(data,"audio/mpeg");
        }
        "Novel" => c.get("")?.query(&[("text",text.as_str()),("voice","-1"),("seed",v),("opus","false"),("version","v2")]),
        "Chutes" => c.post("")?.json(&json!({"text":text,"voice":v,"speed":speed})),
        "Pollinations" => {
            let data = json_response(c.post("/v1/chat/completions")?.json(&json!({"model":model,"stream":false,"modalities":["text","audio"],
                "audio":{"format":"mp3","voice":v},"messages":[{"role":"user","content":format!("Say exactly this and nothing else:\n{text}")}]}))).await?;
            return audio(decode64(&data["choices"][0]["message"]["audio"]["data"])? ,"audio/mpeg");
        }
        "AllTalk" => {
            if s(cfg,"at_generation_method","") == "streaming_enabled" {
                c.get("/api/tts-generate-streaming")?.query(&[("text",text.as_str()),("voice",v),("language",&s(cfg,"language","en")),("output_file","stream_output.wav")])
            } else {
                let mut body = json!({"text_input":text,"text_filtering":"standard","character_voice_gen":v,
                    "narrator_enabled":s(cfg,"narrator_enabled","false"),"narrator_voice_gen":s(cfg,"narrator_voice_gen",""),
                    "text_not_inside":s(cfg,"at_narrator_text_not_inside","narrator"),"language":s(cfg,"language","en"),
                    "output_file_name":"nast_output","output_file_timestamp":"true","autoplay":"false","autoplay_volume":"0.8"});
                for (field, target) in [("rvc_character_voice","rvccharacter_voice_gen"),("rvc_character_pitch","rvccharacter_pitch"),("rvc_narrator_voice","rvcnarrator_voice_gen"),("rvc_narrator_pitch","rvcnarrator_pitch")] {
                    if s(cfg,"server_version","v2") == "v2" { body[target] = cfg[field].clone(); }
                }
                let data = json_response(c.post("/api/tts-generate")?.form(&form(&body))).await?;
                let path = data["output_file_url"].as_str().ok_or_else(|| upstream("AllTalk 未返回音频地址"))?;
                let url = c.url("/")?.join(path).map_err(|_| upstream("AllTalk 返回了无效的音频地址"))?;
                return raw_audio(CLIENT.get(http_url(url.as_str())?),"audio/wav").await;
            }
        }
        "Chatterbox" => {
            let mut body = pick(cfg,&["temperature","exaggeration","cfg_weight","seed","speed_factor","language","split_text","chunk_size","output_format"]);
            body["text"] = json!(text);
            if n(cfg,"seed",-1.0) < 0.0 { body["seed"] = json!(rand::random::<u32>() >> 1); }
            if let Some(name) = v.strip_prefix("ref_") { body["voice_mode"]=json!("clone");body["reference_audio_filename"]=json!(name); }
            else { body["voice_mode"]=json!("predefined");body["predefined_voice_id"]=json!(v); }
            c.post("/tts")?.json(&body)
        }
        "Coqui" => {
            let model = cfg["customVoices"][v].as_str().ok_or_else(|| bad("请配置 Coqui 自定义音色的模型 ID"))?;
            let cleaned = model.replace([']','"'],"");
            let tokens: Vec<_> = cleaned.split('[').collect();
            let multi = tokens[0].contains("multilingual");
            let parse = |i: usize| tokens.get(i).and_then(|s| s.parse::<u32>().ok());
            c.post("/api/text-to-speech/coqui/generate-tts")?.json(&json!({"text":text,"model_id":tokens[0],"language_id":if multi {parse(1)} else {None},"speaker_id":parse(if multi {2} else {1})}))
        }
        "Edge" => c.post(if s(cfg,"provider","extras") == "plugin" {"/api/plugins/edge-tts/generate"} else {"/api/edge-tts/generate"})?.json(&json!({"text":text,"voice":v,"rate":n(cfg,"rate",0.0)})),
        "CosyVoice (Unofficial)" => c.post("/")?.json(&json!({"text":text,"speaker":v,"streaming":if b(cfg,"streaming",false) {1} else {0}})),
        "GPT-SoVITS-Adapter" => c.post("/")?.json(&json!({"text":text,"card_name":input.characters,"use_st_adapter":true,"target_voice":v,"text_lang":s(cfg,"text_lang","zh"),"text_split_method":"cut5","batch_size":1,"media_type":s(cfg,"media_type","auto"),"streaming_mode":"true"})),
        "GPT-SoVITS-V2 (Unofficial)" => c.post("/")?.json(&json!({"text":text,"prompt_text":regex::Regex::new(r"\[.*?\]").unwrap().replace_all(v,""),"ref_audio_path":format!("./参考音频/{v}.wav"),"text_lang":s(cfg,"text_lang","zh"),"prompt_lang":s(cfg,"prompt_lang","zh"),"text_split_method":"cut5","batch_size":1,"media_type":"ogg","streaming_mode":"true"})),
        "GSVI" => {
            let mut body = pick(cfg,&["batch_size","speed","top_k","top_p","temperature","stream"]);
            body["text"]=json!(text); body["cha_name"]=json!(v);body["text_language"]=json!(s(cfg,"language","多语种混合"));
            c.get("/tts")?.query(&form(&body))
        }
        "SBVits2" => {
            let parts: Vec<_> = v.splitn(3,'-').collect();
            if parts.len()!=3 {return Err(bad("SBVits2 音色格式应为 model-speaker-style"));}
            let mut body = pick(cfg,&["sdp_ratio","noise","noisew","length","language","auto_split","split_interval","assist_text","assist_text_weight","style_weight","reference_audio_path"]);
            body["text"]=json!(text.replace("<br>","\n"));body["model_id"]=json!(parts[0]);body["speaker_id"]=json!(parts[1]);body["style"]=json!(parts[2]);
            c.post("/voice")?.query(&form(&body))
        }
        "Silero" => c.post("/generate")?.json(&json!({"text":text,"speaker":v,"session":"sillytavern"})),
        "VITS" => {
            let (model,id)=v.split_once('&').ok_or_else(|| bad("请从 VITS 音色列表选择音色"))?;
            if !matches!(model,"VITS"|"W2V2-VITS"|"BERT-VITS2") {return Err(bad("不支持的 VITS 模型类型"));}
            let mut body=pick(cfg,&["format","lang","length","noise","noisew","segment_size"]);
            body["text"]=json!(text);body["id"]=json!(id);
            if model=="W2V2-VITS" {body["emotion"]=cfg["dim_emotion"].clone();}
            if model=="BERT-VITS2" {body.as_object_mut().unwrap().extend(pick(cfg,&["sdp_ratio","emotion","text_prompt","style_text","style_weight"]).as_object().unwrap().clone());}
            let path=format!("/voice/{}",model.to_lowercase());
            if b(cfg,"streaming",false) {body["streaming"]=json!(true);c.get(&path)?.query(&form(&body))}
            else {c.post(&path)?.form(&form(&body))}
        }
        "XTTSv2" => {
            send(c.post("/set_tts_settings")?.json(&pick(cfg,&["temperature","speed","length_penalty","repetition_penalty","top_p","top_k","enable_text_splitting","stream_chunk_size"]))).await?;
            let body=json!({"text":text,"speaker_wav":v,"language":s(cfg,"language","en")});
            if b(cfg,"streaming",false) {c.get("/tts_stream/")?.query(&form(&body))} else {c.post("/tts_to_audio/")?.json(&body)}
        }
        _ => return Err(bad("此服务商在浏览器内合成，请使用前端朗读功能")),
    };
    raw_audio(
        request,
        if matches!(
            c.name.as_str(),
            "OpenAI"
                | "OpenAI Compatible"
                | "Electron Hub"
                | "ElevenLabs"
                | "Novel"
                | "Chutes"
                | "Google Translate"
                | "Edge"
        ) {
            "audio/mpeg"
        } else {
            "audio/wav"
        },
    )
    .await
}

fn encode_path(value: &str) -> String {
    // Encode a single path segment, including '/' and '.', so IDs cannot alter the endpoint.
    value
        .bytes()
        .map(|v| {
            if v.is_ascii_alphanumeric() || v == b'-' || v == b'_' {
                (v as char).to_string()
            } else {
                format!("%{v:02X}")
            }
        })
        .collect()
}
fn decode_hex(value: &str) -> Result<Vec<u8>, TtsError> {
    let clean: String = value
        .trim_start_matches("0x")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if clean.len() % 2 != 0 || !clean.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(upstream("MiniMax 返回了无效的十六进制音频"));
    }
    (0..clean.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).map_err(|_| upstream("无效音频")))
        .collect()
}

pub async fn add_voice(
    state: web::Data<SharedState>,
    input: web::Json<TtsRequest>,
) -> Result<HttpResponse, TtsError> {
    let c = Context::new(&state, &input).await?;
    if c.name != "ElevenLabs" {
        return Err(bad("此入口仅适用于 ElevenLabs 音色上传"));
    }
    if input.name.trim().is_empty() || input.files.is_empty() || input.files.len() > 10 {
        return Err(bad("请填写音色名称并选择 1–10 个音频文件"));
    }
    let mut form = reqwest::multipart::Form::new()
        .text("name", input.name.clone())
        .text("description", input.description.clone());
    for (index, file) in input.files.iter().enumerate() {
        let (head, data) = file.split_once(",").ok_or_else(|| bad("无效的音频文件"))?;
        let mime = head
            .strip_prefix("data:")
            .and_then(|s| s.strip_suffix(";base64"))
            .filter(|s| s.starts_with("audio/"))
            .ok_or_else(|| bad("请选择音频文件"))?;
        let data = STANDARD.decode(data).map_err(|_| bad("无效的音频文件"))?;
        let extension = if mime.contains("wav") {
            "wav"
        } else if mime.contains("mpeg") || mime.contains("mp3") {
            "mp3"
        } else {
            "ogg"
        };
        let part = reqwest::multipart::Part::bytes(data)
            .file_name(format!("sample-{index}.{extension}"))
            .mime_str(mime)
            .map_err(|_| bad("无效的音频类型"))?;
        form = form.part("files", part);
    }
    let data = json_response(c.post("/voices/add")?.multipart(form)).await?;
    Ok(HttpResponse::Ok().json(json!({"voice_id":data["voice_id"]})))
}

pub async fn manage(
    state: web::Data<SharedState>,
    input: web::Json<TtsRequest>,
) -> Result<HttpResponse, TtsError> {
    let c = Context::new(&state, &input).await?;
    let model = input.model_id.trim();
    if matches!(
        input.action.as_str(),
        "reload" | "check-model" | "install-model" | "repair-model"
    ) && (model.is_empty() || model.len() > 256)
    {
        return Err(bad("请填写有效的模型 ID"));
    }
    let data = match (c.name.as_str(), input.action.as_str()) {
        ("AllTalk", "status") => {
            let settings = json_response(c.get("/api/currentsettings")?).await?;
            pick(
                &settings,
                &[
                    "models_available",
                    "current_model_loaded",
                    "engines_available",
                    "current_engine_loaded",
                    "deepspeed_capable",
                    "deepspeed_available",
                    "deepspeed_enabled",
                    "lowvram_capable",
                    "lowvram_enabled",
                ],
            )
        }
        ("AllTalk", "rvc-voices") => {
            let data = json_response(c.get("/api/rvcvoices")?).await?;
            json!({"rvcvoices": data["rvcvoices"]})
        }
        ("AllTalk", "reload") => {
            json_response(c.post("/api/reload")?.query(&[("tts_method", model)])).await?
        }
        ("AllTalk", "deepspeed" | "low-vram") => {
            let (path, param) = if input.action == "deepspeed" {
                ("/api/deepspeed", "new_deepspeed_value")
            } else {
                ("/api/lowvramsetting", "new_low_vram_value")
            };
            json_response(
                c.post(path)?
                    .query(&[(param, if input.value { "True" } else { "False" })]),
            )
            .await?
        }
        ("Coqui", "local-models") => {
            let data = json_response(
                c.post("/api/text-to-speech/coqui/local/get-models")?
                    .json(&json!({"model_id":"model_id", "action":"action"})),
            )
            .await?;
            json!({"models_list": data["models_list"]})
        }
        ("Coqui", "check-model") => {
            let data = json_response(
                c.post("/api/text-to-speech/coqui/coqui-api/check-model-state")?
                    .json(&json!({"model_id": model})),
            )
            .await?;
            json!({"model_state": data["model_state"]})
        }
        ("Coqui", "install-model" | "repair-model") => {
            let action = if input.action == "repair-model" {
                "repare"
            } else {
                "download"
            };
            let data = json_response(
                c.post("/api/text-to-speech/coqui/coqui-api/install-model")?
                    .json(&json!({"model_id": model, "action": action})),
            )
            .await?;
            json!({"status": data["status"]})
        }
        _ => return Err(bad("此服务商不支持该管理操作")),
    };
    Ok(HttpResponse::Ok().json(data))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binary_formats_and_validation() {
        let pcm = vec![0, 0, 1, 0];
        let data = wav(pcm.clone(), 24000, false);
        assert_eq!(&data[0..4], b"RIFF");
        assert_eq!(&data[44..], &pcm);
        assert_eq!(u32::from_le_bytes(data[4..8].try_into().unwrap()), 40);
        assert_eq!(decode_hex("0x00000100").unwrap(), pcm);
        assert!(decode_hex("xyz").is_err());
        assert!(decode_hex("abc").is_err());
        assert!(http_url("file:///etc/passwd").is_err());
        assert!(http_url("http://secret@example.com").is_err());
        assert!(http_url("http://localhost:9880").is_ok());
        assert_eq!(escaped_xml("<&'\""), "&lt;&amp;&apos;&quot;");
    }
    #[test]
    fn streaming_wav_lengths_are_finalized() {
        let mut data = wav(vec![0; 48], 24000, false);
        data[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        data[40..44].fill(0);
        finalize_wav(&mut data);
        assert_eq!(u32::from_le_bytes(data[4..8].try_into().unwrap()), 84);
        assert_eq!(u32::from_le_bytes(data[40..44].try_into().unwrap()), 48);
        let mut other = b"not a wav".to_vec();
        finalize_wav(&mut other);
        assert_eq!(other, b"not a wav");
    }
    #[test]
    fn catalog_and_voice_contracts() {
        assert_eq!(CATALOG.len(), 28);
        let names: std::collections::HashSet<_> = CATALOG
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert_eq!(names.len(), 28);
        assert_eq!(
            normalize_voices(&json!([{"ShortName":"zh-CN-XiaoxiaoNeural","Locale":"zh-CN"}]))[0]["voice_id"],
            "zh-CN-XiaoxiaoNeural"
        );
        assert_eq!(
            normalize_voices(&json!({"voices":[{"value":"ref.wav","label":"Sample"}]}))[0]["name"],
            "Sample"
        );
    }
}
