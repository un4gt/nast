//! 世界书引擎行为测试：对照 world-info.js 语义逐项验证。

use nast_engine::macros::MacroEnv;
use nast_engine::world_info::{
    check_world_info, ScanSource, WiSettings, WiState, WiBooks,
};
use nast_model::world::{TimedWorldInfo, WIEntry, WorldInfoBook};
use serde_json::json;

fn entry(uid: i64, key: &[&str], content: &str) -> WIEntry {
    WIEntry {
        uid,
        key: key.iter().map(|s| s.to_string()).collect(),
        content: content.to_string(),
        ..Default::default()
    }
}


fn book_of(e: WIEntry) -> WorldInfoBook {
    book(e)
}

fn clone_entry(b: &WorldInfoBook, uid: &str) -> WIEntry {
    b.entries[uid].clone()
}

fn book(entries: WIEntry) -> WorldInfoBook {
    let mut b = WorldInfoBook::default();
    b.entries.insert(entries.uid.to_string(), entries);
    b
}

fn scan(chat: &[&str]) -> ScanSource {
    ScanSource {
        chat: chat.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    }
}

fn run(
    chat_book: Option<WorldInfoBook>,
    chat: &[&str],
    settings: &WiSettings,
    timed: &mut TimedWorldInfo,
    chat_length: i64,
    max_context: i64,
) -> nast_engine::world_info::WiResult {
    let empty: Vec<WorldInfoBook> = vec![];
    let books = WiBooks {
        chat_lore: chat_book.iter().collect(),
        persona_lore: empty.iter().collect(),
        global_lore: empty.iter().collect(),
        character_lore: empty.iter().collect(),
    };
    let source = scan(chat);
    let mut state = WiState { timed, chat_length };
    let env = MacroEnv::default();
    check_world_info(&books, settings, &source, &mut state, &env, max_context)
}

#[test]
fn basic_activation_and_order_desc() {
    let settings = WiSettings::default();
    let mut timed = TimedWorldInfo::default();
    // 两个条目：order 100 与 order 50 都匹配
    let mut b = WorldInfoBook::default();
    b.entries.insert("0".into(), entry(0, &["dragon"], "DRAGON high order"));
    b.entries.insert("1".into(), entry(1, &["dragon"], "DRAGON low order"));
    b.entries.get_mut("0").unwrap().order = 100;
    b.entries.get_mut("1").unwrap().order = 50;

    let r = run(Some(b), &["a dragon appears"], &settings, &mut timed, 1, 4096);
    // before 块：迭代 order 降序，unshift 翻转 → 升序（low 在前，high 邻近角色描述）
    assert!(r.world_info_before.starts_with("DRAGON low order"));
    assert!(r.world_info_before.ends_with("DRAGON high order"));
    assert_eq!(r.activated_count, 2);
}

#[test]
fn secondary_logic_not_any() {
    let settings = WiSettings::default();
    let mut timed = TimedWorldInfo::default();
    let mut e = entry(0, &["sword"], "SWORD ENTRY");
    e.selective = true;
    e.keysecondary = vec!["forbidden".into()];
    e.selective_logic = 2; // NOT_ANY
    let b = book(e);

    // 只有主 key → 激活
    let r = run(Some(book(b.entries["0"].clone())), &["a sword"], &settings, &mut timed, 1, 4096);
    assert_eq!(r.activated_count, 1);
    // 主 key + secondary 出现 → 抑制
    let r = run(
        Some(book_of(b.entries["0"].clone())),
        &["a sword is forbidden"],
        &settings,
        &mut timed,
        1,
        4096,
    );
    assert_eq!(r.activated_count, 0);
}

#[test]
fn regex_key_overrides_case() {
    let settings = WiSettings::default();
    let mut timed = TimedWorldInfo::default();
    // 大小写敏感的全局设置 + /regex/ key 强制大小写
    let mut s = settings.clone();
    s.case_sensitive = false;
    let mut e = entry(0, &[r"/DRAG[Oo]N/"], "REGEX ENTRY");
    e.case_sensitive = Some(true); // regex key 本身已带大写，测试覆盖全局 ignore-case
    let b = book(e);
    let r = run(Some(b), &["a DrAgOn"], &s, &mut timed, 1, 4096);
    // regex key：DRAG[Oo]N 不匹配 "DrAgOn"（a 小写）
    assert_eq!(r.activated_count, 0);
}

#[test]
fn wildcard_is_literal() {
    // GOTCHA #6：* 无通配语义
    let settings = WiSettings::default();
    let mut timed = TimedWorldInfo::default();
    let e = entry(0, &["dra*gon"], "WILDCARD ENTRY");
    let r = run(Some(book_of(e.clone())), &["draAAAgon"], &settings, &mut timed, 1, 4096);
    assert_eq!(r.activated_count, 0);
    let r = run(Some(book_of(e)), &["literal dra*gon here"], &settings, &mut timed, 1, 4096);
    assert_eq!(r.activated_count, 1);
}

#[test]
fn recursion_chain() {
    let mut s = WiSettings::default();
    s.recursive = true;
    let mut timed = TimedWorldInfo::default();
    // A 的内容包含 B 的 key
    let mut b = WorldInfoBook::default();
    b.entries.insert("0".into(), entry(0, &["alpha"], "mentions beta"));
    b.entries.insert("1".into(), entry(1, &["beta"], "BETA RECURSED"));
    let r = run(Some(b), &["alpha starts"], &s, &mut timed, 1, 4096);
    assert_eq!(r.activated_count, 2);
}

#[test]
fn no_recursion_by_default() {
    let settings = WiSettings::default(); // recursive = false
    let mut timed = TimedWorldInfo::default();
    let mut b = WorldInfoBook::default();
    b.entries.insert("0".into(), entry(0, &["alpha"], "mentions beta"));
    b.entries.insert("1".into(), entry(1, &["beta"], "BETA RECURSED"));
    let r = run(Some(b), &["alpha starts"], &settings, &mut timed, 1, 4096);
    assert_eq!(r.activated_count, 1);
}

#[test]
fn prevent_recursion_blocks_entry_content() {
    let mut s = WiSettings::default();
    s.recursive = true;
    let mut timed = TimedWorldInfo::default();
    let mut b = WorldInfoBook::default();
    let mut a = entry(0, &["alpha"], "mentions beta");
    a.prevent_recursion = true; // A 的内容不进递归扫描
    b.entries.insert("0".into(), a);
    b.entries.insert("1".into(), entry(1, &["beta"], "BETA RECURSED"));
    let r = run(Some(b), &["alpha starts"], &s, &mut timed, 1, 4096);
    assert_eq!(r.activated_count, 1);
}

#[test]
fn budget_drops_lower_priority() {
    let mut s = WiSettings::default();
    s.budget = 1; // 1% × maxContext —— 极小
    let mut timed = TimedWorldInfo::default();
    let mut b = WorldInfoBook::default();
    let mut hi = entry(0, &["dragon"], "HIGH PRIORITY");
    hi.order = 100;
    let mut lo = entry(1, &["dragon"], "LOW PRIORITY");
    lo.order = 50;
    b.entries.insert("0".into(), hi);
    b.entries.insert("1".into(), lo);

    // HIGH costs about 4 tokens (content plus newline); budget only fits HIGH, LOW must be dropped
    let max_context = 600; // budget = round(1% x 600) = 6 tokens: HIGH(4) fits, LOW(+4=8>6) drops
    let r = run(Some(b), &["a dragon"], &s, &mut timed, 1, max_context);
    // HIGH（迭代序靠前）存活，LOW 因超预算被丢
    assert!(r.world_info_before.contains("HIGH PRIORITY"));
    assert!(!r.world_info_before.contains("LOW PRIORITY"));
    assert!(r.budget_overflowed);
}

#[test]
fn ignore_budget_bypasses() {
    let mut s = WiSettings::default();
    s.budget = 1;
    let mut timed = TimedWorldInfo::default();
    let mut b = WorldInfoBook::default();
    let mut hi = entry(0, &["dragon"], "HIGH PRIORITY");
    hi.order = 100;
    let mut lo = entry(1, &["dragon"], "IGNORED BUDGET ENTRY");
    lo.order = 50;
    lo.ignore_budget = true;
    b.entries.insert("0".into(), hi);
    b.entries.insert("1".into(), lo);

    let r = run(Some(b), &["a dragon"], &s, &mut timed, 1, 1000);
    assert!(r.world_info_before.contains("IGNORED BUDGET ENTRY"));
}

#[test]
fn sticky_last_one_turn() {
    let mut s = WiSettings::default();
    s.recursive = true;
    let mut timed = TimedWorldInfo::default();
    let mut b = WorldInfoBook::default();
    let mut e = entry(0, &["alpha"], "STICKY ENTRY");
    e.sticky = 2; // 激活后 2 个消息长度内持续
    b.entries.insert("0".into(), e);

    // 第 1 轮：key 命中激活，记录 sticky {start:1, end:3, protected}
    let r = run(Some(book(b.entries["0"].clone())), &["alpha"], &s, &mut timed, 1, 4096);
    assert_eq!(r.activated_count, 1);
    assert!(timed.sticky.contains_key("0.0"));
    assert_eq!(timed.sticky["0.0"].end, 3);

    // 第 2 轮：chat.length=2 < end=3 → 聊天里无关键词也持续激活（entry 不变，hash 一致）
    let r = run(Some(book(b.entries["0"].clone())), &["nothing relevant"], &s, &mut timed, 2, 4096);
    assert_eq!(r.activated_count, 1);

    // 第 3 轮：chat.length=3 = end → sticky 到期，无关键词不再激活
    let r = run(Some(book(b.entries["0"].clone())), &["nothing relevant"], &s, &mut timed, 3, 4096);
    assert_eq!(r.activated_count, 0);

    // hash 失效：entry 内容变化 → sticky 记录作废
    let mut e3 = b.entries["0"].clone();
    e3.content = "CHANGED CONTENT".to_string();
    let r = run(Some(book_of(e3)), &["nothing relevant"], &s, &mut timed, 2, 4096);
    assert_eq!(r.activated_count, 0);
}

#[test]
fn cooldown_suppresses_reentry() {
    let settings = WiSettings::default();
    let mut timed = TimedWorldInfo::default();
    let mut e = entry(0, &["alpha"], "COOLDOWN ENTRY");
    e.cooldown = 3;
    let b = book(e);

    // 首次激活（记录 cooldown）
    let r = run(Some(book_of(b.entries["0"].clone())), &["alpha"], &settings, &mut timed, 1, 4096);
    assert_eq!(r.activated_count, 1);
    assert!(timed.cooldown.contains_key("0.0"));
    // cooldown 窗口内（chat.length < end）即使 key 命中也抑制
    let r = run(Some(book_of(b.entries["0"].clone())), &["alpha again"], &settings, &mut timed, 2, 4096);
    assert_eq!(r.activated_count, 0);
    // 超出窗口后恢复
    let r = run(Some(book_of(b.entries["0"].clone())), &["alpha again"], &settings, &mut timed, 10, 4096);
    assert_eq!(r.activated_count, 1);
}

#[test]
fn delay_blocks_early() {
    let settings = WiSettings::default();
    let mut timed = TimedWorldInfo::default();
    let mut e = entry(0, &["alpha"], "DELAYED ENTRY");
    e.delay = 5;
    let b = book(e);
    // chat.length=2 < delay=5 → 抑制
    let r = run(Some(book_of(b.entries["0"].clone())), &["alpha"], &settings, &mut timed, 2, 4096);
    assert_eq!(r.activated_count, 0);
    let r = run(Some(book_of(b.entries["0"].clone())), &["alpha"], &settings, &mut timed, 6, 4096);
    assert_eq!(r.activated_count, 1);
}

#[test]
fn at_d_position_creates_depth_entry() {
    let settings = WiSettings::default();
    let mut timed = TimedWorldInfo::default();
    let mut e = entry(0, &["alpha"], "AT DEPTH ENTRY");
    e.position = nast_model::world::WIPosition::Text("@D2[a]".into());
    let b = book(e);
    let r = run(Some(b), &["alpha"], &settings, &mut timed, 1, 4096);
    assert_eq!(r.depth_entries.len(), 1);
    assert_eq!(r.depth_entries[0].depth, 2);
    assert_eq!(r.depth_entries[0].role, 2); // assistant
    // 不进 before/after 块
    assert!(r.world_info_before.is_empty());
}

#[test]
fn group_override_wins() {
    let settings = WiSettings::default();
    let mut timed = TimedWorldInfo::default();
    let mut b = WorldInfoBook::default();
    let mut a = entry(0, &["alpha"], "GROUP A");
    a.group = "shared".into();
    a.order = 50;
    let mut o = entry(1, &["alpha"], "GROUP OVERRIDE");
    o.group = "shared".into();
    o.order = 100;
    o.group_override = true;
    b.entries.insert("0".into(), a);
    b.entries.insert("1".into(), o);
    let r = run(Some(b), &["alpha"], &settings, &mut timed, 1, 4096);
    // override 者胜出
    assert_eq!(r.activated_count, 1);
    assert!(r.world_info_before.contains("GROUP OVERRIDE"));
}

#[test]
fn probability_zero_never() {
    let settings = WiSettings::default();
    let mut timed = TimedWorldInfo::default();
    let mut e = entry(0, &["alpha"], "NEVER");
    e.probability = 0;
    e.use_probability = true;
    let b = book(e);
    let r = run(Some(b), &["alpha"], &settings, &mut timed, 1, 4096);
    assert_eq!(r.activated_count, 0);
}

#[test]
fn constant_entry_needs_no_key() {
    let settings = WiSettings::default();
    let mut timed = TimedWorldInfo::default();
    let mut e = entry(0, &[], "ALWAYS ON");
    e.constant = true;
    let b = book(e);
    let r = run(Some(b), &["random text"], &settings, &mut timed, 1, 4096);
    assert_eq!(r.activated_count, 1);
}

#[test]
fn macros_substituted_at_activation() {
    let settings = WiSettings::default();
    let mut timed = TimedWorldInfo::default();
    let env = MacroEnv {
        char: "Seraphina".into(),
        user: "User".into(),
        group: "Seraphina".into(),
        ..Default::default()
    };
    let empty: Vec<WorldInfoBook> = vec![];
    let b = book(entry(0, &["alpha"], "Hello {{char}}, welcome {{user}}"));
    let books = WiBooks {
        chat_lore: std::iter::once(&b).collect(),
        persona_lore: empty.iter().collect(),
        global_lore: empty.iter().collect(),
        character_lore: empty.iter().collect(),
    };
    let source = scan(&["alpha"]);
    let mut state = WiState { timed: &mut timed, chat_length: 1 };
    let r = check_world_info(&books, &settings, &source, &mut state, &env, 4096);
    assert!(r.world_info_before.contains("Hello Seraphina, welcome User"));
}

#[test]
fn scan_depth_limits_buffer() {
    let mut s = WiSettings::default();
    s.depth = 2; // 只看最后 2 条
    let mut timed = TimedWorldInfo::default();
    let b = book(entry(0, &["deepkey"], "DEEP ENTRY"));
    // deepkey 在最早的消息里（超出深度）
    let r = run(Some(book_of(b.entries["0"].clone())), &["deepkey", "m1", "m2", "m3"], &s, &mut timed, 4, 4096);
    assert_eq!(r.activated_count, 0);
    // 在最新消息里
    let r = run(Some(book_of(b.entries["0"].clone())), &["m1", "m2", "m3", "deepkey"], &s, &mut timed, 4, 4096);
    assert_eq!(r.activated_count, 1);
}

#[test]
fn match_character_description_source() {
    let settings = WiSettings::default();
    let mut timed = TimedWorldInfo::default();
    let mut e = entry(0, &["desckey"], "FROM DESCRIPTION");
    e.match_character_description = true;
    let b = book(e);
    // chat 里没有 desckey，但 char_description 有
    let mut source = scan(&["no key here"]);
    source.char_description = "her name has desckey inside".into();
    let empty: Vec<WorldInfoBook> = vec![];
    let books = WiBooks {
        chat_lore: std::iter::once(&b).collect(),
        persona_lore: empty.iter().collect(),
        global_lore: empty.iter().collect(),
        character_lore: empty.iter().collect(),
    };
    let mut state = WiState { timed: &mut timed, chat_length: 1 };
    let env = MacroEnv::default();
    let r = check_world_info(&books, &settings, &source, &mut state, &env, 4096);
    assert_eq!(r.activated_count, 1);
}

#[test]
fn disable_entry_skipped() {
    let settings = WiSettings::default();
    let mut timed = TimedWorldInfo::default();
    let mut e = entry(0, &["alpha"], "DISABLED");
    e.disable = true;
    let b = book(e);
    let r = run(Some(book_of(b.entries["0"].clone())), &["alpha"], &settings, &mut timed, 1, 4096);
    assert_eq!(r.activated_count, 0);
}

#[test]
fn json_roundtrip_of_entry() {
    // ST 文件兼容：从 json 对象解析
    let v = json!({
        "uid": 3,
        "key": ["k1", "k2"],
        "keysecondary": [],
        "comment": "test",
        "content": "content here",
        "constant": false,
        "vectorized": false,
        "selective": true,
        "selectiveLogic": 0,
        "addMemo": true,
        "order": 100,
        "position": 0,
        "disable": false,
        "excludeRecursion": false,
        "preventRecursion": false,
        "delayUntilRecursion": false,
        "probability": 100,
        "useProbability": true,
        "depth": 4,
        "group": "",
        "groupOverride": false,
        "groupWeight": 100,
        "scanDepth": null,
        "caseSensitive": null,
        "matchWholeWords": null,
        "useGroupScoring": null,
        "automationId": "",
        "role": null,
        "sticky": 0,
        "cooldown": 0,
        "delay": 0,
        "matchPersonaDescription": false,
        "matchCharacterDescription": false,
        "matchCharacterPersonality": false,
        "matchCharacterDepthPrompt": false,
        "matchScenario": false,
        "matchCreatorNotes": false,
        "ignoreBudget": false
    });
    let e: WIEntry = serde_json::from_value(v).unwrap();
    assert_eq!(e.uid, 3);
    assert_eq!(e.key.len(), 2);
}





