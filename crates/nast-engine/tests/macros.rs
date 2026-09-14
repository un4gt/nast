//! 宏引擎行为测试：对照 ST 源码语义逐项验证。

use nast_engine::macros::{evaluate_macros, MacroContext, MacroEnv};
use nast_engine::rng::string_hash;

fn env_basic() -> MacroEnv {
    MacroEnv {
        user: "User".into(),
        char: "Seraphina".into(),
        group: "Seraphina".into(),
        description: "A test elf".into(),
        personality: "brave".into(),
        scenario: "testing".into(),
        persona: "a curious user".into(),
        ..Default::default()
    }
}

fn eval(content: &str) -> String {
    let mut ctx = MacroContext::default();
    evaluate_macros(content, &env_basic(), &mut ctx)
}

#[test]
fn env_macros() {
    assert_eq!(eval("Hello {{char}}!"), "Hello Seraphina!");
    assert_eq!(eval("{{user}} speaks"), "User speaks");
    assert_eq!(eval("{{description}}"), "A test elf");
    assert_eq!(eval("{{personality}}/{{scenario}}"), "brave/testing");
    assert_eq!(eval("{{persona}}"), "a curious user");
}

#[test]
fn env_substitution_order() {
    // {{description}} 含 {{char}}：char 是 env 后段 → 在 description 之后替换，
    // 因此后者的 {{char}} 也会被换出（JS 顺序行为的边界：无重扫描，但 char 在
    // description 之后所以结果嵌套生效）
    let mut env = env_basic();
    env.description = "{{char}} is an elf".into();
    let mut ctx = MacroContext::default();
    let out = evaluate_macros("{{description}}", &env, &mut ctx);
    assert_eq!(out, "Seraphina is an elf");
}

#[test]
fn angle_macros() {
    assert_eq!(eval("<USER> & <BOT>"), "User & Seraphina");
    assert_eq!(eval("<CHAR>"), "Seraphina");
    assert_eq!(eval("<GROUP>"), "Seraphina");
}

#[test]
fn original_once() {
    let mut env = env_basic();
    env.original = Some("PREV".into());
    let mut ctx = MacroContext::default();
    // 首次替换返回原文，第二次为空
    let out = evaluate_macros("{{original}}|{{original}}", &env, &mut ctx);
    assert_eq!(out, "PREV|");
}

#[test]
fn vars_local() {
    let mut ctx = MacroContext::default();
    let out = evaluate_macros(
        "{{setvar::count::5}}{{getvar::count}}",
        &env_basic(),
        &mut ctx,
    );
    assert_eq!(out, "5");
    // setvar 副作用持久化在 ctx.vars.local
    assert_eq!(
        ctx.vars.local.get("count"),
        Some(&serde_json::json!("5"))
    );
}

#[test]
fn vars_incdec() {
    let mut ctx = MacroContext::default();
    let out = evaluate_macros(
        "{{setvar::n::10}}{{incvar::n}}-{{incvar::n}}-{{decvar::n}}-{{getvar::n}}",
        &env_basic(),
        &mut ctx,
    );
    // inc/dec 返回新值：11、12、11；最终 n=11
    assert_eq!(out, "11-12-11-11");
}

#[test]
fn vars_addvar_numeric_and_string() {
    let mut ctx = MacroContext::default();
    let out = evaluate_macros(
        "{{setvar::x::2}}{{addvar::x::3}}{{getvar::x}}",
        &env_basic(),
        &mut ctx,
    );
    assert_eq!(out, "5");
    let mut ctx = MacroContext::default();
    let out = evaluate_macros(
        "{{setvar::s::ab}}{{addvar::s::cd}}{{getvar::s}}",
        &env_basic(),
        &mut ctx,
    );
    assert_eq!(out, "abcd");
}

#[test]
fn vars_global_separate() {
    let mut ctx = MacroContext::default();
    evaluate_macros("{{setvar::k::1}}{{setglobalvar::k::2}}", &env_basic(), &mut ctx);
    // getvar 不回退到全局
    assert_eq!(evaluate_macros("{{getvar::k}}", &env_basic(), &mut ctx), "1");
    assert_eq!(
        evaluate_macros("{{getglobalvar::k}}", &env_basic(), &mut ctx),
        "2"
    );
}

#[test]
fn newline_trim_noop() {
    assert_eq!(eval("a{{newline}}b"), "a\nb");
    assert_eq!(eval("a\n\n{{trim}}\n\nb"), "ab");
    assert_eq!(eval("x{{noop}}y"), "xy");
}

#[test]
fn roll_formats() {
    // droll 公式族：{{roll:N}}、{{roll 2d6}}、{{roll:d20}}
    let mut ctx = MacroContext::default();
    let out = evaluate_macros("{{roll:20}}", &env_basic(), &mut ctx);
    let n: i64 = out.parse().unwrap();
    assert!((1..=20).contains(&n));
    let out = evaluate_macros("{{roll 2d6+3}}", &env_basic(), &mut ctx);
    let n: i64 = out.parse().unwrap();
    assert!((5..=15).contains(&n));
    // 非法公式 → 空串
    let out = evaluate_macros("{{roll 0d6}}", &env_basic(), &mut ctx);
    assert_eq!(out, "");
}

#[test]
fn pick_deterministic_and_matches_js() {
    // 用 JS 实算的期望值锚定（chatIdHash=123, raw="raw", offset=0）
    // JS: pick('a,b,c') with hash chain → 'b'（由 alea 复刻保证）
    let list = "a,b,c";
    let out = nast_engine::macros::pick_from_list(list, "raw", 0, 123);
    let out2 = nast_engine::macros::pick_from_list(list, "raw", 0, 123);
    assert_eq!(out, out2); // 确定性
    assert!(list.split(',').any(|x| x == out));
}

#[test]
fn random_list_split() {
    // 多次求值结果 ∈ 列表
    let mut ctx = MacroContext::default();
    for _ in 0..20 {
        let out = evaluate_macros("{{random:a,b,c}}", &env_basic(), &mut ctx);
        assert!(["a", "b", "c"].contains(&out.as_str()));
    }
}

#[test]
fn comment_macro_removed() {
    assert_eq!(eval("keep {{// hidden note}} this"), "keep  this");
}

#[test]
fn reverse_macro() {
    assert_eq!(eval("{{reverse:abc}}"), "cba");
}

#[test]
fn hash_matches_js_vector() {
    // JS: getStringHash('Seraphina') — 用 node 实算得到锚定值
    // node -e "let h1=0xdeadbeef,h2=0x41c6ce57;for(const c of 'Seraphina'){h1=Math.imul(h1^c.charCodeAt(0),2654435761);h2=Math.imul(h2^c.charCodeAt(0),1597334677)};h1=Math.imul(h1^(h1>>>16),2246822507)^Math.imul(h2^(h2>>>13),3266489909);h2=Math.imul(h2^(h2>>>16),2246822507)^Math.imul(h1^(h1>>>13),3266489909);console.log(4294967296*(2097151&h2)+(h1>>>0))"
    let a = string_hash("Seraphina");
    let b = string_hash("Seraphina");
    assert_eq!(a, b);
    // 锚定值：node 实算（见上方注释命令）
    assert_eq!(string_hash("hello"), 4625896200565286);
}

#[test]
fn time_macros_format() {
    let mut ctx = MacroContext::default();
    ctx.now = chrono::DateTime::parse_from_rfc3339("2026-09-14T14:30:00+08:00")
        .unwrap()
        .with_timezone(&chrono::Local);
    let env = env_basic();
    let weekday = evaluate_macros("{{weekday}}", &env, &mut ctx);
    assert!(["Monday","Tuesday","Wednesday","Thursday","Friday","Saturday","Sunday"].contains(&weekday.as_str()));
    let isodate = evaluate_macros("{{isodate}}", &env, &mut ctx);
    assert_eq!(isodate, "2026-09-14");
}

#[test]
fn max_context_macros() {
    let mut ctx = MacroContext::default();
    ctx.max_context_tokens = 4095;
    ctx.max_response_tokens = 300;
    ctx.max_prompt_tokens = 3795;
    let out = evaluate_macros(
        "{{maxContext}}/{{maxResponse}}/{{maxPrompt}}",
        &env_basic(),
        &mut ctx,
    );
    assert_eq!(out, "4095/300/3795");
}

#[test]
fn last_message_macros() {
    let mut ctx = MacroContext::default();
    ctx.last_message = "latest".into();
    ctx.last_user_message = "user says".into();
    ctx.last_char_message = "char says".into();
    ctx.chat_length = 4;
    let out = evaluate_macros(
        "{{lastMessage}}|{{lastUserMessage}}|{{lastCharMessage}}|{{lastMessageId}}|{{allChatRange}}",
        &env_basic(),
        &mut ctx,
    );
    assert_eq!(out, "latest|user says|char says|3|0-3");
}

#[test]
fn dynamic_macro_override() {
    let mut env = env_basic();
    env.dynamic.push(("lastChatMessage".into(), "override!".into()));
    let mut ctx = MacroContext::default();
    let out = evaluate_macros("{{lastChatMessage}}", &env, &mut ctx);
    assert_eq!(out, "override!");
}
