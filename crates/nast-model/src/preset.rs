//! OpenAI 预设 / PromptManager 类型。
//! 对齐 refrence/SillyTavern/public/scripts/PromptManager.js 与
//! default/content/presets/openai/Default.json（1.18.0）。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// PromptManager.js:37-40 INJECTION_POSITION
pub const INJ_RELATIVE: i64 = 0;
pub const INJ_ABSOLUTE: i64 = 1;

/// PromptManager.js:31-32
pub const INJ_DEFAULT_DEPTH: i64 = 4;
pub const INJ_DEFAULT_ORDER: i64 = 100;

/// Chat Completions 全局策略 dummy id（openai.js:666-712，GOTCHA #1）。
pub const CC_DUMMY_ID: i64 = 100001;
/// 文本补全管理器内部默认（仅保留兼容旧文件）
pub const TC_DUMMY_ID: i64 = 100000;

/// prompt 项标识符（Default.json prompts[]）
pub const ID_MAIN: &str = "main";
pub const ID_NSWF: &str = "nsfw";
pub const ID_JAILBREAK: &str = "jailbreak";
pub const ID_ENHANCE: &str = "enhanceDefinitions";
pub const ID_DIALOGUE_EXAMPLES: &str = "dialogueExamples";
pub const ID_CHAT_HISTORY: &str = "chatHistory";
pub const ID_WI_BEFORE: &str = "worldInfoBefore";
pub const ID_WI_AFTER: &str = "worldInfoAfter";
pub const ID_CHAR_DESCRIPTION: &str = "charDescription";
pub const ID_CHAR_PERSONALITY: &str = "charPersonality";
pub const ID_SCENARIO: &str = "scenario";
pub const ID_PERSONA: &str = "personaDescription";
pub const ID_IMPERSONATE: &str = "impersonate";
pub const ID_QUIET: &str = "quietPrompt";
pub const ID_GROUP_NUDGE: &str = "groupNudge";
pub const ID_BIAS: &str = "bias";
pub const ID_SYS_PROMPT: &str = "sysprompt";

/// 单个 prompt 项（PromptManager.js:80-196 字段集）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PromptEntry {
    pub enabled: bool,
    pub identifier: String,
    /// system/user/assistant
    pub role: String,
    pub content: String,
    pub name: String,
    pub system_prompt: bool,
    /// injection_position: 0 relative / 1 absolute（=INJ_*）
    pub injection_position: i64,
    pub injection_depth: i64,
    pub injection_order: i64,
    /// 卡片覆盖保护
    pub forbid_overrides: bool,
    /// injection_trigger：空 = 总是；否则含生成类型字符串
    pub injection_trigger: Vec<String>,
    /// marker 项（chatHistory 等）
    pub marker: bool,
    /// 外部字段透传
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Default for PromptEntry {
    fn default() -> Self {
        Self {
            enabled: true,
            identifier: String::new(),
            role: "system".into(),
            content: String::new(),
            name: String::new(),
            system_prompt: true,
            injection_position: INJ_RELATIVE,
            injection_depth: INJ_DEFAULT_DEPTH,
            injection_order: INJ_DEFAULT_ORDER,
            forbid_overrides: false,
            injection_trigger: vec![],
            marker: false,
            extra: Default::default(),
        }
    }
}

/// prompt_order 的一行：character_id（CC 用全局 100001）→ 有序项。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptOrderEntry {
    pub character_id: i64,
    pub order: Vec<PromptOrderItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptOrderItem {
    pub identifier: String,
    pub enabled: bool,
}

/// oai_settings（开放拼装引擎需要的字段；settings.json 内为一份 blob）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OaiSettings {
    pub prompts: Vec<PromptEntry>,
    pub prompt_order: Vec<PromptOrderEntry>,
    pub openai_max_context: i64,
    pub openai_max_tokens: i64,
    pub main_prompt: String,
    pub nsfw_prompt: String,
    pub jailbreak_prompt: String,
    pub impersonation_prompt: String,
    pub new_chat_prompt: String,
    pub new_group_chat_prompt: String,
    pub new_example_prompt: String,
    pub continue_nudge_prompt: String,
    pub continue_postfix: String,
    pub continue_prefill: bool,
    pub send_if_empty: String,
    pub squash_system_messages: bool,
    /// -1/0/1/2（NONE/DEFAULT/COMPLETION/CONTENT）
    pub character_names_behavior: i64,
    pub wi_format: String,
    pub scenario_format: String,
    pub personality_format: String,
    pub chat_completion_source: String,
    pub openai_model: String,
    pub claude_model: String,
    pub google_model: String,
    pub custom_url: String,
    pub custom_model: String,
    /// custom 源附加请求头："Header-Name: value" 每行一对（ST custom_include_headers）
    pub custom_include_headers: String,
    /// custom 源附加请求体：JSON 对象（ST custom_include_body）
    pub custom_include_body: String,
    pub stream_openai: bool,
    pub temperature: f64,
    pub frequency_penalty: f64,
    pub presence_penalty: f64,
    pub top_p: f64,
    pub max_context_unlocked: bool,
    pub custom_prompt_post_processing: String,
    pub include_reasoning: bool,
    /// Claude assistant prefill（默认空）
    pub assistant_prefill: String,
    /// Claude impersonation prefill
    pub assistant_impersonation: String,
    pub sysprompt: SysPromptState,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// power_user.sysprompt（CC 路径不使用，仅文本补全路径——保留字段）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SysPromptState {
    pub enabled: bool,
    pub prefer_character_prompt: bool,
    pub content: String,
    pub post_history: String,
}

impl Default for OaiSettings {
    fn default() -> Self {
        Self {
            prompts: default_prompts(),
            prompt_order: default_prompt_order(),
            openai_max_context: 4095,
            openai_max_tokens: 300,
            main_prompt: String::new(),
            nsfw_prompt: String::new(),
            jailbreak_prompt: String::new(),
            impersonation_prompt: DEFAULT_IMPERSONATION_PROMPT.into(),
            new_chat_prompt: "[Start a new Chat]".into(),
            new_group_chat_prompt: "[Start a new group chat. Group members: {{group}}]".into(),
            new_example_prompt: "[Example Chat]".into(),
            continue_nudge_prompt: "[Continue your last message without repeating its original content.]".into(),
            continue_postfix: " ".into(),
            continue_prefill: false,
            send_if_empty: String::new(),
            squash_system_messages: false,
            character_names_behavior: 0,
            wi_format: "{0}".into(),
            scenario_format: "{{scenario}}".into(),
            personality_format: "{{personality}}".into(),
            chat_completion_source: "openai".into(),
            openai_model: "gpt-4-turbo".into(),
            claude_model: String::new(),
            google_model: String::new(),
            custom_url: String::new(),
            custom_model: String::new(),
            custom_include_headers: String::new(),
            custom_include_body: String::new(),
            stream_openai: true,
            temperature: 1.0,
            frequency_penalty: 0.0,
            presence_penalty: 0.0,
            top_p: 1.0,
            max_context_unlocked: false,
            custom_prompt_post_processing: String::new(),
            include_reasoning: false,
            assistant_prefill: String::new(),
            assistant_impersonation: String::new(),
            sysprompt: Default::default(),
            extra: Default::default(),
        }
    }
}

pub const DEFAULT_IMPERSONATION_PROMPT: &str = "[System note: This message is from the user and {{char}} exists unoccupied. Write a reply that {{char}} might say next out loud. Don't write anything for {{user}}.]";

/// Default.json 的 prompts[]（main/nsfw/jailbreak 文本为 1.18.0 默认）。
pub fn default_prompts() -> Vec<PromptEntry> {
    let mk = |identifier: &str, name: &str, content: &str, marker: bool| PromptEntry {
        identifier: identifier.into(),
        name: name.into(),
        content: content.into(),
        marker,
        ..Default::default()
    };
    vec![
        mk("main", "Main Prompt", "Write {{char}}'s next reply in a fictional chat between {{char}} and {{user}}.", false),
        mk("nsfw", "Auxiliary Prompt", "", false),
        mk(ID_DIALOGUE_EXAMPLES, "Chat Examples", "", true),
        mk("jailbreak", "Post-History Instructions", "", false),
        mk(ID_CHAT_HISTORY, "Chat History", "", true),
        mk(ID_WI_AFTER, "World Info (after)", "", true),
        mk(ID_WI_BEFORE, "World Info (before)", "", true),
        mk("enhanceDefinitions", "Enhance Definitions",
           "If you have more knowledge of {{char}}, add to the character's lore and personality to enhance them but keep the Character Sheet's definitions absolute.", false),
        mk(ID_CHAR_DESCRIPTION, "Char Description", "", true),
        mk(ID_CHAR_PERSONALITY, "Char Personality", "", true),
        mk(ID_SCENARIO, "Scenario", "", true),
        mk(ID_PERSONA, "Persona Description", "", true),
    ]
}

/// Default.json prompt_order：100000（遗留，无 personaDescription）与 100001（生效）。
pub fn default_prompt_order() -> Vec<PromptOrderEntry> {
    let order = |include_persona: bool| {
        let mut o: Vec<PromptOrderItem> = ["main", ID_WI_BEFORE]
            .iter()
            .map(|i| PromptOrderItem { identifier: i.to_string(), enabled: true })
            .collect();
        if include_persona {
            o.push(PromptOrderItem { identifier: ID_PERSONA.into(), enabled: true });
        }
        o.extend(
            [
                ID_CHAR_DESCRIPTION,
                ID_CHAR_PERSONALITY,
                ID_SCENARIO,
                "enhanceDefinitions",
                "nsfw",
                ID_WI_AFTER,
                ID_DIALOGUE_EXAMPLES,
                ID_CHAT_HISTORY,
                "jailbreak",
            ]
            .iter()
            .map(|i| PromptOrderItem { identifier: i.to_string(), enabled: true }),
        );
        o
    };
    vec![
        PromptOrderEntry { character_id: TC_DUMMY_ID, order: order(false) },
        PromptOrderEntry { character_id: CC_DUMMY_ID, order: order(true) },
    ]
}
