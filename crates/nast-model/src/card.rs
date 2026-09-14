//! 角色卡类型：V1 / V2 (chara_card_v2) / V3 (chara_card_v3)。
//!
//! 对齐 refrence/SillyTavern/public/scripts/char-data.js 的 typedef 与
//! src/endpoints/characters.js 的 charaFormatData 行为：
//! ST 内存模型始终是「顶层 V1 字段 + data: V2 字段」并存的结构，
//! 导入 V1 卡时由服务端合成 data 与 data.extensions。

use crate::regex_script::RegexScript;
use crate::{ModelError, ModelResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const SPEC_V2: &str = "chara_card_v2";
pub const SPEC_V3: &str = "chara_card_v3";

/// 角色（ST 内存形态）：顶层字段 + data 双份。
/// `chat`/`avatar`/`json_data`/`create_date`/`shallow` 为服务端附加字段
/// （characters.js：avatar 文件名即身份）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Character {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub personality: String,
    #[serde(default)]
    pub scenario: String,
    #[serde(default)]
    pub first_mes: String,
    #[serde(default)]
    pub mes_example: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creatorcomment: Option<String>,
    #[serde(default)]
    pub creator_notes: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// ST 存字符串（V1 遗留），导入时 String(talkativeness)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub talkativeness: Option<String>,
    #[serde(default)]
    pub fav: bool,
    #[serde(default)]
    pub fav_checked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat: Option<String>,
    /// avatar PNG 文件名（身份标识，聊天目录以此命名）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_data: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shallow: Option<Value>,
    #[serde(default)]
    pub data: V2CharData,
}

/// V2/V3 data 对象（char-data.js v2CharData）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct V2CharData {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub character_version: String,
    #[serde(default)]
    pub personality: String,
    #[serde(default)]
    pub scenario: String,
    #[serde(default)]
    pub first_mes: String,
    #[serde(default)]
    pub mes_example: String,
    #[serde(default)]
    pub creator_notes: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub post_history_instructions: String,
    #[serde(default)]
    pub alternate_greetings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character_book: Option<CharacterBook>,
    /// V3: group_only_greetings 必填（可空数组）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_only_greetings: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets: Option<Vec<CardAsset>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creator_notes_multilingual: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creation_date: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modification_date: Option<i64>,
    #[serde(default)]
    pub extensions: CharExtensions,
    /// 容忍外部扩展未知字段
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// data.extensions（ST 关心的键 + 透传外部键）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CharExtensions {
    /// ST 里是 string 型数字
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub talkativeness: Option<String>,
    #[serde(default)]
    pub fav: bool,
    /// 主世界书名
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth_prompt: Option<DepthPrompt>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regex_scripts: Vec<RegexScript>,
    /// V3 外部键：chub / risuai / sd_character_prompt / github_repo ...
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// @depth 注入：depth = 距末尾消息数，role = system/user/assistant。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DepthPrompt {
    #[serde(default)]
    pub prompt: String,
    #[serde(default = "default_depth")]
    pub depth: i64,
    #[serde(default = "default_role_system")]
    pub role: String,
}

fn default_depth() -> i64 {
    4
}
fn default_role_system() -> String {
    "system".into()
}

/// V3 assets
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardAsset {
    #[serde(rename = "type")]
    pub kind: String,
    pub uri: String,
    pub name: String,
    pub ext: String,
}

/// character_book：书体与内嵌 entries（spec v2/v3 + ST 扩展字段）。
/// 导入服务端时 ST 把 entries 从数组转成 {uid: entry} 的 map 存入 worlds/。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CharacterBook {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scan_depth: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<i64>,
    #[serde(default)]
    pub recursive_scanning: bool,
    #[serde(default)]
    pub extensions: BTreeMap<String, Value>,
    #[serde(default)]
    pub entries: Vec<CharacterBookEntry>,
}

/// 卡内嵌条目（spec 字段 + ST extensions 数值字段，见 char-data.js v2DataWorldInfoEntry）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CharacterBookEntry {
    pub keys: Vec<String>,
    #[serde(default)]
    pub secondary_keys: Vec<String>,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub enabled: bool,
    /// spec: insertion_order；ST 默认 100
    #[serde(default = "default_order")]
    pub insertion_order: i64,
    #[serde(default)]
    pub case_sensitive: Option<bool>,
    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default)]
    pub selective: bool,
    #[serde(default)]
    pub constant: bool,
    #[serde(default)]
    pub position: Option<String>, // 'before_char' | 'after_char'
    #[serde(default)]
    pub vectorized: bool,
    #[serde(default)]
    pub use_regex: Option<bool>,
    #[serde(default)]
    pub extensions: BookEntryExtensions,
}

fn default_order() -> i64 {
    100
}

/// ST 数值扩展字段全部放在 entry.extensions（导入时映射为 WIEntry 数值字段）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BookEntryExtensions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<i64>, // world_info_position 0-7
    #[serde(default)]
    pub exclude_recursion: bool,
    #[serde(default)]
    pub prevent_recursion: bool,
    #[serde(default)]
    pub delay_until_recursion: bool,
    /// ST 存 0-100
    #[serde(default = "default_prob")]
    pub probability: i64,
    #[serde(default = "default_true")]
    pub use_probability: bool,
    #[serde(default = "default_depth4")]
    pub depth: i64,
    #[serde(default)]
    pub selective_logic: i64,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub group_override: bool,
    #[serde(default = "default_weight")]
    pub group_weight: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scan_depth: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_sensitive: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_whole_words: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_group_scoring: Option<bool>,
    #[serde(default)]
    pub automation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<i64>,
    #[serde(default)]
    pub vectorized: bool,
    #[serde(default)]
    pub sticky: i64,
    #[serde(default)]
    pub cooldown: i64,
    #[serde(default)]
    pub delay: i64,
    #[serde(default)]
    pub match_persona_description: bool,
    #[serde(default)]
    pub match_character_description: bool,
    #[serde(default)]
    pub match_character_personality: bool,
    #[serde(default)]
    pub match_character_depth_prompt: bool,
    #[serde(default)]
    pub match_scenario: bool,
    #[serde(default)]
    pub match_creator_notes: bool,
    #[serde(default)]
    pub ignore_budget: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn default_prob() -> i64 {
    100
}
fn default_true() -> bool {
    true
}
fn default_depth4() -> i64 {
    4
}
fn default_weight() -> i64 {
    100
}

/// PNG tEXt 卡片载体：chara (V2/V1) / ccv3 (V3)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardChunk {
    /// "chara" | "ccv3"
    pub keyword: String,
    /// base64(UTF-8 JSON)
    pub value_b64: String,
}

impl Character {
    /// V1 顶层 + V2 data 合并的 ST 内存形态（characters.js import 后的形状）。
    ///
    /// 兼容三种输入形态（characters.js charaFormatData 语义）：
    /// - V1：字段全在顶层，无 data
    /// - V2/V3：字段全在 data 下，顶层只有 spec（野生卡常见——顶层 name 可缺失）
    /// - ST 内存形态：顶层 + data 并存
    pub fn from_card_json(v: &Value) -> ModelResult<Self> {
        let obj = v
            .as_object()
            .ok_or(ModelError::InvalidValue { field: "card", reason: "root is not an object".into() })?;
        let spec = obj.get("spec").and_then(|s| s.as_str()).unwrap_or("");
        let has_data = obj.get("data").is_some();

        // 第一阶段：data 对象优先解析（野生 V2/V3 的字段全部在此）
        let mut ch: Character = if has_data {
            // 顶层宽容解析：name 缺失时先置空（后面从 data 回填）
            let mut c: Character = serde_json::from_value(v.clone()).or_else(|_| {
                let mut partial: Value = v.clone();
                partial["name"] = Value::String(String::new());
                serde_json::from_value(partial)
            })?;
            // 顶层字段从 data 合成（ST import 后顶层与 data 并存）
            let d = &c.data;
            if c.name.is_empty() {
                c.name = d.name.clone();
            }
            if c.description.is_empty() && !d.description.is_empty() {
                c.description = d.description.clone();
            }
            if c.personality.is_empty() && !d.personality.is_empty() {
                c.personality = d.personality.clone();
            }
            if c.scenario.is_empty() && !d.scenario.is_empty() {
                c.scenario = d.scenario.clone();
            }
            if c.first_mes.is_empty() && !d.first_mes.is_empty() {
                c.first_mes = d.first_mes.clone();
            }
            if c.mes_example.is_empty() && !d.mes_example.is_empty() {
                c.mes_example = d.mes_example.clone();
            }
            if c.creator_notes.is_empty() && !d.creator_notes.is_empty() {
                c.creator_notes = d.creator_notes.clone();
            }
            if c.tags.is_empty() && !d.tags.is_empty() {
                c.tags = d.tags.clone();
            }
            c
        } else {
            serde_json::from_value(v.clone())?
        };

        // 第二阶段：V1（无 data）→ 从顶层合成 data
        if spec.is_empty() && !has_data {
            ch.data = V2CharData {
                name: ch.name.clone(),
                description: ch.description.clone(),
                personality: ch.personality.clone(),
                scenario: ch.scenario.clone(),
                first_mes: ch.first_mes.clone(),
                mes_example: ch.mes_example.clone(),
                creator_notes: ch.creatorcomment.clone().unwrap_or_default(),
                ..Default::default()
            };
        }
        if ch.data.name.is_empty() {
            ch.data.name = ch.name.clone();
        }
        // V3 group_only_greetings 必填
        if ch.data.group_only_greetings.is_none() {
            ch.data.group_only_greetings = Some(vec![]);
        }
        // extensions 缺省合成（charaFormatData 行为）
        if ch.data.extensions.talkativeness.is_none() {
            ch.data.extensions.talkativeness =
                Some(ch.talkativeness.clone().unwrap_or_else(|| "0.5".into()));
        }
        if ch.data.extensions.depth_prompt.is_none() {
            ch.data.extensions.depth_prompt = Some(DepthPrompt {
                prompt: String::new(),
                depth: crate::world::WI_DEFAULT_DEPTH,
                role: "system".into(),
            });
        }
        Ok(ch)
    }

    pub fn validate(&self) -> ModelResult<()> {
        if self.name.trim().is_empty() && self.data.name.trim().is_empty() {
            return Err(ModelError::MissingField("name"));
        }
        Ok(())
    }
}
