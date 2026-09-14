//! 宏引擎：逐位复刻 ST 1.18.0 默认（legacy）evaluateMacros 的多 pass 有序替换。
//!
//! 求值顺序（macros.js evaluateMacros）：
//! 1. preEnv：尖括号宏、{{roll}}、instruct 系、变量系、{{newline}}/{{trim}}/{{noop}}/{{input}}
//! 2. env：环境变量按 environment 构造序逐条替换（含 {{original}} 一次性语义）
//! 3. postEnv：max* 系、lastMessage 系、注释、时间系、{{random}}/{{pick}} 等
//!
//! 与 JS 的差异说明：JS 用 /gi 正则逐条 replace；这里用 regex crate (?i) +
//! captures 迭代替换，语义一致。变量副作用（setvar 等）在替换时立即生效。

use crate::rng;
use serde_json::{Map, Value};

/// 宏环境（substituteParamsLegacy 的 environment 对象）。
#[derive(Debug, Clone, Default)]
pub struct MacroEnv {
    // 角色卡字段
    pub char_prompt: String,
    pub char_instruction: String,
    pub description: String,
    pub personality: String,
    pub scenario: String,
    pub persona: String,
    pub mes_examples_raw: String,
    pub char_version: String,
    pub char_depth_prompt: String,
    pub creator_notes: String,

    // 名字类
    pub user: String,
    pub char: String,
    pub group: String,
    pub group_not_muted: String,
    pub not_char: String,
    pub model: String,

    /// {{original}}：首次替换返回原文，其后为空
    pub original: Option<String>,

    /// 附加动态宏（additionalMacro / group override 等）
    pub dynamic: Vec<(String, String)>,
}

/// 变量存储（chat_metadata.variables / 全局变量，替换时立即回写）。
#[derive(Debug, Clone, Default)]
pub struct VarStore {
    pub local: Map<String, Value>,
    pub global: Map<String, Value>,
}

/// 求值上下文（可变状态集中在 RefCell 中以便规则闭包访问）。
pub struct MacroContext {
    pub vars: VarStore,
    pub chat_id_hash: i64,
    pub input: String,
    pub now: chrono::DateTime<chrono::Local>,
    pub last_user_message_at: Option<chrono::DateTime<chrono::Local>>,
    pub max_prompt_tokens: i64,
    pub max_context_tokens: i64,
    pub max_response_tokens: i64,
    pub last_message: String,
    pub last_user_message: String,
    pub last_char_message: String,
    pub first_included_message_id: Option<i64>,
    pub last_swipe_id: Option<i64>,
    pub current_swipe_id: Option<i64>,
    pub chat_length: usize,
    pub outlets: Map<String, Value>,
    pub banned: Vec<String>,
}

impl Default for MacroContext {
    fn default() -> Self {
        Self {
            vars: Default::default(),
            chat_id_hash: 0,
            input: String::new(),
            now: chrono::Local::now(),
            last_user_message_at: None,
            max_prompt_tokens: 0,
            max_context_tokens: 0,
            max_response_tokens: 0,
            last_message: String::new(),
            last_user_message: String::new(),
            last_char_message: String::new(),
            first_included_message_id: None,
            last_swipe_id: None,
            current_swipe_id: None,
            chat_length: 0,
            outlets: Default::default(),
            banned: vec![],
        }
    }
}

// ---------- 变量操作（variables.js 语义） ----------


/// 取捕获组（Option<&Option<String>> → String）。
fn cap(caps: &[Option<String>], i: usize) -> String {
    caps.get(i).cloned().flatten().unwrap_or_default()
}

fn get_var(map: &Map<String, Value>, name: &str) -> String {
    match map.get(name) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => {
            // JS String(number) 语义：整数值 double 不带小数点
            let f = n.as_f64().unwrap_or(0.0);
            if f.fract() == 0.0 && f.abs() < 1e15 && n.as_i64().is_none() {
                format!("{}", f as i64)
            } else {
                n.to_string()
            }
        }
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

fn set_var(map: &mut Map<String, Value>, name: &str, value: &str) {
    map.insert(name.to_string(), Value::String(value.to_string()));
}

fn add_var(map: &mut Map<String, Value>, name: &str, value: &str) -> String {
    let current = get_var(map, name);
    // 1. 当前值是 JSON 数组 → push value
    if let Ok(Value::Array(mut arr)) = serde_json::from_str::<Value>(&current) {
        arr.push(Value::String(value.to_string()));
        let serialized = serde_json::to_string(&arr).unwrap_or_default();
        map.insert(name.to_string(), Value::String(serialized.clone()));
        return serialized;
    }
    // 2. 数值相加（任一 NaN → 字符串拼接）
    let increment: f64 = value.trim().parse().unwrap_or(f64::NAN);
    let current_num: f64 = current.parse().unwrap_or(f64::NAN);
    if increment.is_nan() || current_num.is_nan() {
        let s = format!("{current}{value}");
        set_var(map, name, &s);
        return s;
    }
    let new_value = current_num + increment;
    if new_value.is_nan() {
        return String::new();
    }
    map.insert(
        name.to_string(),
        Value::Number(serde_json::Number::from_f64(new_value).unwrap_or(0.into())),
    );
    // JS 数字字符串化：整数不带小数点（{{incvar}} 返回值即此形态）
    if new_value.fract() == 0.0 && new_value.abs() < 1e15 {
        format!("{}", new_value as i64)
    } else {
        format!("{new_value}")
    }
}

// ---------- droll 0.2.1：{{roll ...}} ----------

pub fn roll_dice(formula: &str, rand: impl Fn() -> f64) -> Option<i64> {
    let f = formula.trim();
    // 纯数字 → 1dN
    let f = if !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()) {
        format!("1d{f}")
    } else {
        f.to_ascii_lowercase()
    };
    let lower = f.to_ascii_lowercase();
    let dpos = lower.find('d')?;
    let (num_s, rest) = (&lower[..dpos], &lower[dpos + 1..]);
    // numDice: [1-9]\d* 或缺省 1
    let num_dice: i64 = if num_s.is_empty() {
        1
    } else if num_s.bytes().all(|b| b.is_ascii_digit()) && !num_s.starts_with('0') {
        num_s.parse().ok()?
    } else {
        return None;
    };
    // numSides: [1-9]\d*
    let (sides_s, modifier) = match rest.find(['+', '-']) {
        Some(pos) => (&rest[..pos], rest[pos..].parse::<i64>().ok()?),
        None => (rest, 0i64),
    };
    if sides_s.is_empty()
        || !sides_s.bytes().all(|b| b.is_ascii_digit())
        || sides_s.starts_with('0')
    {
        return None;
    }
    let num_sides: i64 = sides_s.parse().ok()?;
    let mut total = modifier;
    for _ in 0..num_dice {
        total += 1 + (rand() * num_sides as f64).floor() as i64;
    }
    Some(total)
}

// ---------- random/pick 列表分割 ----------

fn split_list(list_string: &str) -> Vec<String> {
    if list_string.contains("::") {
        list_string.split("::").map(|s| s.to_string()).collect()
    } else {
        const PH: char = '\u{FFFC}';
        list_string
            .replace("\\,", &PH.to_string())
            .split(',')
            .map(|item| item.trim().replace(PH, ","))
            .collect()
    }
}

/// {{pick}}：seed = getStringHash(chatIdHash-rawContentHash-offset)。
pub fn pick_from_list(list: &str, raw_content: &str, offset: usize, chat_id_hash: i64) -> String {
    let items = split_list(list);
    if items.is_empty() {
        return String::new();
    }
    let raw_hash = rng::string_hash(raw_content);
    let combined = format!("{chat_id_hash}-{raw_hash}-{offset}");
    let final_seed = rng::string_hash(&combined);
    let mut prng = rng::Alea::new(&final_seed.to_string());
    let idx = (prng.next_f64() * items.len() as f64).floor() as usize;
    items.into_iter().nth(idx).unwrap_or_default()
}

// ---------- 时间 ----------

fn moment_lt(now: &chrono::DateTime<chrono::Local>) -> String {
    // en locale LT："h:mm A"（不补零小时）
    now.format("%-I:%M %p").to_string()
}

fn moment_ll(now: &chrono::DateTime<chrono::Local>) -> String {
    // en locale LL："MMMM D, YYYY"
    now.format("%B %-d, %Y").to_string()
}

fn idle_duration(ctx_now: &chrono::DateTime<chrono::Local>, last: Option<&chrono::DateTime<chrono::Local>>) -> String {
    // moment.humanize()：无精确复刻，提供近似的英文 humanized 输出
    match last {
        None => "just now".to_string(),
        Some(t) => {
            let secs = (*ctx_now - *t).num_seconds();
            let mins = (secs as f64 / 60.0).round() as i64;
            let humanized = match () {
                _ if mins < 1 => "a few seconds".to_string(),
                _ if mins < 2 => "a minute".to_string(),
                _ if mins < 45 => format!("{mins} minutes"),
                _ if mins < 90 => "an hour".to_string(),
                _ if mins < 60 * 24 => format!("{} hours", mins / 60),
                _ if mins < 60 * 36 => "a day".to_string(),
                _ if mins < 60 * 24 * 26 => format!("{} days", mins / (60 * 24)),
                _ if mins < 60 * 24 * 345 => format!("{} months", mins / (60 * 24 * 30)),
                _ if mins < 60 * 24 * 545 => "a year".to_string(),
                _ => format!("{} years", mins / (60 * 24 * 365)),
            };
            if secs < 45 { "just now".to_string() } else { format!("{humanized} ago") }
        }
    }
}

// ---------- 规则 ----------

type ReplaceFn = Box<dyn Fn(&[Option<String>]) -> String>;

struct Rule {
    regex: regex::Regex,
    replace: ReplaceFn,
}

fn rule(pattern: &str, replace: impl Fn(&[Option<String>]) -> String + 'static) -> Rule {
    Rule {
        regex: regex::Regex::new(&format!("(?i){pattern}")).unwrap(),
        replace: Box::new(replace),
    }
}

/// 替换执行器：等价 JS content.replace(/gi, fn) 逐条循环。
/// JS 短路语义：非尖括号规则在内容不含 {{ 时跳过。
fn apply(content: &str, rules: &[Rule]) -> String {
    let mut content = content.to_string();
    for r in rules {
        let is_angle = r.regex.as_str().starts_with("(?i)<");
        if !is_angle && !content.contains("{{") {
            break; // JS: content 不含 {{ 直接 break（所有后续规则同跳）
        }
        let mut out = String::with_capacity(content.len());
        let mut last_end = 0usize;
        let mut matched = false;
        for caps in r.regex.captures_iter(&content) {
            let m = caps.get(0).unwrap();
            let owned: Vec<Option<String>> =
                caps.iter().map(|c| c.map(|c| c.as_str().to_string())).collect();
            let replaced = (r.replace)(&owned);
            out.push_str(&content[last_end..m.start()]);
            out.push_str(&replaced);
            last_end = m.end();
            matched = true;
        }
        if matched {
            out.push_str(&content[last_end..]);
            content = out;
        }
    }
    content
}

/// 主入口：等价 evaluateMacros(content, env, ctx)。
pub fn evaluate_macros(content: &str, env: &MacroEnv, ctx: &mut MacroContext) -> String {
    if content.is_empty() {
        return String::new();
    }
    let content = content.to_string();

    // 共享可变状态
    use std::cell::RefCell;
    let vars = std::rc::Rc::new(RefCell::new(std::mem::take(&mut ctx.vars)));
    let banned = std::rc::Rc::new(RefCell::new(std::mem::take(&mut ctx.banned)));
    let original = std::rc::Rc::new(RefCell::new((env.original.clone(), false)));

    // ================= pass 1: preEnv =================
    let pre: Vec<Rule> = vec![
        rule(r"<USER>", {
            let v = env.user.clone();
            move |_| v.clone()
        }),
        rule(r"<BOT>|<CHAR>", {
            let v = env.char.clone();
            move |_| v.clone()
        }),
        rule(r"<CHARIFNOTGROUP>|<GROUP>", {
            let v = env.group.clone();
            move |_| v.clone()
        }),
        // {{roll ...}}（空格或冒号分隔）
        rule(r"\{\{roll[ : ]([^}]+)\}\}", |caps| {
            roll_dice(&cap(caps, 1), rand::random::<f64>)
                .map(|v| v.to_string())
                .unwrap_or_default()
        }),
        // instruct 系宏：CC 路径 instruct 未启用 → 全部替换为空串
        // （键列表见 instruct-mode.js getInstructMacros；含 | 别名）
        rule(
            r"\{\{(instructStoryStringPrefix|instructStoryStringSuffix|instructInput|instructUserPrefix|instructUserSuffix|instructOutput|instructAssistantPrefix|instructSeparator|instructAssistantSuffix|instructSystemPrefix|instructSystemSuffix|instructFirstOutput|instructFirstAssistantPrefix|instructLastOutput|instructLastAssistantPrefix|instructStop|instructUserFiller|instructSystemInstructionPrefix|instructFirstInput|instructFirstUserPrefix|instructLastInput|instructLastUserPrefix|systemPrompt|defaultSystemPrompt|instructSystem|instructSystemPrompt|chatSeparator|chatStart)\}\}",
            |_| String::new(),
        ),
        // 变量系（立即副作用）：每条规则先在块作用域 clone Rc，再 move 进闭包
        {
            let vars = std::rc::Rc::clone(&vars);
            rule(r"\{\{setvar::([^:]+)::([^}]*)\}\}", move |caps| {
                let name = cap(caps, 1);
                let value = cap(caps, 2);
                set_var(&mut vars.borrow_mut().local, name.trim(), &value);
                String::new()
            })
        },
        {
            let vars = std::rc::Rc::clone(&vars);
            rule(r"\{\{addvar::([^:]+)::([^}]+)\}\}", move |caps| {
                let name = cap(caps, 1);
                let value = cap(caps, 2);
                add_var(&mut vars.borrow_mut().local, name.trim(), &value);
                String::new()
            })
        },
        {
            let vars = std::rc::Rc::clone(&vars);
            rule(r"\{\{incvar::([^}]+)\}\}", move |caps| {
                let name = cap(caps, 1);
                add_var(&mut vars.borrow_mut().local, name.trim(), "1")
            })
        },
        {
            let vars = std::rc::Rc::clone(&vars);
            rule(r"\{\{decvar::([^}]+)\}\}", move |caps| {
                let name = cap(caps, 1);
                add_var(&mut vars.borrow_mut().local, name.trim(), "-1")
            })
        },
        {
            let vars = std::rc::Rc::clone(&vars);
            rule(r"\{\{getvar::([^}]+)\}\}", move |caps| {
                let name = cap(caps, 1);
                get_var(&vars.borrow().local, name.trim())
            })
        },
        {
            let vars = std::rc::Rc::clone(&vars);
            rule(r"\{\{setglobalvar::([^:]+)::([^}]*)\}\}", move |caps| {
                let name = cap(caps, 1);
                let value = cap(caps, 2);
                set_var(&mut vars.borrow_mut().global, name.trim(), &value);
                String::new()
            })
        },
        {
            let vars = std::rc::Rc::clone(&vars);
            rule(r"\{\{addglobalvar::([^:]+)::([^}]+)\}\}", move |caps| {
                let name = cap(caps, 1);
                let value = cap(caps, 2);
                add_var(&mut vars.borrow_mut().global, name.trim(), &value)
            })
        },
        {
            let vars = std::rc::Rc::clone(&vars);
            rule(r"\{\{incglobalvar::([^}]+)\}\}", move |caps| {
                let name = cap(caps, 1);
                add_var(&mut vars.borrow_mut().global, name.trim(), "1")
            })
        },
        {
            let vars = std::rc::Rc::clone(&vars);
            rule(r"\{\{decglobalvar::([^}]+)\}\}", move |caps| {
                let name = cap(caps, 1);
                add_var(&mut vars.borrow_mut().global, name.trim(), "-1")
            })
        },
        {
            let vars = std::rc::Rc::clone(&vars);
            rule(r"\{\{getglobalvar::([^}]+)\}\}", move |caps| {
                let name = cap(caps, 1);
                get_var(&vars.borrow().global, name.trim())
            })
        },
        rule(r"\{\{newline\}\}", |_| "\n".to_string()),
        // {{trim}}：连同周围换行一起移除
        rule(r"(?:\r?\n)*\{\{trim\}\}(?:\r?\n)*", |_| String::new()),
        rule(r"\{\{noop\}\}", |_| String::new()),
        rule(r"\{\{input\}\}", {
            let input = ctx.input.clone();
            move |_| input.clone()
        }),
    ];
    let mut content = apply(&content, &pre);

    // ================= pass 2: env =================
    // environment 构造序（script.js substituteParamsLegacy）
    let entries: Vec<(&str, String)> = vec![
        ("charPrompt", env.char_prompt.clone()),
        ("charInstruction", env.char_instruction.clone()),
        ("charJailbreak", env.char_instruction.clone()),
        ("description", env.description.clone()),
        ("personality", env.personality.clone()),
        ("scenario", env.scenario.clone()),
        ("persona", env.persona.clone()),
        // mesExamples 为函数：CC 拼装路径由拼装器填充，此处 raw
        ("mesExamples", String::new()),
        ("mesExamplesRaw", env.mes_examples_raw.clone()),
        ("charVersion", env.char_version.clone()),
        ("char_version", env.char_version.clone()),
        ("charDepthPrompt", env.char_depth_prompt.clone()),
        ("creatorNotes", env.creator_notes.clone()),
        // "Must be substituted last"
        ("user", env.user.clone()),
        ("char", env.char.clone()),
        ("group", env.group.clone()),
        ("charIfNotGroup", env.group.clone()),
        ("groupNotMuted", env.group_not_muted.clone()),
        ("notChar", env.not_char.clone()),
        ("model", env.model.clone()),
    ];
    for (name, value) in &entries {
        let pattern = format!(r"\{{\{{{}\}}\}}", regex::escape(name));
        let r = rule(&pattern, {
            let v = value.clone();
            move |_| v.clone()
        });
        content = apply(&content, &[r]);
    }
    for (name, value) in &env.dynamic {
        let pattern = format!(r"\{{\{{{}\}}\}}", regex::escape(name));
        let r = rule(&pattern, {
            let v = value.clone();
            move |_| v.clone()
        });
        content = apply(&content, &[r]);
    }
    // {{original}}：一次性
    if env.original.is_some() {
        let r = rule(r"\{\{original\}\}", move |_| {
            let mut o = original.borrow_mut();
            let is_used = o.1;
            if !is_used {
                o.1 = true;
                o.0.clone().unwrap_or_default()
            } else {
                String::new()
            }
        });
        content = apply(&content, &[r]);
    }

    // ================= pass 3: postEnv =================
    let raw_content = content.clone();
    let chat_id_hash = ctx.chat_id_hash;
    let post: Vec<Rule> = vec![
        rule(r"\{\{maxPrompt(?:Tokens)?\}\}", {
            let v = ctx.max_prompt_tokens;
            move |_| v.to_string()
        }),
        rule(r"\{\{maxContext(?:Tokens)?\}\}", {
            let v = ctx.max_context_tokens;
            move |_| v.to_string()
        }),
        rule(r"\{\{maxResponse(?:Tokens)?\}\}", {
            let v = ctx.max_response_tokens;
            move |_| v.to_string()
        }),
        rule(r"\{\{lastMessage\}\}", {
            let v = ctx.last_message.clone();
            move |_| v.clone()
        }),
        rule(r"\{\{lastMessageId\}\}", {
            let v = ctx.chat_length.checked_sub(1).map(|v| v as i64);
            move |_| v.map(|n| n.to_string()).unwrap_or_default()
        }),
        rule(r"\{\{lastUserMessage\}\}", {
            let v = ctx.last_user_message.clone();
            move |_| v.clone()
        }),
        rule(r"\{\{lastCharMessage\}\}", {
            let v = ctx.last_char_message.clone();
            move |_| v.clone()
        }),
        rule(r"\{\{firstIncludedMessageId\}\}", {
            let v = ctx.first_included_message_id;
            move |_| v.map(|n| n.to_string()).unwrap_or_default()
        }),
        rule(r"\{\{firstDisplayedMessageId\}\}", |_| String::new()),
        rule(r"\{\{lastSwipeId\}\}", {
            let v = ctx.last_swipe_id;
            move |_| v.map(|n| n.to_string()).unwrap_or_default()
        }),
        rule(r"\{\{currentSwipeId\}\}", {
            let v = ctx.current_swipe_id;
            move |_| v.map(|n| n.to_string()).unwrap_or_default()
        }),
        rule(r"\{\{allChatRange\}\}", {
            let len = ctx.chat_length;
            move |_| {
                if len == 0 {
                    String::new()
                } else {
                    format!("0-{}", len - 1)
                }
            }
        }),
        rule(r"\{\{reverse:(.+?)\}\}", |caps| {
            let s = cap(caps, 1);
            // Array.from(str).reverse()：按 UTF-16 code unit 反转
            let units: Vec<u16> = s.encode_utf16().collect();
            String::from_utf16_lossy(&units.into_iter().rev().collect::<Vec<_>>())
        }),
        // 注释移除（/gm）
        rule(r"(?s)\{\{//([\s\S]*?)\}\}", |_| String::new()),
        // 时间系
        rule(r"\{\{time\}\}", {
            let now = ctx.now;
            move |_| moment_lt(&now)
        }),
        rule(r"\{\{date\}\}", {
            let now = ctx.now;
            move |_| moment_ll(&now)
        }),
        rule(r"\{\{weekday\}\}", {
            let now = ctx.now;
            move |_| now.format("%A").to_string()
        }),
        rule(r"\{\{isotime\}\}", {
            let now = ctx.now;
            move |_| now.format("%H:%M").to_string()
        }),
        rule(r"\{\{isodate\}\}", {
            let now = ctx.now;
            move |_| now.format("%Y-%m-%d").to_string()
        }),
        rule(r"\{\{datetimeformat +([^}]*)\}\}", {
            let now = ctx.now;
            move |caps| {
                // moment 格式与 chrono 不完全一致；常见符号映射：
                let f = cap(caps, 1);
                let f = f.replace("HH", "%H").replace("mm", "%M").replace("ss", "%S")
                    .replace("YYYY", "%Y").replace("MM", "%m").replace("DD", "%d")
                    .replace("hh", "%I").replace("A", "%p").replace("a", "%P");
                now.format(&f).to_string()
            }
        }),
        rule(r"\{\{idle_duration\}\}", {
            let now = ctx.now;
            let last = ctx.last_user_message_at;
            move |_| idle_duration(&now, last.as_ref())
        }),
        rule(r"\{\{time_UTC([-+]\d+)\}\}", {
            let now = ctx.now;
            move |caps| {
                let offset: i32 = cap(caps, 1).parse().unwrap_or(0);
                let utc = now.naive_utc() + chrono::Duration::hours(offset as i64);
                // LT 格式
                utc.format("%-I:%M %p").to_string()
            }
        }),
        rule(r"\{\{outlet::(.+?)\}\}", {
            let outlets = ctx.outlets.clone();
            move |caps| {
                let key = cap(caps, 1);
                match outlets.get(key.trim()) { Some(Value::String(s)) => s.clone(), Some(Value::Number(n)) => n.to_string(), _ => String::new() }
            }
        }),
        {
            let banned = std::rc::Rc::clone(&banned);
            rule(r#"\{\{banned "(.*)"\}\}"#, move |caps| {
                let word = cap(caps, 1);
                banned.borrow_mut().push(word);
                String::new()
            })
        },
        // {{random}}：每次求值重新随机
        rule(r"\{\{random\s?::?([^}]+)\}\}", |caps| {
            let list = split_list(&cap(caps, 1));
            if list.is_empty() {
                String::new()
            } else {
                let idx = (rand::random::<f64>() * list.len() as f64).floor() as usize;
                list.into_iter().nth(idx).unwrap_or_default()
            }
        }),
        // {{pick}}：确定性（chat hash + raw hash + offset）
        rule(r"\{\{pick\s?::?([^}]+)\}\}", {
            let raw = raw_content.clone();
            move |caps| {
                let list = cap(caps, 1);
                let offset = caps.first().and_then(|m| m.as_ref().map(|s| s.len())).unwrap_or(0);
                pick_from_list(&list, &raw, offset, chat_id_hash)
            }
        }),
    ];
    let mut content = apply(&content, &post);

    // 归还状态
    // 归还状态（规则闭包仍持有 Rc 引用，直接克隆出值）
    ctx.vars = vars.borrow().clone();
    ctx.banned = banned.borrow().clone();

    content
}
