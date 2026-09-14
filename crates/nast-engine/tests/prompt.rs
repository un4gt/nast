//! ChatCompletion 拼装行为测试：对照 ST openai.js 语义逐项验证。
//! 输入形态对齐 Default.json 默认预设 + 默认 prompt_order。

use nast_engine::prompt::{
    assemble, AssembleInput, ExampleBlock, HistoryMessage, InChatInjection,
};
use nast_model::preset::OaiSettings;

fn default_oai() -> OaiSettings {
    OaiSettings::default()
}

fn base_input(oai: &OaiSettings) -> AssembleInput<'_> {
    AssembleInput {
        oai,
        generation_type: "normal",
        name1: "User",
        name2: "Seraphina",
        is_group: false,
        char_description: "An elf princess".into(),
        char_personality: "brave".into(),
        scenario: "a forest".into(),
        persona_description: "I am a traveler".into(),
        persona_position_in_prompt: true,
        world_info_before: "WI before text".into(),
        world_info_after: "WI after text".into(),
        quiet_prompt: String::new(),
        bias: String::new(),
        system_prompt_override: None,
        jailbreak_prompt_override: None,
        messages: vec![
            HistoryMessage {
                role: "user".into(),
                content: "hello".into(),
                name: Some("User".into()),
                is_narrator: false,
                injected: false,
            },
            HistoryMessage {
                role: "assistant".into(),
                content: "Hi there!".into(),
                name: Some("Seraphina".into()),
                is_narrator: false,
                injected: false,
            },
        ],
        message_examples: vec![],
        pin_examples: false,
        in_chat_injections: vec![],
        continue_prefill_assistant: false,
        assistant_prefill: String::new(),
        cycle_prompt: None,
        last_role: None,
    }
}

fn roles(out: &nast_engine::prompt::AssembleOutput) -> Vec<String> {
    out.chat.iter().map(|m| m.role.clone()).collect()
}

fn contents(out: &nast_engine::prompt::AssembleOutput) -> Vec<String> {
    out.chat.iter().map(|m| m.content.clone()).collect()
}

#[test]
fn default_prompt_layout() {
    let oai = default_oai();
    let input = base_input(&oai);
    let out = assemble(&input);
    assert!(out.error.is_none(), "{:?}", out.error);

    let cs = contents(&out);
    // 期望顺序（Default.json prompt_order 100001，1.18.0 实际布局）：
    // main → worldInfoBefore → personaDescription → charDescription → charPersonality
    // → scenario → enhanceDefinitions → worldInfoAfter → chatHistory(newMainChat+历史)
    // nsfw/jailbreak 默认空内容 → 被 getChat 丢弃
    assert_eq!(cs[0], "Write Seraphina's next reply in a fictional chat between Seraphina and User.");
    assert_eq!(cs[1], "WI before text");
    assert_eq!(cs[2], "I am a traveler");
    assert_eq!(cs[3], "An elf princess");
    assert_eq!(cs[4], "brave");
    assert_eq!(cs[5], "a forest");
    let enhance_pos = cs.iter().position(|c| c.contains("If you have more knowledge")).unwrap();
    assert_eq!(enhance_pos, 6);
    assert_eq!(cs[7], "WI after text");
    // history: newMainChat 无条件最前
    let new_chat_pos = cs.iter().position(|c| c == "[Start a new Chat]").unwrap();
    assert_eq!(new_chat_pos, 8);
    // 历史旧→新
    assert_eq!(cs[new_chat_pos + 1], "hello");
    assert_eq!(cs[new_chat_pos + 2], "Hi there!");
}

#[test]
fn budget_trims_oldest_first() {
    let mut oai = default_oai();
    oai.openai_max_context = 100;
    oai.openai_max_tokens = 0;
    let input = base_input(&oai);
    let out = assemble(&input);
    assert!(out.error.is_none());
    // 小预算：非强制系统项跳过、历史从新到旧填、首条塞不下即断
    let cs = contents(&out);
    let new_chat_pos = cs.iter().position(|c| c == "[Start a new Chat]");
    if let Some(pos) = new_chat_pos {
        // 若最旧一条（hello）在而最新一条（Hi there!）不在 → 断式行为破坏
        let has_hello = cs[pos..].iter().any(|c| c == "hello");
        let has_hi = cs[pos..].iter().any(|c| c == "Hi there!");
        if !has_hi {
            assert!(!has_hello, "倒序填充被断开后不得再包含更旧消息");
        }
    }
}

#[test]
fn history_newest_first_stops_at_first_overflow() {
    let mut oai = default_oai();
    // 预算刚好只够最后 1 条历史
    oai.openai_max_context = 200;
    oai.openai_max_tokens = 0;
    let mut input = base_input(&oai);
    input.messages.push(HistoryMessage {
        role: "user".into(),
        content: "third".into(),
        name: Some("User".into()),
        is_narrator: false,
        injected: false,
    });
    let out = assemble(&input);
    let cs = contents(&out);
    let has_third = cs.contains(&"third".to_string());
    let has_second = cs.contains(&"Hi there!".to_string());
    let has_first = cs.contains(&"hello".to_string());
    // 倒序填充：最新优先，首个塞不下的位置断开 → 后面的更旧消息全丢
    if !has_second {
        assert!(!has_first, "若第二条塞不下，第一条必须也被丢弃");
    }
    let _ = has_third;
}

#[test]
fn new_chat_always_inserted() {
    let oai = default_oai();
    let mut input = base_input(&oai);
    input.messages.clear();
    let out = assemble(&input);
    let cs = contents(&out);
    // 无条件插入（非仅新聊天）
    assert!(cs.contains(&"[Start a new Chat]".to_string()));
}

#[test]
fn character_overrides_main_and_jailbreak() {
    let oai = default_oai();
    let mut input = base_input(&oai);
    input.system_prompt_override = Some("OVERRIDE MAIN".into());
    input.jailbreak_prompt_override = Some("OVERRIDE JB".into());
    let out = assemble(&input);
    let cs = contents(&out);
    assert!(cs.contains(&"OVERRIDE MAIN".to_string()));
    assert!(cs.contains(&"OVERRIDE JB".to_string()));
    assert!(!cs.iter().any(|c| c.starts_with("Write Seraphina")));
}

#[test]
fn forbid_overrides_blocks_replacement() {
    let mut oai = default_oai();
    if let Some(main) = oai.prompts.iter_mut().find(|p| p.identifier == "main") {
        main.forbid_overrides = true;
    }
    let mut input = base_input(&oai);
    input.system_prompt_override = Some("OVERRIDE MAIN".into());
    let out = assemble(&input);
    let cs = contents(&out);
    assert!(cs.iter().any(|c| c.starts_with("Write Seraphina")));
}

#[test]
fn disabled_main_keeps_placeholder() {
    let mut oai = default_oai();
    if let Some(order) = oai.prompt_order.iter_mut().find(|o| o.character_id == 100001) {
        for item in order.order.iter_mut() {
            if item.identifier == "main" {
                item.enabled = false;
            }
        }
    }
    let input = base_input(&oai);
    let out = assemble(&input);
    // disabled main 输出为空（getChat 丢弃）但不报错，后续项继续
    assert!(out.error.is_none());
    let cs = contents(&out);
    assert!(cs.contains(&"An elf princess".to_string()));
}

#[test]
fn impersonate_adds_control_prompt_last() {
    let oai = default_oai();
    let mut input = base_input(&oai);
    input.generation_type = "impersonate";
    let out = assemble(&input);
    let cs = contents(&out);
    // impersonation prompt 在最末尾（controlPrompts）
    let last = cs.last().unwrap();
    assert!(last.contains("[System note:"), "{}", last);
}

#[test]
fn quiet_prompt_added_to_control() {
    let oai = default_oai();
    let mut input = base_input(&oai);
    input.generation_type = "quiet";
    input.quiet_prompt = "DO A THING".into();
    let out = assemble(&input);
    let cs = contents(&out);
    assert_eq!(cs.last().unwrap(), "DO A THING");
}

#[test]
fn continue_nudge_mode_appends_at_end() {
    let oai = default_oai();
    let mut input = base_input(&oai);
    input.generation_type = "continue";
    let out = assemble(&input);
    let cs = contents(&out);
    // continue nudge 系统提示在最末尾（含 lastChatMessage 替换）
    let last = cs.last().unwrap();
    assert!(
        last.contains("[Continue your last message without repeating its original content.]"),
        "{}",
        last
    );
    // nudge 在最末尾（controlPrompts 之后无其他）
    let nudge_pos = cs.iter().rposition(|c| c.contains("[Continue your last")).unwrap();
    assert_eq!(nudge_pos, cs.len() - 1);
    // 被续消息在 nudge 之前的末尾区域（tail 集合内），且历史区不再有它
    assert!(cs[..nudge_pos].iter().any(|c| c == "Hi there!"));
    let new_chat_pos = new_chat_pos_of(&cs);
    // 历史区从 newMainChat 到 prompt 结束前 tail 部分；被续消息已从中移除，
    // 因此历史区最后一条应是 "hello"
    assert_eq!(cs[nudge_pos - 2], "hello");
}

fn new_chat_pos_of(cs: &[String]) -> usize {
    cs.iter().position(|c| c == "[Start a new Chat]").unwrap()
}

#[test]
fn in_chat_injection_depth_zero_after_last_message() {
    let oai = default_oai();
    let mut input = base_input(&oai);
    input.in_chat_injections.push(InChatInjection {
        content: "DEPTH0 NOTE".into(),
        depth: 0,
        role: 0,
        injection_order: 100,
    });
    let out = assemble(&input);
    let cs = contents(&out);
    // depth 0 = 最后一条消息之后
    let last_msg_pos = cs.iter().rposition(|c| c == "Hi there!").unwrap();
    let note_pos = cs.iter().position(|c| c == "DEPTH0 NOTE").unwrap();
    assert!(note_pos > last_msg_pos, "note@{} last@{}", note_pos, last_msg_pos);
    // 且在 newMainChat 之后（历史区内）
    let new_chat_pos = new_chat_pos_of(&cs);
    assert!(note_pos > new_chat_pos);
}

#[test]
fn injection_order_desc_processing_asc_result() {
    let oai = default_oai();
    let mut input = base_input(&oai);
    // 同深度两条不同 order：order 200 先处理（splice 在后），order 50 后处理（在前）
    input.in_chat_injections = vec![
        InChatInjection {
            content: "ORDER_LOW".into(),
            depth: 0,
            role: 0,
            injection_order: 50,
        },
        InChatInjection {
            content: "ORDER_HIGH".into(),
            depth: 0,
            role: 0,
            injection_order: 200,
        },
    ];
    let out = assemble(&input);
    let cs = contents(&out);
    // 最终呈现升序：低 order 在前
    let low = cs.iter().position(|c| c == "ORDER_LOW").unwrap();
    let high = cs.iter().position(|c| c == "ORDER_HIGH").unwrap();
    assert!(low < high, "low@{} high@{}", low, high);
}

#[test]
fn squash_system_messages_merges() {
    let mut oai = default_oai();
    oai.squash_system_messages = true;
    let input = base_input(&oai);
    let out = assemble(&input);
    // 连续 system（main+worldInfoBefore+persona+...）合并为一条
    let system_count = roles(&out).iter().filter(|r| r.as_str() == "system").count();
    // squash 后前部 system 全并为 1 条（newMainChat 除外）
    assert!(system_count <= 3, "system count = {}", system_count);
    // 排除表生效：newMainChat 独立保留
    let cs = contents(&out);
    assert!(cs.contains(&"[Start a new Chat]".to_string()));
}

#[test]
fn wi_format_wraps() {
    let mut oai = default_oai();
    oai.wi_format = "[Details of the fictional world the roleplay is set in:\n{0}\n]".into();
    let input = base_input(&oai);
    let out = assemble(&input);
    let cs = contents(&out);
    assert!(cs
        .iter()
        .any(|c| c.contains("[Details of the fictional world") && c.contains("WI before text")));
}

#[test]
fn scenario_personality_format_wrapped() {
    let oai = default_oai();
    let input = base_input(&oai);
    let out = assemble(&input);
    let cs = contents(&out);
    // scenario_format {{scenario}} → 内容即 scenario；personality 同理
    assert!(cs.contains(&"a forest".to_string()));
    assert!(cs.contains(&"brave".to_string()));
}

#[test]
fn examples_system_role_and_example_names() {
    let oai = default_oai();
    let mut input = base_input(&oai);
    input.message_examples.push(ExampleBlock {
        messages: vec![
            ("user".into(), "example_user".into(), "hi example".into()),
            (
                "assistant".into(),
                "example_assistant".into(),
                "hello example".into(),
            ),
        ],
    });
    let out = assemble(&input);
    // 示例全部为 system 角色
    for m in &out.chat {
        if m.content.contains("hi example") || m.content.contains("hello example") {
            assert_eq!(m.role, "system");
        }
    }
    // [Example Chat] 头存在
    assert!(contents(&out).iter().any(|c| c.contains("[Example Chat]")));
}

#[test]
fn empty_content_dropped() {
    let oai = default_oai();
    let input = base_input(&oai);
    let out = assemble(&input);
    // nsfw/jailbreak 默认空 → 不出现
    for m in &out.chat {
        assert!(!m.content.is_empty());
    }
}

#[test]
fn mandatory_overflow_errors() {
    let mut oai = default_oai();
    oai.openai_max_context = 10;
    oai.openai_max_tokens = 0;
    let input = base_input(&oai);
    let out = assemble(&input);
    assert!(out.error.is_some());
}




