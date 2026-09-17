//! 聊天消息与聊天文件格式。
//! 对齐 refrence/SillyTavern/src/endpoints/chats.js 的 jsonl 布局与
//! script.js saveReply/sendMessageAsUser 的消息对象形状（1.18.0）。

use crate::regex_script::RegexScript;
use crate::world::TimedWorldInfo;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// jsonl 首行 header。现代 ST 文件 user_name/character_name 均为 'unused'。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatHeader {
    #[serde(default = "unused")]
    pub user_name: String,
    #[serde(default = "unused")]
    pub character_name: String,
    pub chat_metadata: ChatMetadata,
}

fn unused() -> String {
    "unused".into()
}

/// chat_metadata（chats.js + 各扩展归属字段）。None 字段不落盘（ST 同为按需写入）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChatMetadata {
    /// 并发写保护 slug；保存时校验，mismatch 除非 force 否则拒绝
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrity: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub tainted: bool,
    /// 聊天级世界书名
    #[serde(skip_serializing_if = "Option::is_none")]
    pub world: Option<String>,
    /// Author's Note
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_interval: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_position: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_depth: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_role: Option<i64>,
    /// 用户 @depth 提示
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth_prompt: Option<crate::card::DepthPrompt>,
    /// WI timed effects
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timed_world_info: Option<TimedWorldInfo>,
    /// 聊天作用域正则脚本
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regex_scripts: Vec<RegexScript>,
    /// 本地变量（{{setvar}}/{{getvar}} 存储，即时持久化）
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variables: BTreeMap<String, Value>,
    /// 群聊覆盖字段
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scenario: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mes_example: Option<String>,
    /// 扩展自由存储
    #[serde(flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, Value>,
}

/// 一条消息。字段为 1.18.0 实测形状：
/// 推理在 extra.reasoning{,_duration,_type,_signature}（无 isThinking）；
/// token_count 仅在开启计数时存在；用户消息无 swipes。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChatMessage {
    pub name: String,
    pub is_user: bool,
    pub is_system: bool,
    /// ISO 8601（getMessageTimeStamp = toISOString()）
    pub send_date: String,
    pub mes: String,
    pub extra: MessageExtra,
    /// swipes[swipe_id] 与 mes 平行；swipe_info 与 swipes 平行
    pub swipes: Option<Vec<String>>,
    pub swipe_id: Option<i64>,
    pub swipe_info: Option<Vec<SwipeInfo>>,
    /// 群聊消息：头像锁定与身份
    pub force_avatar: Option<String>,
    pub original_avatar: Option<String>,
    /// ST 写在消息顶层的生成时间戳（script.js:6617-6618）；extra 内另有同名副本
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gen_started: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gen_finished: Option<String>,
    /// 消息标题（title 扩展）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// swipe 生成期间：当条 swipe 正在流式
    pub is_streaming: Option<bool>,
}

/// extra 对象（无 schema 自由扩展；这里列出已知键）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MessageExtra {
    pub api: Option<String>,
    pub model: Option<String>,
    /// (reasoning||'') + mes 的计数（开启时才有）
    pub token_count: Option<i64>,
    /// 渲染替换文本
    pub display_text: Option<String>,
    /// 推理
    pub reasoning: Option<String>,
    pub reasoning_duration: Option<f64>,
    pub reasoning_type: Option<String>,
    pub reasoning_signature: Option<String>,
    /// 群聊批次 id
    pub gen_id: Option<Value>,
    pub gen_started: Option<String>,
    pub gen_finished: Option<String>,
    /// 时间戳字符串（saveReply 顶层也有 gen_started/gen_finished）
    /// 附加类型: 'narrator' | 'comment' | 'nointer' ...
    #[serde(rename = "type")]
    pub kind: Option<String>,
    #[serde(rename = "isSmallSys")]
    pub is_small_sys: Option<bool>,
    /// 用户消息 prompt bias
    pub bias: Option<String>,
    pub time_to_first_token: Option<f64>,
    /// 附件/图片
    pub image: Option<Value>,
    pub inline_image: Option<Value>,
    pub media: Option<Value>,
    pub append_title: Option<String>,
    pub title: Option<String>,
    /// 外部扩展自由字段
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SwipeInfo {
    pub send_date: Option<String>,
    pub gen_started: Option<String>,
    pub gen_finished: Option<String>,
    pub extra: MessageExtra,
}

/// 整个聊天文件（header + messages）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChatFile(pub Vec<serde_json::Value>);

impl ChatFile {
    pub fn header(&self) -> Option<&serde_json::Value> {
        self.0.first()
    }

    /// 解析 header；兼容旧文件首行含 user_name/name/chat_metadata 任一。
    pub fn metadata(&self) -> ChatMetadata {
        self.0
            .first()
            .and_then(|v| v.get("chat_metadata"))
            .map(|m| serde_json::from_value(m.clone()).unwrap_or_default())
            .unwrap_or_default()
    }

    pub fn messages(&self) -> impl Iterator<Item = &serde_json::Value> {
        self.0.iter().skip(1)
    }
}

/// 生成类型（1.18.0 字符串，constants.js GENERATION_TYPE_TRIGGERS）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationType {
    Normal,
    Continue,
    Impersonate,
    Swipe,
    Regenerate,
    Quiet,
}

impl GenerationType {
    pub fn as_str(&self) -> &'static str {
        match self {
            GenerationType::Normal => "normal",
            GenerationType::Continue => "continue",
            GenerationType::Impersonate => "impersonate",
            GenerationType::Swipe => "swipe",
            GenerationType::Regenerate => "regenerate",
            GenerationType::Quiet => "quiet",
        }
    }
}
