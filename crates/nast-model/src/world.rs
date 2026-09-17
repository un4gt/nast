//! 世界书（lorebook）独立文件格式：worlds/<name>.json = { entries: { "<uid>": entry } }。
//! 对齐 refrence/SillyTavern/public/scripts/world-info.js newWorldInfoEntryDefinition。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// world_info_position（world-info.js 枚举，数值必须一致）。
pub const WI_POS_BEFORE: i64 = 0;
pub const WI_POS_AFTER: i64 = 1;
pub const WI_POS_ANTOP: i64 = 2;
pub const WI_POS_ANBOTTOM: i64 = 3;
pub const WI_POS_AT_DEPTH: i64 = 4;
pub const WI_POS_EM_TOP: i64 = 5;
pub const WI_POS_EM_BOTTOM: i64 = 6;
pub const WI_POS_OUTLET: i64 = 7;

/// world_info_logic
pub const WI_LOGIC_AND_ANY: i64 = 0;
pub const WI_LOGIC_NOT_ALL: i64 = 1;
pub const WI_LOGIC_NOT_ANY: i64 = 2;
pub const WI_LOGIC_AND_ALL: i64 = 3;

pub const WI_DEFAULT_DEPTH: i64 = 4;
pub const WI_DEFAULT_ORDER: i64 = 100;
pub const WI_MAX_SCAN_DEPTH: i64 = 1000;
pub const WI_MAX_COMMENT_LENGTH: usize = 100;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorldInfoBook {
    pub entries: BTreeMap<String, WIEntry>,
}

/// 一个条目；字段与默认值逐项对齐 newWorldInfoEntryDefinition。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WIEntry {
    pub uid: i64,
    pub key: Vec<String>,
    pub keysecondary: Vec<String>,
    pub comment: String,
    pub content: String,
    pub constant: bool,
    pub vectorized: bool,
    pub selective: bool,
    pub selective_logic: i64,
    pub add_memo: bool,
    pub order: i64,
    /// 数字 0-7，或字符串 "@D3" / "@D2[a]" / "@D2[r]"
    pub position: WIPosition,
    pub exclude_recursion: bool,
    pub prevent_recursion: bool,
    /// false / true(=1) / N：递归层级 N 起才激活（ST delayUntilRecursion）
    pub delay_until_recursion: BoolOrNum,
    pub disable: bool,
    pub probability: i64,
    pub use_probability: bool,
    pub depth: i64,
    pub group: String,
    pub group_override: bool,
    pub group_weight: i64,
    /// null = 用全局设置
    pub scan_depth: Option<i64>,
    pub case_sensitive: Option<bool>,
    pub match_whole_words: Option<bool>,
    pub use_group_scoring: Option<bool>,
    pub automation_id: String,
    /// null / 0 / 1 / 2 = system/user/assistant
    pub role: Option<i64>,
    /// timed effects
    pub sticky: i64,
    pub cooldown: i64,
    pub delay: i64,
    pub match_persona_description: bool,
    pub match_character_description: bool,
    pub match_character_personality: bool,
    pub match_character_depth_prompt: bool,
    pub match_scenario: bool,
    pub match_creator_notes: bool,
    pub ignore_budget: bool,
    /// position=7 时的 outlet 名（{{outlet::name}} 宏消费）
    #[serde(rename = "outletName", skip_serializing_if = "Option::is_none")]
    pub outlet_name: Option<String>,
    /// 角色过滤（持久化键 character_filter；内部 camelCase）
    #[serde(rename = "character_filter", skip_serializing_if = "Option::is_none")]
    pub character_filter: Option<CharacterFilter>,
    /// 运行时附加：所属书名 / UI 顺序
    pub world: Option<String>,
    pub display_index: i64,
    /// 外部扩展透传
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Default for WIEntry {
    fn default() -> Self {
        Self {
            uid: 0,
            key: vec![],
            keysecondary: vec![],
            comment: String::new(),
            content: String::new(),
            constant: false,
            vectorized: false,
            selective: true,
            selective_logic: WI_LOGIC_AND_ANY,
            add_memo: true,
            order: WI_DEFAULT_ORDER,
            position: WIPosition::Num(WI_POS_BEFORE),
            exclude_recursion: false,
            prevent_recursion: false,
            delay_until_recursion: BoolOrNum::Bool(false),
            disable: false,
            probability: 100,
            use_probability: true,
            depth: WI_DEFAULT_DEPTH,
            group: String::new(),
            group_override: false,
            group_weight: 100,
            scan_depth: None,
            case_sensitive: None,
            match_whole_words: None,
            use_group_scoring: None,
            automation_id: String::new(),
            role: None,
            sticky: 0,
            cooldown: 0,
            delay: 0,
            match_persona_description: false,
            match_character_description: false,
            match_character_personality: false,
            match_character_depth_prompt: false,
            match_scenario: false,
            match_creator_notes: false,
            ignore_budget: false,
            outlet_name: None,
            character_filter: None,
            world: None,
            display_index: 0,
            extra: BTreeMap::new(),
        }
    }
}

/// position 可能是数字或 "@D.." 字符串（GOTCHA #7）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WIPosition {
    Num(i64),
    Text(String),
}

/// bool 或数字双型（delayUntilRecursion 等）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BoolOrNum {
    Bool(bool),
    Num(i64),
}

impl BoolOrNum {
    /// false=0；true=1；N=N。
    pub fn level(&self) -> i64 {
        match self {
            BoolOrNum::Bool(b) => *b as i64,
            BoolOrNum::Num(n) => *n,
        }
    }
}

/// 条目角色过滤（ST character_filter：{isExclude, names[], tags[]}）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CharacterFilter {
    pub is_exclude: bool,
    pub names: Vec<String>,
    pub tags: Vec<String>,
}

impl WIPosition {
    /// 解析出数值 position 与 @D 覆盖 (depth, role_override)。
    /// "@D3" → (at_depth, Some(3, None))；"@D2[a]" → (at_depth, Some(2, Some(2=assistant)))；
    /// "@D2[r]" → (at_depth, Some(2, Some(1=user)))。非 @D 字符串按 num 失败处理回 0。
    pub fn resolve(&self) -> (i64, Option<(i64, Option<i64>)>) {
        match self {
            WIPosition::Num(n) => (*n, None),
            WIPosition::Text(s) => {
                let t = s.trim();
                if let Some(rest) = t.strip_prefix("@D").or_else(|| t.strip_prefix("@d")) {
                    let (depth_s, role) = match rest.split_once('[') {
                        Some((d, r)) => (d, r.trim_end_matches(']').to_string()),
                        None => (rest, String::new()),
                    };
                    let depth = depth_s.trim().parse::<i64>().unwrap_or(WI_DEFAULT_DEPTH);
                    let role = match role.as_str() {
                        "a" => Some(2),
                        "r" | "u" => Some(1),
                        "s" => Some(0),
                        _ => None,
                    };
                    (WI_POS_AT_DEPTH, Some((depth, role)))
                } else {
                    (WI_POS_BEFORE, None)
                }
            }
        }
    }
}

/// timed effects 记录（chat_metadata.timedWorldInfo[type]["world.uid"]）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimedEffect {
    /// getStringHash(JSON.stringify(entry))
    pub hash: i64,
    /// 开始时的 chat.length
    pub start: i64,
    /// 结束的 chat.length 水平线
    pub end: i64,
    /// sticky 到期保护（swipe/regen 不推进历史时效果保留）
    pub protected: bool,
}

/// chat_metadata.timedWorldInfo
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TimedWorldInfo {
    #[serde(default)]
    pub sticky: BTreeMap<String, TimedEffect>,
    #[serde(default)]
    pub cooldown: BTreeMap<String, TimedEffect>,
}
