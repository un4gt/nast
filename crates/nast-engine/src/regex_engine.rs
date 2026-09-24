//! 正则引擎：复刻 extensions/regex/engine.js 的 getRegexedString / runRegexScript。
//!
//! 契约：
//! - 应用条件：markdownOnly→isMarkdown pass；promptOnly→isPrompt pass；
//!   两者皆 false → 仅在「既非 markdown 也非 prompt」的 pass（改动聊天文本本身）
//! - 深度过滤：minDepth>=-1 且 depth < minDepth 跳过；maxDepth>=0 且 depth > maxDepth 跳过
//! - trimStrings：先从原文移除这些子串（filterString 作用于捕获组值）
//! - substituteRegex：0 原样 / 1 宏替换进 findRegex / 2 宏替换且结果 regex 转义
//! - 替换串：{{match}} → 整体匹配（$0）；$N / $<name> → 捕获组；
//!   捕获组值先过 trimStrings 过滤；替换结果末尾再过一遍 substituteParams
//! - 脚本顺序：全局列表 → 角色脚本 → 聊天脚本，链式应用

use nast_model::regex_script::{RegexScript, SUB_ESCAPED, SUB_RAW};

pub struct RegexParams<'a> {
    /// 距底部深度（0 = 最新消息）；None 表示不做深度过滤
    pub depth: Option<i64>,
    pub is_markdown: bool,
    pub is_prompt: bool,
    pub is_edit: bool,
    pub character_override: Option<&'a str>,
}

impl<'a> Default for RegexParams<'a> {
    fn default() -> Self {
        Self {
            depth: None,
            is_markdown: false,
            is_prompt: false,
            is_edit: false,
            character_override: None,
        }
    }
}

/// 宏替换回调（用于 substituteRegex 与替换串末尾的 substituteParams）。
pub type MacroFn<'a> = &'a dyn Fn(&str) -> String;

/// 三作用域脚本集合（调用方聚合：全局→角色→聊天）。
pub struct RegexScripts {
    pub global: Vec<RegexScript>,
    pub character: Vec<RegexScript>,
    pub chat: Vec<RegexScript>,
}

impl RegexScripts {
    /// 链式应用全部脚本（聚合顺序：global → character → chat）。
    pub fn apply(&self, raw: &str, placement: i64, params: &RegexParams, macro_fn: MacroFn) -> String {
        let mut s = raw.to_string();
        for script in self.global.iter().chain(self.character.iter()).chain(self.chat.iter()) {
            if script_applies(script, placement, params) {
                s = run_script(script, &s, macro_fn);
            }
        }
        s
    }
}

/// 单一脚本集合版（仅全局，供简单调用方）。
pub fn get_regexed_string(
    raw: &str,
    placement: i64,
    scripts: &[RegexScript],
    params: &RegexParams,
    macro_fn: MacroFn,
) -> String {
    let mut s = raw.to_string();
    for script in scripts {
        if script_applies(script, placement, params) {
            s = run_script(script, &s, macro_fn);
        }
    }
    s
}

fn script_applies(script: &RegexScript, placement: i64, params: &RegexParams) -> bool {
    if script.disabled {
        return false;
    }
    // pass 匹配（engine.js 348-355）：
    // markdownOnly 只在 isMarkdown；promptOnly 只在 isPrompt；
    // 两者皆 false → 所有 pass 应用（改动聊天文本的 pass 只是其中一个）
    let pass_ok = (script.markdown_only && params.is_markdown)
        || (script.prompt_only && params.is_prompt)
        || (!script.markdown_only && !script.prompt_only && !params.is_markdown && !params.is_prompt);
    if !pass_ok {
        return false;
    }
    // isEdit 门控
    if params.is_edit && !script.run_on_edit {
        return false;
    }
    // 深度过滤
    if let Some(depth) = params.depth {
        if let Some(min) = script.min_depth {
            if min >= -1 && depth < min {
                return false;
            }
        }
        if let Some(max) = script.max_depth {
            if max >= 0 && depth > max {
                return false;
            }
        }
    }
    script.placement.contains(&placement)
}

/// A bounded ECMAScript runtime keeps browser RegExp semantics on the server.
fn execute_regex(payload: serde_json::Value) -> Result<String, String> {
    let runtime = rquickjs::Runtime::new().map_err(|e| e.to_string())?;
    runtime.set_memory_limit(32 * 1024 * 1024);
    runtime.set_max_stack_size(512 * 1024);
    let started = std::time::Instant::now();
    runtime.set_interrupt_handler(Some(Box::new(move || started.elapsed() > std::time::Duration::from_millis(100))));
    let context = rquickjs::Context::full(&runtime).map_err(|e| e.to_string())?;
    context.with(|ctx| {
        ctx.globals().set("payload", payload.to_string()).map_err(|e| e.to_string())?;
        ctx.eval::<String, _>(include_str!("regex_runtime.js")).map_err(|e| e.to_string())
    })
}

pub fn ecma_is_match(pattern: &str, text: &str) -> Result<bool, String> {
    let output = execute_regex(serde_json::json!({"find":pattern,"raw":text,"test":true}))?;
    serde_json::from_str(&output).map_err(|e| e.to_string())
}

fn escape_macro(text: &str) -> String {
    let mut output = String::new();
    for c in text.chars() {
        match c {
            '\n' => output.push_str("\\n"), '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"), '\0' => output.push_str("\\0"),
            '.' | '^' | '$' | '*' | '+' | '?' | '{' | '}' | '[' | ']' | '\\' | '/' | '|' | '(' | ')' => {
                output.push('\\'); output.push(c);
            }
            _ => output.push(c),
        }
    }
    output
}

fn run_script(script: &RegexScript, raw: &str, macro_fn: MacroFn) -> String {
    if script.disabled || script.find_regex.is_empty() || raw.is_empty() { return raw.into(); }
    let find = match script.substitute_regex {
        SUB_RAW => macro_fn(&script.find_regex),
        SUB_ESCAPED => regex::Regex::new(r"\{\{[^}]+\}\}").unwrap()
            .replace_all(&script.find_regex, |caps: &regex::Captures| escape_macro(&macro_fn(&caps[0]))).into_owned(),
        _ => script.find_regex.clone(),
    };
    let result = execute_regex(serde_json::json!({
        "find":find, "raw":raw, "replace":script.replace_string,
        "trim":script.trim_strings.iter().map(|s| macro_fn(s)).collect::<Vec<_>>(),
    })).and_then(|result| serde_json::from_str::<Vec<(bool,String)>>(&result).map_err(|e| e.to_string()));
    match result {
        Ok(pieces) => pieces.into_iter().map(|(replacement,text)| if replacement { macro_fn(&text) } else { text }).collect(),
        Err(error) => {
            tracing::warn!(script_id=%script.id, %error, "regular expression skipped");
            raw.into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nast_model::regex_script::RegexScript;

    fn script(find: &str, replace: &str, placement: Vec<i64>) -> RegexScript {
        RegexScript {
            find_regex: find.into(),
            replace_string: replace.into(),
            placement,
            ..Default::default()
        }
    }

    const M: MacroFn = &|s| s.to_string();
    const USER_MACRO: MacroFn = &|s| s.replace("{{user}}", "Alice");

    #[test]
    fn basic_replacement() {
        let scripts = vec![script("foo", "bar", vec![0, 1])];
        let out = get_regexed_string("foo baz foo", 1, &scripts, &RegexParams::default(), M);
        assert_eq!(out, "bar baz foo"); // No g flag: ST replaces only the first match.
    }

    #[test]
    fn placement_filter() {
        let scripts = vec![script("foo", "bar", vec![0])]; // 仅 USER_INPUT
        let out = get_regexed_string("foo", 1, &scripts, &RegexParams::default(), M);
        assert_eq!(out, "foo"); // AI_OUTPUT 不应用
    }

    #[test]
    fn prompt_only_pass() {
        let mut s = script("foo", "bar", vec![1]);
        s.prompt_only = true;
        let scripts = vec![s];
        // isPrompt pass 应用
        let out = get_regexed_string(
            "foo",
            1,
            &scripts,
            &RegexParams {
                is_prompt: true,
                ..Default::default()
            },
            M,
        );
        assert_eq!(out, "bar");
        // display pass 不应用
        let out = get_regexed_string(
            "foo",
            1,
            &scripts,
            &RegexParams {
                is_markdown: true,
                ..Default::default()
            },
            M,
        );
        assert_eq!(out, "foo");
    }

    #[test]
    fn markdown_only_pass() {
        let mut s = script("foo", "bar", vec![1]);
        s.markdown_only = true;
        let scripts = vec![s];
        let out = get_regexed_string(
            "foo",
            1,
            &scripts,
            &RegexParams {
                is_markdown: true,
                ..Default::default()
            },
            M,
        );
        assert_eq!(out, "bar");
        let out = get_regexed_string(
            "foo",
            1,
            &scripts,
            &RegexParams {
                is_prompt: true,
                ..Default::default()
            },
            M,
        );
        assert_eq!(out, "foo");
    }

    #[test]
    fn depth_filter() {
        let mut s = script("foo", "bar", vec![1]);
        s.prompt_only = true;
        s.min_depth = Some(1);
        s.max_depth = Some(3);
        let scripts = vec![s];
        // depth 0（最新）< minDepth → 跳过
        let out = get_regexed_string(
            "foo",
            1,
            &scripts,
            &RegexParams {
                depth: Some(0),
                is_prompt: true,
                ..Default::default()
            },
            M,
        );
        assert_eq!(out, "foo");
        // depth 2 在窗口内
        let out = get_regexed_string(
            "foo",
            1,
            &scripts,
            &RegexParams {
                depth: Some(2),
                is_prompt: true,
                ..Default::default()
            },
            M,
        );
        assert_eq!(out, "bar");
        // depth 5 > maxDepth → 跳过
        let out = get_regexed_string(
            "foo",
            1,
            &scripts,
            &RegexParams {
                depth: Some(5),
                is_prompt: true,
                ..Default::default()
            },
            M,
        );
        assert_eq!(out, "foo");
    }

    #[test]
    fn trim_strings_filters_group_values() {
        // trimStrings 过滤的是捕获组展开值（JS filterString 作用于 $N 值）
        let mut s = script(r"(remove-me)?foo", "$1bar", vec![1]);
        s.prompt_only = true;
        s.trim_strings = vec!["remove-me".into()];
        let scripts = vec![s];
        let out = get_regexed_string(
            "remove-mefoo",
            1,
            &scripts,
            &RegexParams {
                is_prompt: true,
                ..Default::default()
            },
            M,
        );
        // $1 = "remove-me" → 过滤后为空 → 输出 "bar"
        assert_eq!(out, "bar");
    }

    #[test]
    fn capture_groups_and_match() {
        let mut scripts = vec![script(
            r"(\w+) says",
            "$1 spoke ({{match}})",
            vec![1],
        )];
        scripts[0].prompt_only = true;
        let out = get_regexed_string(
            "Alice says hi",
            1,
            &scripts,
            &RegexParams {
                is_prompt: true,
                ..Default::default()
            },
            M,
        );
        assert_eq!(out, "Alice spoke (Alice says) hi");
    }

    #[test]
    fn substitute_regex_raw() {
        let mut s = script(r"\{\{user\}\} walks", "Alice walks", vec![1]);
        s.substitute_regex = 1; // RAW：findRegex 先过宏替换
        let scripts = vec![s];
        let out = get_regexed_string(
            "Alice walks home",
            1,
            &scripts,
            &RegexParams {
                is_prompt: true,
                ..Default::default()
            },
            USER_MACRO,
        );
        assert_eq!(out, "Alice walks home");
    }

    #[test]
    fn replace_string_macro_substituted() {
        // 替换结果末尾过 substituteParams
        let mut scripts = vec![script("hello", "hi {{user}}", vec![1])];
        scripts[0].prompt_only = true;
        let out = get_regexed_string(
            "hello world",
            1,
            &scripts,
            &RegexParams {
                is_prompt: true,
                ..Default::default()
            },
            USER_MACRO,
        );
        assert_eq!(out, "hi Alice world");
    }

    #[test]
    fn disabled_and_edit_gates() {
        let mut s = script("foo", "bar", vec![1]);
        s.disabled = true;
        let scripts = vec![s];
        let out = get_regexed_string(
            "foo",
            1,
            &scripts,
            &RegexParams {
                is_prompt: true,
                ..Default::default()
            },
            M,
        );
        assert_eq!(out, "foo");

        let mut s2 = script("foo", "bar", vec![1]);
        s2.run_on_edit = false;
        let scripts2 = vec![s2];
        let out = get_regexed_string(
            "foo",
            1,
            &scripts2,
            &RegexParams {
                is_prompt: true,
                is_edit: true,
                ..Default::default()
            },
            M,
        );
        assert_eq!(out, "foo");
    }

    #[test]
    fn scoped_chaining_order() {
        // 全局 → 角色 → 聊天 链式：后一环作用在前一环输出上
        let mut scoped = RegexScripts {
            global: vec![script("one", "two", vec![1])],
            character: vec![script("two", "three", vec![1])],
            chat: vec![script("three", "four", vec![1])],
        };
        for script in scoped.global.iter_mut().chain(scoped.character.iter_mut()).chain(scoped.chat.iter_mut()) {
            script.prompt_only = true;
        }
        let out = scoped.apply(
            "one",
            1,
            &RegexParams {
                is_prompt: true,
                ..Default::default()
            },
            M,
        );
        assert_eq!(out, "four");
    }

    #[test]
    fn lookahead_is_supported() {
        let mut scripts = vec![script(r"foo(?=bar)", "baz", vec![1])];
        scripts[0].prompt_only = true;
        let out = get_regexed_string(
            "foobar",
            1,
            &scripts,
            &RegexParams {
                is_prompt: true,
                ..Default::default()
            },
            M,
        );
        assert_eq!(out, "bazbar");
    }
    #[test]
    fn ecmascript_flags_named_groups_and_raw_pass_gates() {
        let script = script(r"/(?<=prefix)(?<word>foo)/gi", "$<word>!", vec![1]);
        assert_eq!(run_script(&script, "prefixFoo prefixfoo", M), "prefixFoo! prefixfoo!");
        assert!(!script_applies(&script, 1, &RegexParams { is_prompt:true, ..Default::default() }));
        let mut both = script;
        both.markdown_only = true; both.prompt_only = true;
        assert!(script_applies(&both, 1, &RegexParams { is_prompt:true, ..Default::default() }));
        assert!(script_applies(&both, 1, &RegexParams { is_markdown:true, ..Default::default() }));
        assert!(ecma_is_match("/(?<=a)b/", "ab").unwrap());
    }

}
