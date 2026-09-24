//! Chat Completion 提示拼装：逐条复刻 openai.js 的 prepareOpenAIMessages 流程。
//!
//! 契约锚点（refrence/SillyTavern/public/scripts/openai.js，行号为 1.18.0）：
//! - prepareOpenAIMessages (1533)：budget = max_context - max_tokens
//! - populateChatCompletion (1176)：reserve 3（assistant priming）→
//!   worldInfoBefore/main/worldInfoAfter/charDescription/charPersonality/scenario/personaDescription
//!   → controlPrompts（impersonate/quiet）→ nsfw/jailbreak/用户相对项/enhanceDefinitions
//!   → bias → 已知扩展注入 main → continue prefill → 绝对深度注入 → 历史+示例
//!   → freeBudget(controlPrompts) + add(controlPrompts)
//! - populateChatHistory (876)：new-chat 无条件 reserve+insertAtStart；历史倒序填充，
//!   首条塞不下即 break；末尾 add(continueNudgeCollection, -1)
//! - populationInjectionPrompts (801)：深度从末尾数，order 组降序处理最终升序呈现，
//!   角色 system→user→assistant，order-100 桶合并扩展 IN_CHAT 注入
//! - squashSystemMessages (3827)：排除 newMainChat/newChat/groupNudge，\n 连接
//! - getChat (3737)：空 content 且无 tool_calls 的消息丢弃

use crate::macros::{evaluate_macros, MacroContext, MacroEnv};
use crate::tokens::count_tokens;
use nast_model::preset::{
    OaiSettings, CC_DUMMY_ID, ID_BIAS, ID_CHAT_HISTORY, ID_ENHANCE,
    ID_IMPERSONATE, ID_JAILBREAK, ID_MAIN, ID_NSWF, ID_PERSONA, ID_QUIET, ID_SCENARIO,
    ID_WI_AFTER, ID_WI_BEFORE, INJ_ABSOLUTE, INJ_DEFAULT_ORDER,
};

/// 一条拼装产物消息（getChat 输出形状）。
#[derive(Debug, Clone, PartialEq)]
pub struct PromptMessage {
    pub role: String,
    pub content: String,
    pub name: Option<String>,
    /// 拼装期内部标识（squash 排除表用）
    pub identifier: String,
    pub injected: bool,
    pub tokens: i64,
}

impl PromptMessage {
    fn new(role: &str, content: String, identifier: &str, injected: bool) -> Self {
        let tokens = count_tokens(&content, crate::tokens::resolve_tokenizer("gpt-4o")) as i64;
        Self {
            role: role.to_string(),
            content,
            name: None,
            identifier: identifier.to_string(),
            injected,
            tokens,
        }
    }

    /// 按来源选择 tokenizer 计数（D10：TokenHandler 语义）。
    fn new_with_tok(
        role: &str,
        content: String,
        identifier: &str,
        injected: bool,
        source: &str,
        model: &str,
    ) -> Self {
        let model = crate::tokens::tokenizer_model_for_source(source, model);
        let tokens = count_tokens(&content, crate::tokens::resolve_tokenizer(&model)) as i64;
        Self {
            role: role.to_string(),
            content,
            name: None,
            identifier: identifier.to_string(),
            injected,
            tokens,
        }
    }
}

/// generation input（对应 prepareOpenAIMessages 的参数包）。
pub struct AssembleInput<'a> {
    pub macro_env: Option<&'a MacroEnv>,
    pub macro_context: Option<&'a std::cell::RefCell<MacroContext>>,
    pub oai: &'a OaiSettings,
    pub generation_type: &'a str, // normal/continue/impersonate/swipe/regenerate/quiet
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
    pub quiet_prompt: String,
    pub bias: String,
    pub system_prompt_override: Option<String>,
    pub jailbreak_prompt_override: Option<String>,
    /// 消息历史（openai.js setOpenAIMessages 产物，已含 name 前缀逻辑由调用方完成）
    /// 每项: (role, content, name, injected=false)
    pub messages: Vec<HistoryMessage>,
    pub message_examples: Vec<ExampleBlock>,
    pub pin_examples: bool,
    /// extension IN_CHAT 注入（AN/depth prompt/WI depth entries）
    /// 每项: (content, depth, role: 0/1/2, injection_order)
    pub in_chat_injections: Vec<InChatInjection>,
    /// AN 相对注入（position 0 = 主提示后 / 2 = 主提示前；AN.js getPromptPosition 'end'/'start'）。
    /// 聊天内深度（position 1）由调用方走 in_chat_injections。
    pub authors_note: Option<AuthorsNote>,
    pub continue_prefill_assistant: bool, // source==claude 且 continue_prefill
    pub assistant_prefill: String,
    /// continue 模式：被续消息原文（nudge 替换 {{lastChatMessage}} / prefill 拼接）
    pub cycle_prompt: Option<String>,
    pub last_role: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HistoryMessage {
    pub role: String, // user/assistant/system
    pub content: String,
    pub name: Option<String>,
    /// narrator 类系统消息
    pub is_narrator: bool,
    /// 注入标记（populationInjectionPrompts 产物）
    pub injected: bool,
}

#[derive(Debug, Clone)]
pub struct ExampleBlock {
    /// 每条: (role user/assistant, name example_user/example_assistant, content)
    pub messages: Vec<(String, String, String)>,
}

#[derive(Debug, Clone)]
pub struct InChatInjection {
    pub content: String,
    pub depth: i64,
    pub role: i64, // 0 system / 1 user / 2 assistant
    pub injection_order: i64,
}

/// AN 相对注入（仅 position 0/2 走此路径）。
#[derive(Debug, Clone)]
pub struct AuthorsNote {
    pub text: String,
    /// 0 = 主提示后（IN_PROMPT，'end'）/ 2 = 主提示前（BEFORE_PROMPT，'start'）
    pub position: i64,
}

/// 拼装结果。
pub struct AssembleOutput {
    pub chat: Vec<PromptMessage>,
    pub token_counts: Vec<i64>,
    pub error: Option<String>,
}

/// 主入口：等价 prepareOpenAIMessages。
pub fn assemble(input: &AssembleInput) -> AssembleOutput {
    let oai = input.oai;
    let budget: i64 = oai.openai_max_context - oai.openai_max_tokens;
    let mut error = None;

    // ---------- preparePromptsForChatCompletion：构建 systemPrompts 并合并用户顺序 ----------
    // 用户 prompt_order（CC 全局 100001 行；缺失则空）
    let order_items: Vec<(String, bool)> = oai
        .prompt_order
        .iter()
        .find(|o| o.character_id == CC_DUMMY_ID)
        .map(|o| o.order.iter().map(|x| (x.identifier.clone(), x.enabled)).collect())
        .unwrap_or_default();

    // prompts：有序集合（identifier → content/role/...），从 oai.prompts 补齐内容
    let prompt_map: std::collections::HashMap<&str, &nast_model::preset::PromptEntry> = oai
        .prompts
        .iter()
        .map(|p| (p.identifier.as_str(), p))
        .collect();

    // systemPrompts 生成（preparePromptsForChatCompletion 1358-1507）
    let scenario_text = if !input.scenario.is_empty() && !oai.scenario_format.is_empty() {
        substitute(oai.scenario_format.as_str(), input)
    } else {
        input.scenario.clone()
    };
    let personality_text = if !input.char_personality.is_empty() && !oai.personality_format.is_empty() {
        substitute(oai.personality_format.as_str(), input)
    } else {
        input.char_personality.clone()
    };
    let wi_fmt = |s: &str| format_wi(s, &oai.wi_format);
    let group_nudge = substitute("[Write the next reply only as {{char}}.]", input);
    let impersonation_prompt = if !oai.impersonation_prompt.is_empty() {
        substitute(&oai.impersonation_prompt, input)
    } else {
        String::new()
    };

    // 动态 systemPrompts（identifier → (role, content)）
    let mut dynamic: std::collections::HashMap<String, (String, String)> =
        std::collections::HashMap::new();
    dynamic.insert(
        ID_WI_BEFORE.into(),
        ("system".into(), wi_fmt(&input.world_info_before)),
    );
    dynamic.insert(
        ID_WI_AFTER.into(),
        ("system".into(), wi_fmt(&input.world_info_after)),
    );
    dynamic.insert(
        "charDescription".into(),
        ("system".into(), input.char_description.clone()),
    );
    dynamic.insert("charPersonality".into(), ("system".into(), personality_text));
    dynamic.insert(ID_SCENARIO.into(), ("system".into(), scenario_text));
    dynamic.insert(ID_IMPERSONATE.into(), ("system".into(), impersonation_prompt));
    dynamic.insert(ID_QUIET.into(), ("system".into(), input.quiet_prompt.clone()));
    dynamic.insert("groupNudge".into(), ("system".into(), group_nudge.clone()));
    dynamic.insert(ID_BIAS.into(), ("assistant".into(), input.bias.clone()));
    if input.persona_position_in_prompt {
        dynamic.insert(
            ID_PERSONA.into(),
            ("system".into(), input.persona_description.clone()),
        );
    }
    // order-100 桶外的绝对注入由注入阶段处理；相对扩展（AN before/after main）
    // 简化为 authorsNote 处理：in_chat_injections 中 depth>=0 的走注入。
    // 已知相对扩展（summary/authorsNote position IN_PROMPT）由调用方放入
    // in_chat_injections 之外的 rel_main_before/rel_main_after —— v1 先省略（无扩展）。

    // 用户有序集合：按 prompt_order 展开（enabled 过滤在 add 阶段做）
    // collection: Vec<(identifier, role, content, injection_position, injection_depth, injection_order, forbid_overrides, marker, enabled)>
    let mut collection: Vec<CollectionItem> = order_items
        .iter()
        .map(|(id, enabled)| {
            let p = prompt_map.get(id.as_str());
            CollectionItem {
                identifier: id.clone(),
                role: p.map(|p| p.role.clone()).unwrap_or_else(|| "system".into()),
                content: p.map(|p| p.content.clone()).unwrap_or_default(),
                injection_position: p.map(|p| p.injection_position).unwrap_or(0),
                injection_depth: p.map(|p| p.injection_depth).unwrap_or(4),
                injection_order: p.map(|p| p.injection_order).unwrap_or(INJ_DEFAULT_ORDER),
                forbid_overrides: p.map(|p| p.forbid_overrides).unwrap_or(false),
                marker: p.map(|p| p.marker).unwrap_or(false),
                enabled: *enabled,
            }
        })
        .collect();

    // 合并动态内容到 collection（marker 替换 + 覆盖角色/深度）
    for (id, (role, content)) in &dynamic {
        if let Some(item) = collection.iter_mut().find(|c| &c.identifier == id) {
            if !item.marker {
                // 非 marker 的动态项（impersonate/groupNudge/bias 等）在 collection 中
                // 没有对应条目时走 add 分支；有对应条目时覆盖角色/深度。
                item.role = role.clone();
                item.content = content.clone();
                continue;
            }
            item.role = role.clone();
            item.content = content.clone();
        } else {
            collection.push(CollectionItem {
                identifier: id.clone(),
                role: role.clone(),
                content: content.clone(),
                injection_position: 0,
                injection_depth: 4,
                injection_order: INJ_DEFAULT_ORDER,
                forbid_overrides: false,
                marker: false,
                enabled: true,
            });
        }
    }

    // 卡片覆盖 main/jailbreak（forbid_overrides 检查）
    apply_override(
        &mut collection,
        ID_MAIN,
        &input.system_prompt_override,
    );
    apply_override(
        &mut collection,
        ID_JAILBREAK,
        &input.jailbreak_prompt_override,
    );

    // ---------- populateChatCompletion ----------
    // JS 语义：chatCompletion.add(collection, prompts.index(identifier)) 按
    // prompt_order 位置放置集合 → flatten 后最终顺序 = prompt_order 顺序。
    // 因此按 prompt_order 顺序逐项生成消息，chatHistory 位置展开历史。
    let mut reserved: i64 = 0;
    reserved += 3; // reserveBudget(3)：assistant priming

    // controlPrompts：impersonate + quiet（总是最末尾）
    let mut control: Vec<PromptMessage> = Vec::new();
    if input.generation_type == "impersonate" {
        if let Some(item) = collection.iter().find(|c| c.identifier == ID_IMPERSONATE) {
            let text = substitute(&item.content, input);
            if !text.is_empty() {
                control.push(PromptMessage::new(&item.role, text, ID_IMPERSONATE, false));
            }
        }
    }
    if let Some(item) = collection.iter().find(|c| c.identifier == ID_QUIET) {
        if !item.content.is_empty() {
            control.push(PromptMessage::new(&item.role, item.content.clone(), ID_QUIET, false));
        }
    }
    let control_tokens: i64 = control.iter().map(|m| m.tokens).sum();
    reserved += control_tokens;

    // continue prefill：被续消息移出历史进 controlPrompts（Claude 专用路径）
    let mut messages: Vec<HistoryMessage> = input.messages.clone();
    if input.generation_type == "continue"
        && input.continue_prefill_assistant
        && !messages.is_empty()
    {
        let chat_message = messages.remove(messages.len() - 1);
        let is_assistant = chat_message.role == "assistant";
        let prefill = if is_assistant {
            substitute(&input.assistant_prefill, input)
        } else {
            String::new()
        };
        let content = [prefill, chat_message.content.clone()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        let msg = PromptMessage::new(&chat_message.role, content, "continuePrefill", false);
        reserved += msg.tokens;
        // 插在 quiet 之前（"Add all further control prompts BEFORE this prompt"）
        let quiet_pos = control
            .iter()
            .position(|m| m.identifier == ID_QUIET)
            .unwrap_or(control.len());
        control.insert(quiet_pos, msg);
    }

    // 绝对注入（populationInjectionPrompts）：messages 新→旧，深度从末尾数
    let mut absolute_prompts: Vec<(i64, i64, i64, String, String)> = collection
        .iter()
        .filter(|c| c.injection_position == INJ_ABSOLUTE && !c.content.is_empty())
        .map(|c| {
            (
                c.injection_depth,
                c.injection_order,
                role_num(&c.role),
                c.role.clone(),
                c.content.clone(),
            )
        })
        .collect();
    for inj in &input.in_chat_injections {
        absolute_prompts.push((
            inj.depth,
            inj.injection_order,
            inj.role,
            role_name(inj.role),
            inj.content.clone(),
        ));
    }
    messages = inject_prompts(absolute_prompts, messages);

    // 历史填充准备（populateChatHistory）
    let new_chat_text = if input.is_group {
        substitute(&oai.new_group_chat_prompt, input)
    } else {
        substitute(&oai.new_chat_prompt, input)
    };
    let new_chat_tokens = tok_for(&new_chat_text, &oai.chat_completion_source, &oai.openai_model);
    reserved += new_chat_tokens;

    let group_nudge_tokens = if input.is_group && input.generation_type != "impersonate" {
        let t = tok_for(&group_nudge, &oai.chat_completion_source, &oai.openai_model);
        reserved += t;
        t
    } else {
        0
    };

    // continue nudge 模式（非 prefill）：被续消息 + nudge 集合移到最末尾
    let mut continue_nudge_tail: Vec<PromptMessage> = Vec::new();
    if input.generation_type == "continue"
        && !input.continue_prefill_assistant
        && !messages.is_empty()
    {
        let idx = messages.iter().rposition(|m| !m.injected);
        let mut continued: Option<HistoryMessage> = None;
        if let Some(pos) = idx {
            continued = Some(messages.remove(pos));
        }
        // cycle_prompt 优先（生成器已知原文），否则取被续消息内容
        let last_text = input
            .cycle_prompt
            .as_ref()
            .map(|s| s.trim().to_string())
            .or_else(|| {
                continued.as_ref().map(|m| m.content.trim().to_string())
            })
            .unwrap_or_default();
        let nudge_text =
            substitute_dyn(&oai.continue_nudge_prompt, &[("lastChatMessage", &last_text)], input);
        reserved += continued
            .as_ref()
            .map(|m| tok_for(&m.content, &oai.chat_completion_source, &oai.openai_model))
            .unwrap_or(0)
            + tok_for(&nudge_text, &oai.chat_completion_source, &oai.openai_model);
        if let Some(c) = continued {
            continue_nudge_tail.push(PromptMessage::new(&c.role, c.content, "continueNudge", false));
        }
        continue_nudge_tail.push(PromptMessage::new("system", nudge_text, "continueNudge", false));
    }

    // send_if_empty：最后一条是 assistant 且设置非空 → user 占位
    let mut send_if_empty_msg: Option<PromptMessage> = None;
    if let Some(last) = messages.last() {
        if last.role == "assistant" && !oai.send_if_empty.is_empty() {
            send_if_empty_msg = Some(PromptMessage::new(
                "user",
                oai.send_if_empty.clone(),
                "emptyUserMessageReplacement",
                false,
            ));
        }
    }

    // 历史填充：倒序（新→旧），首条塞不下即 break；insertAtStart → 最终旧→新
    let mut history: Vec<PromptMessage> = Vec::new();
    for m in messages.iter().rev() {
        let mut msg = PromptMessage::new(&m.role, m.content.clone(), "chatHistory", false);
        msg.name = m.name.clone(); // COMPLETION names_behavior
        if reserved + msg.tokens > budget {
            break;
        }
        reserved += msg.tokens;
        history.insert(0, msg);
    }
    // new chat 无条件插最前 + freeBudget
    reserved -= new_chat_tokens;
    history.insert(0, PromptMessage::new("system", new_chat_text.clone(), "newMainChat", false));

    if input.is_group && input.generation_type != "impersonate" && group_nudge_tokens > 0 {
        reserved -= group_nudge_tokens;
        history.push(PromptMessage::new("system", group_nudge.clone(), "groupNudge", false));
    }
    // send_if_empty 在 newMainChat 之后（JS: 先 insert 到空 chatHistory 首位，
    // 再 insertAtStart(newChat) 覆盖其上）
    if let Some(m) = send_if_empty_msg {
        history.insert(1, m);
    }

    // 对话示例：all-or-nothing per block，全部 system 角色
    let mut examples: Vec<PromptMessage> = Vec::new();
    for (bi, block) in input.message_examples.iter().enumerate() {
        let mut block_msgs = vec![PromptMessage::new(
            "system",
            substitute(&oai.new_example_prompt, input),
            &format!("example-{bi}"),
            false,
        )];
        let mut block_tokens = tok_for(&oai.new_example_prompt, &oai.chat_completion_source, &oai.openai_model);
        for (_role, name, content) in &block.messages {
            let mut text = content.clone();
            if input.is_group {
                text = format!("{name}: {text}");
            }
            let msg = PromptMessage::new("system", text, "example-msg", false);
            block_tokens += msg.tokens;
            block_msgs.push(msg);
        }
        if reserved + block_tokens <= budget {
            reserved += block_tokens;
            examples.extend(block_msgs);
        } else {
            break;
        }
    }

    // AN 相对注入（position 0/2）：预留预算，order 放置后相对 main 插入
    let an_msg: Option<PromptMessage> = input.authors_note.as_ref().and_then(|an| {
        if an.text.trim().is_empty() {
            return None;
        }
        Some(PromptMessage::new_with_tok(
            "system",
            an.text.clone(),
            "authorsNote",
            false,
            &oai.chat_completion_source,
            &oai.openai_model,
        ))
    });
    if let Some(m) = &an_msg {
        reserved += m.tokens;
    }

    // ---------- 按 prompt_order 顺序放置（= JS add(collection, index) flatten） ----------
    let mut chat: Vec<PromptMessage> = Vec::new();
    for item in &collection {
        if !item.enabled && item.identifier != ID_MAIN {
            continue;
        }
        if item.injection_position == INJ_ABSOLUTE {
            continue;
        }
        // 这些动态项由 controlPrompts / 历史区专门处理，不在 order 放置阶段输出
        if matches!(
            item.identifier.as_str(),
            ID_IMPERSONATE | ID_QUIET | "groupNudge"
        ) {
            continue;
        }
        // 动态内容优先（marker 填充），否则静态内容 + preparePrompt 宏替换
        let content = if let Some((_, dyn_content)) = dynamic.get(&item.identifier) {
            dyn_content.clone()
        } else {
            substitute(&item.content, input)
        };
        if item.identifier == ID_CHAT_HISTORY {
            chat.extend(examples.iter().cloned());
            chat.extend(history.iter().cloned());
            continue;
        }
        let is_known = matches!(
            item.identifier.as_str(),
            ID_MAIN
                | ID_WI_BEFORE
                | ID_WI_AFTER
                | "charDescription"
                | "charPersonality"
                | ID_SCENARIO
                | ID_PERSONA
                | ID_NSWF
                | ID_JAILBREAK
                | ID_ENHANCE
                | ID_BIAS
        );
        if is_known {
            if !content.is_empty() || item.identifier == ID_MAIN {
                let msg = PromptMessage::new_with_tok(&item.role, content, &item.identifier, false, &oai.chat_completion_source, &oai.openai_model);
                // 预算检查：main 强制（超限报错，对应 JS TokenBudgetExceededError），
                // 其余塞不下跳过（JS insert() 的 canAfford 检查）
                if reserved + msg.tokens <= budget {
                    reserved += msg.tokens;
                    chat.push(msg);
                } else if item.identifier == ID_MAIN {
                    error = Some("Mandatory prompts exceed the context size.".into());
                    return AssembleOutput { chat: vec![], token_counts: vec![], error };
                }
            }
        } else if !content.is_empty() {
            // 用户自定义相对项
            let msg = PromptMessage::new_with_tok(&item.role, content, &item.identifier, false, &oai.chat_completion_source, &oai.openai_model);
            if reserved + msg.tokens <= budget {
                reserved += msg.tokens;
                chat.push(msg);
            }
        }
    }

    // AN 相对插入：2 = main 之前 / 0 = main 之后（PM 'start'/'end'）
    if let Some(an) = an_msg {
        let main_idx = chat
            .iter()
            .position(|m| m.identifier == ID_MAIN)
            .unwrap_or(0);
        let insert_at = if input
            .authors_note
            .as_ref()
            .map(|a| a.position == 2)
            .unwrap_or(false)
        {
            main_idx
        } else {
            main_idx + 1
        };
        chat.insert(insert_at.min(chat.len()), an);
    }

    // controlPrompts 末尾（freeBudget 后 add）
    chat.extend(continue_nudge_tail);
    chat.extend(control);

    // squash（非 dry run）
    if oai.squash_system_messages {
        chat = squash_system_messages(chat);
    }

    // getChat：丢弃空内容
    chat.retain(|m| !m.content.is_empty());

    let token_counts: Vec<i64> = chat.iter().map(|m| m.tokens).collect();
    AssembleOutput { chat, token_counts, error }
}

struct CollectionItem {
    identifier: String,
    role: String,
    content: String,
    injection_position: i64,
    injection_depth: i64,
    injection_order: i64,
    forbid_overrides: bool,
    marker: bool,
    enabled: bool,
}

fn apply_override(collection: &mut [CollectionItem], ident: &str, override_text: &Option<String>) {
    if let Some(text) = override_text {
        if let Some(item) = collection.iter_mut().find(|c| c.identifier == ident) {
            if !item.forbid_overrides && item.enabled {
                item.content = text.clone();
            }
        }
    }
}

fn role_num(role: &str) -> i64 {
    match role {
        "user" => 1,
        "assistant" => 2,
        _ => 0,
    }
}

fn role_name(num: i64) -> String {
    match num {
        1 => "user".into(),
        2 => "assistant".into(),
        _ => "system".into(),
    }
}


/// populationInjectionPrompts：messages 输入为旧→新（人类时序）；深度从末尾数。
/// 输出仍为旧→新。
fn inject_prompts(
    prompts: Vec<(i64, i64, i64, String, String)>, // (depth, order, role_num, role, content)
    mut messages: Vec<HistoryMessage>,
) -> Vec<HistoryMessage> {
    // JS 契约是"新→旧"输入；这里内部先翻转对齐
    messages.reverse();
    let max_depth = prompts.iter().map(|p| p.0).max().unwrap_or(0);
    let mut total_inserted = 0usize;
    for depth in 0..=max_depth {
        // order 组：降序处理（GOTCHA #5）
        let mut orders: Vec<i64> = prompts
            .iter()
            .filter(|p| p.0 == depth)
            .map(|p| p.1)
            .collect();
        orders.sort_unstable_by(|a, b| b.cmp(a));
        orders.dedup();
        let mut role_msgs: Vec<HistoryMessage> = Vec::new();
        for order in orders {
            // roles: system→user→assistant
            for role in ["system", "user", "assistant"] {
                let joined: Vec<String> = prompts
                    .iter()
                    .filter(|p| p.0 == depth && p.1 == order && p.3 == role)
                    .map(|p| p.4.clone())
                    .collect();
                let content = joined.join("\n");
                if !content.is_empty() {
                    role_msgs.push(HistoryMessage {
                        role: role.to_string(),
                        content,
                        name: None,
                        is_narrator: false,
                        injected: true,
                    });
                }
            }
        }
        if !role_msgs.is_empty() {
            let inject_idx = depth as usize + total_inserted;
            let inject_idx = inject_idx.min(messages.len());
            total_inserted += role_msgs.len();
            messages.splice(inject_idx..inject_idx, role_msgs);
        }
    }
    messages.reverse(); // 恢复旧→新
    messages
}

fn squash_system_messages(messages: Vec<PromptMessage>) -> Vec<PromptMessage> {
    const EXCLUDE: [&str; 3] = ["newMainChat", "newChat", "groupNudge"];
    let mut out: Vec<PromptMessage> = Vec::new();
    for m in messages {
        if m.role == "system" && m.content.is_empty() {
            continue;
        }
        let should_squash =
            m.role == "system" && m.name.is_none() && !EXCLUDE.contains(&m.identifier.as_str());
        if should_squash {
            if let Some(last) = out.last_mut() {
                let last_squash = last.role == "system"
                    && last.name.is_none()
                    && !EXCLUDE.contains(&last.identifier.as_str());
                if last_squash {
                    last.content.push('\n');
                    last.content.push_str(&m.content);
                    last.tokens += m.tokens;
                    continue;
                }
            }
            out.push(m);
        } else {
            out.push(m);
        }
    }
    out
}


/// 按 chat_completion_source/model 解析 tokenizer（TokenHandler 语义）。
pub fn tok_for(s: &str, source: &str, model: &str) -> i64 {
    let model = crate::tokens::tokenizer_model_for_source(source, model);
    count_tokens(s, crate::tokens::resolve_tokenizer(&model)) as i64
}

fn format_wi(s: &str, fmt: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    fmt.replace("{0}", s)
}

/// {{char}}/{{user}} 等基础宏替换（拼装期）。
fn substitute(text: &str, input: &AssembleInput) -> String {
    substitute_dyn(text, &[], input)
}

fn substitute_dyn(text: &str, extra: &[(&str, &str)], input: &AssembleInput) -> String {
    let env = MacroEnv {
        user: input.name1.to_string(),
        char: input.name2.to_string(),
        group: input.macro_env.map(|env| env.group.clone()).unwrap_or_else(|| input.name2.to_string()),
        // personality/scenario format 宏替换需要角色卡字段
        description: input.char_description.clone(),
        personality: input.char_personality.clone(),
        scenario: input.scenario.clone(),
        persona: input.persona_description.clone(),
        ..input.macro_env.cloned().unwrap_or_default()
    };
    let mut ctx = MacroContext::default();
    let mut out = if let Some(context) = input.macro_context {
        evaluate_macros(text, &env, &mut context.borrow_mut())
    } else { evaluate_macros(text, &env, &mut ctx) };
    for (k, v) in extra {
        out = out.replace(&format!("{{{{{k}}}}}"), v);
    }
    out
}
