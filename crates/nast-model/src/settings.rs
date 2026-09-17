//! settings.json blob（服务端原样存取客户端整个对象；这里给出拼装引擎读取的强类型切片）。
//! 对齐 refrence/SillyTavern/public/scripts/world-info.js 全局默认与 power-user.js。

use crate::persona::Personas;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// WI 全局默认值（world-info.js 顶部 globals）。
pub const WI_DEPTH_DEFAULT: i64 = 2;
pub const WI_MIN_ACTIVATIONS_DEFAULT: i64 = 0;
pub const WI_MIN_ACTIVATIONS_DEPTH_MAX_DEFAULT: i64 = 0;
pub const WI_BUDGET_DEFAULT: i64 = 25;
pub const WI_INCLUDE_NAMES_DEFAULT: bool = true;
pub const WI_RECURSIVE_DEFAULT: bool = false;
pub const WI_CASE_SENSITIVE_DEFAULT: bool = false;
pub const WI_MATCH_WHOLE_WORDS_DEFAULT: bool = false;
pub const WI_USE_GROUP_SCORING_DEFAULT: bool = false;
pub const WI_CHARACTER_STRATEGY_DEFAULT: i64 = 1; // character_first
pub const WI_BUDGET_CAP_DEFAULT: i64 = 0;
pub const WI_MAX_RECURSION_STEPS_DEFAULT: i64 = 0;

/// world_info_insertion_strategy
pub const WIS_EVENLY: i64 = 0;
pub const WIS_CHARACTER_FIRST: i64 = 1;
pub const WIS_GLOBAL_FIRST: i64 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorldInfoSettings {
    pub world_info_depth: i64,
    pub world_info_min_activations: i64,
    pub world_info_min_activations_depth_max: i64,
    pub world_info_budget: i64,
    pub world_info_include_names: bool,
    pub world_info_recursive: bool,
    pub world_info_case_sensitive: bool,
    pub world_info_match_whole_words: bool,
    pub world_info_use_group_scoring: bool,
    pub world_info_character_strategy: i64,
    pub world_info_budget_cap: i64,
    pub world_info_max_recursion_steps: BoolOrI64,
    /// globalSelect: 全局选中的书
    pub global_select: Vec<String>,
    /// charLore: 角色辅助书
    pub char_lore: Vec<CharLore>,
    pub world_info_overflow_alert: bool,
}

impl Default for WorldInfoSettings {
    fn default() -> Self {
        Self {
            world_info_depth: WI_DEPTH_DEFAULT,
            world_info_min_activations: WI_MIN_ACTIVATIONS_DEFAULT,
            world_info_min_activations_depth_max: WI_MIN_ACTIVATIONS_DEPTH_MAX_DEFAULT,
            world_info_budget: WI_BUDGET_DEFAULT,
            world_info_include_names: WI_INCLUDE_NAMES_DEFAULT,
            world_info_recursive: WI_RECURSIVE_DEFAULT,
            world_info_case_sensitive: WI_CASE_SENSITIVE_DEFAULT,
            world_info_match_whole_words: WI_MATCH_WHOLE_WORDS_DEFAULT,
            world_info_use_group_scoring: WI_USE_GROUP_SCORING_DEFAULT,
            world_info_character_strategy: WI_CHARACTER_STRATEGY_DEFAULT,
            world_info_budget_cap: WI_BUDGET_CAP_DEFAULT,
            // release 里是数值；宽容解析
            world_info_max_recursion_steps: BoolOrI64::I64(WI_MAX_RECURSION_STEPS_DEFAULT),
            global_select: vec![],
            char_lore: vec![],
            world_info_overflow_alert: false,
        }
    }
}

/// 兼容 bool/number 双型的字段。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BoolOrI64 {
    Bool(bool),
    I64(i64),
}

impl BoolOrI64 {
    pub fn as_i64(&self) -> i64 {
        match self {
            BoolOrI64::Bool(b) => *b as i64,
            BoolOrI64::I64(n) => *n,
        }
    }
}

/// @seriesss charLore 条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharLore {
    pub name: String,
    pub characters: Vec<String>,
}

/// power_user 中拼装引擎需要的切片（其余字段走 blob 透传）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PowerUser {
    pub username: String,
    /// extension_prompt_types
    pub persona_description_position: i64,
    pub persona_description: String,
    pub persona_description_depth: i64,
    pub persona_description_role: i64,
    pub persona_description_lorebook: Option<String>,
    pub context_size: i64,
    pub message_token_count_enabled: bool,
    /// AN 默认档
    pub note_default: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// 客户端 settings blob 的强类型视图 + 原样透传。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ClientSettings {
    pub first_run: bool,
    pub power_user: PowerUser,
    pub oai_settings: crate::preset::OaiSettings,
    pub world_info: WorldInfoSettings,
    pub extension_settings: BTreeMap<String, Value>,
    pub personas: Option<Personas>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
