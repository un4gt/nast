//! fixture 回归：真实 ST 文件必须原样解析。
use nast_model::*;

#[test]
fn parse_default_openai_preset() {
    let raw = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/openai_default.json"
    ))
    .unwrap();
    let preset: preset::OaiSettings = serde_json::from_str(&raw).unwrap();
    // Default.json: prompt_order 双列表 100000 + 100001
    let ids: Vec<i64> = preset.prompt_order.iter().map(|o| o.character_id).collect();
    assert_eq!(ids, vec![preset::TC_DUMMY_ID, preset::CC_DUMMY_ID]);
    let cc = preset
        .prompt_order
        .iter()
        .find(|o| o.character_id == preset::CC_DUMMY_ID)
        .unwrap();
    // 100001 含 personaDescription 且第一项是 main
    assert_eq!(cc.order[0].identifier, "main");
    assert!(cc.order.iter().any(|o| o.identifier == preset::ID_PERSONA));
    // 12 个 prompt 项
    assert_eq!(preset.prompts.len(), 12);
    assert!(preset.prompts.iter().any(|p| p.identifier == "main" && !p.marker));
    assert!(preset
        .prompts
        .iter()
        .any(|p| p.identifier == preset::ID_CHAT_HISTORY && p.marker));
}

#[test]
fn parse_eldoria_world_book() {
    let raw = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/world_eldoria.json"
    ))
    .unwrap();
    let book: world::WorldInfoBook = serde_json::from_str(&raw).unwrap();
    assert!(!book.entries.is_empty());
    let first = book.entries.values().next().unwrap();
    assert_eq!(first.uid, 0);
    assert!(first.key.iter().any(|k| k == "eldoria"));
    // roundtrip 必须无损（serde(default) + flatten 兼容）
    let out = serde_json::to_string(&book).unwrap();
    let book2: world::WorldInfoBook = serde_json::from_str(&out).unwrap();
    assert_eq!(book2.entries.len(), book.entries.len());
}

#[test]
fn wi_position_at_d_syntax() {
    use world::WIPosition;
    let (p, o) = WIPosition::Text("@D3".into()).resolve();
    assert_eq!(p, world::WI_POS_AT_DEPTH);
    assert_eq!(o, Some((3, None)));
    let (p, o) = WIPosition::Text("@D2[a]".into()).resolve();
    assert_eq!(o, Some((2, Some(2)))); // assistant
    let (p, o) = WIPosition::Text("@D2[r]".into()).resolve();
    assert_eq!(o, Some((2, Some(1)))); // user
    let (p, _) = WIPosition::Num(1).resolve();
    assert_eq!(p, world::WI_POS_AFTER);
}

#[test]
fn parse_v1_card_synthesizes_data() {
    let v1 = serde_json::json!({
        "name": "Test",
        "description": "A test char",
        "personality": "brave",
        "scenario": "testing",
        "first_mes": "hello",
        "mes_example": "<START>\n{{user}}: hi\n{{char}}: hello",
        "talkativeness": "0.7"
    });
    let ch = card::Character::from_card_json(&v1).unwrap();
    assert_eq!(ch.data.name, "Test");
    assert_eq!(ch.data.description, "A test char");
    // 合成的 extensions：talkativeness 保留字符串、depth_prompt 默认
    assert_eq!(ch.data.extensions.talkativeness.as_deref(), Some("0.7"));
    let dp = ch.data.extensions.depth_prompt.as_ref().unwrap();
    assert_eq!(dp.depth, 4);
    assert_eq!(dp.role, "system");
    // V3 必填组
    assert_eq!(ch.data.group_only_greetings.as_ref().unwrap().len(), 0);
}

#[test]
fn parse_v2_card_full() {
    let v2 = serde_json::json!({
        "spec": "chara_card_v2",
        "spec_version": "2.0",
        "name": "Seraphina",
        "description": "desc",
        "data": {
            "name": "Seraphina",
            "description": "desc",
            "character_version": "1.0",
            "first_mes": "greet",
            "alternate_greetings": ["alt1"],
            "extensions": {
                "talkativeness": "0.5",
                "world": "Eldoria",
                "depth_prompt": {"prompt": "be nice", "depth": 4, "role": "system"}
            },
            "external_field": {"unknown": true}
        }
    });
    let ch = card::Character::from_card_json(&v2).unwrap();
    assert_eq!(ch.data.extensions.world.as_deref(), Some("Eldoria"));
    assert_eq!(
        ch.data.extensions.depth_prompt.as_ref().unwrap().prompt,
        "be nice"
    );
    // 未知外部键保留
    assert!(ch
        .data
        .extra
        .contains_key("external_field"));
}

#[test]
fn message_shape_roundtrip() {
    let msg = chat::ChatMessage {
        name: "Seraphina".into(),
        is_user: false,
        is_system: false,
        send_date: "2026-09-14T08:00:00.000Z".into(),
        mes: "hi".into(),
        swipes: Some(vec!["hi".into()]),
        swipe_id: Some(0),
        swipe_info: Some(vec![chat::SwipeInfo::default()]),
        extra: chat::MessageExtra {
            api: Some("openai".into()),
            reasoning: Some(String::new()),
            ..Default::default()
        },
        ..Default::default()
    };
    let s = serde_json::to_string(&msg).unwrap();
    let back: chat::ChatMessage = serde_json::from_str(&s).unwrap();
    assert_eq!(back.extra.api.as_deref(), Some("openai"));
}

#[test]
fn chat_header_defaults_unused() {
    let h: chat::ChatHeader =
        serde_json::from_str(r#"{"chat_metadata": {"world": "Eldoria"}}"#).unwrap();
    assert_eq!(h.user_name, "unused");
    assert_eq!(h.character_name, "unused");
    assert_eq!(h.chat_metadata.world.as_deref(), Some("Eldoria"));
}
