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
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Explicit mapping between ST's independent-book wire names and card extensions.
const BOOK_EXTENSION_KEYS: &[(&str, &str)] = &[
    ("position", "position"), ("excludeRecursion", "exclude_recursion"),
    ("preventRecursion", "prevent_recursion"), ("delayUntilRecursion", "delay_until_recursion"),
    ("displayIndex", "display_index"), ("probability", "probability"),
    ("useProbability", "useProbability"), ("depth", "depth"),
    ("selectiveLogic", "selectiveLogic"), ("outletName", "outlet_name"),
    ("group", "group"), ("groupOverride", "group_override"), ("groupWeight", "group_weight"),
    ("scanDepth", "scan_depth"), ("caseSensitive", "case_sensitive"),
    ("matchWholeWords", "match_whole_words"), ("useGroupScoring", "use_group_scoring"),
    ("automationId", "automation_id"), ("role", "role"), ("vectorized", "vectorized"),
    ("sticky", "sticky"), ("cooldown", "cooldown"), ("delay", "delay"),
    ("matchPersonaDescription", "match_persona_description"),
    ("matchCharacterDescription", "match_character_description"),
    ("matchCharacterPersonality", "match_character_personality"),
    ("matchCharacterDepthPrompt", "match_character_depth_prompt"),
    ("matchScenario", "match_scenario"), ("matchCreatorNotes", "match_creator_notes"),
    ("triggers", "triggers"), ("ignoreBudget", "ignore_budget"),
];

impl WorldInfoBook {
    /// Canonical ST keys win when legacy nast and ST names coexist.
    pub fn from_json(mut value: Value) -> Result<Self, serde_json::Error> {
        if let Some(entries) = value.get_mut("entries").and_then(Value::as_object_mut) {
            for entry in entries.values_mut().filter_map(Value::as_object_mut) {
                for (canonical, legacy) in BOOK_EXTENSION_KEYS.iter().copied().chain([
                    ("useProbability", "use_probability"), ("selectiveLogic", "selective_logic"), ("addMemo", "add_memo"),
                    ("character_filter", "characterFilter")]) {
                    if canonical != legacy {
                        if let Some(value) = entry.remove(legacy) { entry.entry(canonical).or_insert(value); }
                    }
                }
            }
        }
        serde_json::from_value(value)
    }

    /// ST convertCharacterBook, retaining the original card data for later export.
    pub fn from_embedded(book: &crate::card::CharacterBook) -> crate::ModelResult<Self> {
        let original = serde_json::to_value(book)?;
        let mut result = Self::default();
        result.extra.insert("originalData".into(), original.clone());
        for (index, entry) in original["entries"].as_array().into_iter().flatten().enumerate() {
            let uid = entry["id"].as_i64().unwrap_or(index as i64);
            if result.entries.contains_key(&uid.to_string()) {
                return Err(crate::ModelError::InvalidValue {
                    field: "character_book.entries.id", reason: format!("duplicate entry id {uid}"),
                });
            }
            let mut wire = serde_json::json!({
                "uid": uid, "key": entry["keys"], "keysecondary": entry["secondary_keys"],
                "content": entry["content"], "comment": entry["comment"].as_str().unwrap_or(""),
                "constant": entry["constant"], "selective": entry["selective"],
                "order": entry["insertion_order"], "disable": !entry["enabled"].as_bool().unwrap_or(false),
                "addMemo": !entry["comment"].as_str().unwrap_or("").is_empty(),
                "position": if entry["position"].as_str() == Some("before_char") { 0 } else { 1 },
                "displayIndex": index,
                "extensions": entry["extensions"],
            });
            for (world_key, card_key) in BOOK_EXTENSION_KEYS {
                if let Some(value) = entry["extensions"].get(*card_key).filter(|v| !v.is_null()) {
                    wire[*world_key] = value.clone();
                }
            }
            result.entries.insert(uid.to_string(), serde_json::from_value(wire)?);
        }
        Ok(result)
    }

    /// Rebuild current entries while retaining unknown book/entry extension data.
    pub fn to_embedded(&self, name: &str) -> crate::ModelResult<crate::card::CharacterBook> {
        let mut original = self.extra.get("originalData").filter(|v| v.is_object())
            .cloned().unwrap_or_else(|| serde_json::json!({"name": name}));
        let previous = original["entries"].as_array().cloned().unwrap_or_default();
        let mut entries = Vec::new();
        for entry in self.entries.values() {
            let wire = serde_json::to_value(entry)?;
            let mut card = previous.iter().find(|e| e["id"].as_i64() == Some(entry.uid))
                .cloned().unwrap_or_else(|| serde_json::json!({}));
            for (key, value) in [
                ("id", serde_json::json!(entry.uid)), ("keys", wire["key"].clone()),
                ("secondary_keys", wire["keysecondary"].clone()), ("content", wire["content"].clone()),
                ("comment", wire["comment"].clone()), ("constant", wire["constant"].clone()),
                ("selective", wire["selective"].clone()), ("insertion_order", wire["order"].clone()),
                ("enabled", serde_json::json!(!entry.disable)), ("use_regex", Value::Bool(true)),
                ("position", serde_json::json!(if entry.position.resolve().0 == 0 { "before_char" } else { "after_char" })),
            ] { card[key] = value; }
            if !card["extensions"].is_object() { card["extensions"] = serde_json::json!({}); }
            if let Some(extra) = wire.get("extensions").and_then(Value::as_object) {
                card["extensions"].as_object_mut().unwrap().extend(extra.clone());
            }
            for (world_key, card_key) in BOOK_EXTENSION_KEYS {
                if let Some(value) = wire.get(*world_key) { card["extensions"][*card_key] = value.clone(); }
            }
            entries.push(card);
        }
        original["entries"] = Value::Array(entries);
        Ok(serde_json::from_value(original)?)
    }
}

/// 一个条目；字段与默认值逐项对齐 newWorldInfoEntryDefinition。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WIEntry {
    /// Runtime identity captured before decorators and macros alter content.
    #[serde(skip)]
    pub scan_hash: Option<i64>,
    pub uid: i64,
    pub key: Vec<String>,
    pub keysecondary: Vec<String>,
    pub comment: String,
    pub content: String,
    pub constant: bool,
    pub vectorized: bool,
    pub selective: bool,
    #[serde(rename = "selectiveLogic", alias = "selective_logic")]
    pub selective_logic: i64,
    #[serde(rename = "addMemo", alias = "add_memo")]
    pub add_memo: bool,
    pub order: i64,
    /// 数字 0-7，或字符串 "@D3" / "@D2[a]" / "@D2[r]"
    pub position: WIPosition,
    #[serde(rename = "excludeRecursion", alias = "exclude_recursion")]
    pub exclude_recursion: bool,
    #[serde(rename = "preventRecursion", alias = "prevent_recursion")]
    pub prevent_recursion: bool,
    /// false / true(=1) / N：递归层级 N 起才激活（ST delayUntilRecursion）
    #[serde(rename = "delayUntilRecursion", alias = "delay_until_recursion")]
    #[serde(deserialize_with = "crate::compat::null_default")]
    pub delay_until_recursion: BoolOrNum,
    pub disable: bool,
    pub probability: i64,
    #[serde(rename = "useProbability", alias = "use_probability")]
    pub use_probability: bool,
    pub depth: i64,
    pub group: String,
    #[serde(rename = "groupOverride", alias = "group_override")]
    pub group_override: bool,
    #[serde(rename = "groupWeight", alias = "group_weight")]
    pub group_weight: i64,
    /// null = 用全局设置
    #[serde(rename = "scanDepth", alias = "scan_depth")]
    pub scan_depth: Option<i64>,
    #[serde(rename = "caseSensitive", alias = "case_sensitive")]
    pub case_sensitive: Option<bool>,
    #[serde(rename = "matchWholeWords", alias = "match_whole_words")]
    pub match_whole_words: Option<bool>,
    #[serde(rename = "useGroupScoring", alias = "use_group_scoring")]
    pub use_group_scoring: Option<bool>,
    #[serde(rename = "automationId", alias = "automation_id")]
    pub automation_id: String,
    /// null / 0 / 1 / 2 = system/user/assistant
    pub role: Option<i64>,
    /// timed effects
    #[serde(deserialize_with = "crate::compat::null_default")]
    pub sticky: i64,
    #[serde(deserialize_with = "crate::compat::null_default")]
    pub cooldown: i64,
    #[serde(deserialize_with = "crate::compat::null_default")]
    pub delay: i64,
    #[serde(rename = "matchPersonaDescription", alias = "match_persona_description")]
    pub match_persona_description: bool,
    #[serde(rename = "matchCharacterDescription", alias = "match_character_description")]
    pub match_character_description: bool,
    #[serde(rename = "matchCharacterPersonality", alias = "match_character_personality")]
    pub match_character_personality: bool,
    #[serde(rename = "matchCharacterDepthPrompt", alias = "match_character_depth_prompt")]
    pub match_character_depth_prompt: bool,
    #[serde(rename = "matchScenario", alias = "match_scenario")]
    pub match_scenario: bool,
    #[serde(rename = "matchCreatorNotes", alias = "match_creator_notes")]
    pub match_creator_notes: bool,
    #[serde(rename = "ignoreBudget", alias = "ignore_budget")]
    pub ignore_budget: bool,
    /// position=7 时的 outlet 名（{{outlet::name}} 宏消费）
    #[serde(rename = "outletName", skip_serializing_if = "Option::is_none")]
    pub outlet_name: Option<String>,
    /// 角色过滤（持久化键 character_filter；内部 camelCase）
    #[serde(rename = "character_filter", alias = "characterFilter", skip_serializing_if = "Option::is_none")]
    pub character_filter: Option<CharacterFilter>,
    /// 运行时附加：所属书名 / UI 顺序
    pub world: Option<String>,
    #[serde(rename = "displayIndex", alias = "display_index")]
    pub display_index: i64,
    /// 外部扩展透传
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Default for WIEntry {
    fn default() -> Self {
        Self {
            scan_hash: None,
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

impl Default for BoolOrNum {
    fn default() -> Self { Self::Bool(false) }
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
