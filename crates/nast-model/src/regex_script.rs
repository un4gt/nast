//! 正则脚本 schema（refrence/SillyTavern/public/scripts/extensions/regex/engine.js）。

use serde::{Deserialize, Serialize};

/// regex_placement 枚举数值（regex-engine.js:281-293，1.18.0 实值）。
/// 0 = MD_DISPLAY（废弃），4 = 遗留 sendAs，不再使用。
pub const RP_MD_DISPLAY: i64 = 0;
pub const RP_USER_INPUT: i64 = 1;
pub const RP_AI_OUTPUT: i64 = 2;
pub const RP_SLASH_COMMAND: i64 = 3;
pub const RP_WORLD_INFO: i64 = 5;
pub const RP_REASONING: i64 = 6;

/// substitute_find_regex
pub const SUB_NONE: i64 = 0;
pub const SUB_RAW: i64 = 1;
pub const SUB_ESCAPED: i64 = 2;

/// script_scoping：全局→角色→聊天 聚合链式应用。
pub const SCOPE_GLOBAL: i64 = 0;
pub const SCOPE_CHARACTER: i64 = 1;
pub const SCOPE_CHAT: i64 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RegexScript {
    pub id: String,
    #[serde(rename = "scriptName", alias = "script_name")]
    pub script_name: String,
    /// JS 正则语法（Rust 端用 regex crate 解析；不支持的 look-around 记录错误跳过）
    #[serde(rename = "findRegex", alias = "find_regex")]
    pub find_regex: String,
    #[serde(rename = "replaceString", alias = "replace_string")]
    pub replace_string: String,
    /// 先移除的子串列表（split(t).join('') 语义）
    #[serde(rename = "trimStrings", alias = "trim_strings")]
    pub trim_strings: Vec<String>,
    /// placement 数组（RP_*）
    pub placement: Vec<i64>,
    pub disabled: bool,
    /// 仅显示 pass
    #[serde(rename = "markdownOnly", alias = "markdown_only")]
    pub markdown_only: bool,
    /// 仅 prompt pass
    #[serde(rename = "promptOnly", alias = "prompt_only")]
    pub prompt_only: bool,
    /// 编辑消息时也运行（改动持久化进保存的消息）
    #[serde(rename = "runOnEdit", alias = "run_on_edit")]
    pub run_on_edit: bool,
    /// 0/1/2
    #[serde(rename = "substituteRegex", alias = "substitute_regex")]
    pub substitute_regex: i64,
    /// 距底深度过滤；null = 不限
    #[serde(rename = "minDepth", alias = "min_depth")]
    pub min_depth: Option<i64>,
    #[serde(rename = "maxDepth", alias = "max_depth")]
    pub max_depth: Option<i64>,
}

impl Default for RegexScript {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            script_name: String::new(),
            find_regex: String::new(),
            replace_string: String::new(),
            trim_strings: vec![],
            placement: vec![RP_USER_INPUT, RP_AI_OUTPUT],
            disabled: false,
            markdown_only: false,
            prompt_only: false,
            run_on_edit: true,
            substitute_regex: SUB_NONE,
            min_depth: None,
            max_depth: None,
        }
    }
}
