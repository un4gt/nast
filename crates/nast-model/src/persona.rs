//! persona（refrence/SillyTavern/public/scripts/personas.js + power_user）。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// persona_description_position
pub const PDP_IN_PROMPT: i64 = 0;
pub const PDP_TOP_AN: i64 = 1;
pub const PDP_BOTTOM_AN: i64 = 2;
pub const PDP_AT_DEPTH: i64 = 3;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PersonaDescription {
    pub description: String,
    /// PDP_* 或 extension_prompt_types（IN_PROMPT=0/IN_CHAT=1/BEFORE_PROMPT=2 语义由 personas.js 统一）
    pub position: i64,
    pub depth: i64,
    /// system/user/assistant（0/1/2）
    pub role: i64,
}

/// power_user.personas / persona_descriptions 的集合形态。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Personas {
    /// persona 名 → 头像文件名
    pub personas: BTreeMap<String, String>,
    pub persona_descriptions: BTreeMap<String, PersonaDescription>,
    pub default_persona: Option<String>,
    pub persona_description_position: i64,
    /// persona 绑定的世界书
    pub persona_description_lorebook: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
