//! prompt 拼装桥：把宏替换接入 nast_engine::prompt::assemble，
//! 并提供 impersonate/quiet/continue 变体的拼装。

use nast_engine::macros::{evaluate_macros, MacroContext, MacroEnv};
use nast_engine::prompt::{assemble, AssembleInput, AssembleOutput};
use nast_model::preset::OaiSettings;

pub use nast_engine::prompt::{ExampleBlock, HistoryMessage, InChatInjection};

/// 生成器侧的拼装输入（借用 oai）。
pub struct BridgeInput<'a> {
    /// 预留：生成器侧直接访问 oai（当前消费者经 session 持有，保留 API 对称）
    #[allow(dead_code)]
    pub oai: &'a OaiSettings,
    pub generation_type: &'a str,
    pub name1: &'a str,
    pub name2: &'a str,
    pub is_group: bool,
    pub char_description: String,
    pub char_personality: String,
    pub scenario: String,
    pub persona_description: String,
    pub persona_position_in_prompt: bool,
    pub world_info_before: String,
    pub world_info_after: String,
    pub messages: Vec<HistoryMessage>,
    pub message_examples: Vec<ExampleBlock>,
    pub pin_examples: bool,
    pub in_chat_injections: Vec<InChatInjection>,
    pub system_prompt_override: Option<String>,
    pub jailbreak_prompt_override: Option<String>,
    pub cycle_prompt: Option<String>,
    pub last_role: Option<String>,
}

/// 基础宏环境（char/user/persona/description 等）。
fn env_for<'a>(input: &'a BridgeInput) -> MacroEnv {
    MacroEnv {
        user: input.name1.to_string(),
        char: input.name2.to_string(),
        group: input.name2.to_string(),
        description: input.char_description.clone(),
        personality: input.char_personality.clone(),
        scenario: input.scenario.clone(),
        persona: input.persona_description.clone(),
        ..Default::default()
    }
}

/// 宏替换一条文本。
pub fn substitute_text(text: &str, env: &MacroEnv) -> String {
    let mut ctx = MacroContext::default();
    evaluate_macros(text, env, &mut ctx)
}

/// 简化的 {{char}}/{{user}} 替换。
pub fn substitute_basic(text: &str, user: &str, char: &str) -> String {
    let env = MacroEnv {
        user: user.to_string(),
        char: char.to_string(),
        group: char.to_string(),
        ..Default::default()
    };
    substitute_text(text, &env)
}

/// 主拼装：先对历史内容做宏替换，再走 assemble。
pub fn assemble_with_macros(oai: &OaiSettings, input: &BridgeInput) -> AssembleOutput {
    let env = env_for(input);
    let mut messages = input.messages.clone();
    for m in &mut messages {
        m.content = substitute_text(&m.content, &env);
    }
    let ai = AssembleInput {
        oai,
        generation_type: input.generation_type,
        name1: input.name1,
        name2: input.name2,
        is_group: input.is_group,
        char_description: substitute_text(&input.char_description, &env),
        char_personality: substitute_text(&input.char_personality, &env),
        scenario: substitute_text(&input.scenario, &env),
        persona_description: substitute_text(&input.persona_description, &env),
        persona_position_in_prompt: input.persona_position_in_prompt,
        world_info_before: input.world_info_before.clone(),
        world_info_after: input.world_info_after.clone(),
        quiet_prompt: String::new(),
        bias: String::new(),
        system_prompt_override: None,
        jailbreak_prompt_override: None,
        messages,
        message_examples: input.message_examples.clone(),
        pin_examples: input.pin_examples,
        in_chat_injections: input.in_chat_injections.clone(),
        continue_prefill_assistant: false,
        assistant_prefill: String::new(),
        cycle_prompt: None,
        last_role: None,
    };
    assemble(&ai)
}

/// impersonate：impersonation prompt（system）+ 完整历史，controlPrompts 收尾。
pub fn assemble_impersonate(
    oai: &OaiSettings,
    input: &BridgeInput,
    impersonation_prompt: &str,
) -> AssembleOutput {
    let mut ai = bridge_to_input(oai, input);
    ai.generation_type = "impersonate";
    ai.quiet_prompt = impersonation_prompt.to_string();
    assemble(&ai)
}

/// quiet：quiet prompt + 历史，quiet 作为 controlPrompts 收尾，返回字符串不落盘。
pub fn assemble_quiet(
    oai: &OaiSettings,
    input: &BridgeInput,
    quiet_prompt: &str,
) -> AssembleOutput {
    let mut ai = bridge_to_input(oai, input);
    ai.generation_type = "quiet";
    ai.quiet_prompt = quiet_prompt.to_string();
    assemble(&ai)
}

/// continue nudge 模式：被续消息 + nudge 移到最末尾。
pub fn assemble_continue_nudge(
    oai: &OaiSettings,
    input: &BridgeInput,
    last_message: &str,
) -> AssembleOutput {
    let mut ai = bridge_to_input(oai, input);
    ai.generation_type = "continue";
    // continue_nudge_prompt 由拼装器从 oai 读取；lastChatMessage 由拼装器替换
    ai.cycle_prompt = Some(last_message.to_string());
    assemble(&ai)
}

/// continue prefill 模式（Claude）：被续消息移入 controlPrompts + assistant_prefill。
pub fn assemble_continue_prefill(
    oai: &OaiSettings,
    input: &BridgeInput,
    last_message: &str,
    last_role: &str,
) -> AssembleOutput {
    let mut ai = bridge_to_input(oai, input);
    ai.generation_type = "continue";
    ai.continue_prefill_assistant = true;
    ai.assistant_prefill = oai.assistant_prefill.clone();
    ai.cycle_prompt = Some(format!("{last_message}{}", oai.continue_postfix));
    ai.last_role = Some(last_role.to_string());
    assemble(&ai)
}

fn bridge_to_input<'a>(oai: &'a OaiSettings, input: &'a BridgeInput) -> AssembleInput<'a> {
    let env = env_for(input);
    let mut messages = input.messages.clone();
    for m in &mut messages {
        m.content = substitute_text(&m.content, &env);
    }
    AssembleInput {
        oai,
        generation_type: input.generation_type,
        name1: input.name1,
        name2: input.name2,
        is_group: input.is_group,
        char_description: substitute_text(&input.char_description, &env),
        char_personality: substitute_text(&input.char_personality, &env),
        scenario: substitute_text(&input.scenario, &env),
        persona_description: substitute_text(&input.persona_description, &env),
        persona_position_in_prompt: input.persona_position_in_prompt,
        world_info_before: input.world_info_before.clone(),
        world_info_after: input.world_info_after.clone(),
        quiet_prompt: String::new(),
        bias: String::new(),
        system_prompt_override: input.system_prompt_override.clone(),
        jailbreak_prompt_override: input.jailbreak_prompt_override.clone(),
        messages,
        message_examples: input.message_examples.clone(),
        pin_examples: input.pin_examples,
        in_chat_injections: input.in_chat_injections.clone(),
        continue_prefill_assistant: false,
        assistant_prefill: String::new(),
        cycle_prompt: input.cycle_prompt.clone(),
        last_role: input.last_role.clone(),
    }
}
