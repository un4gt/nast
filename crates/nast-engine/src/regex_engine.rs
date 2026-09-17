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
    let pass_ok = if script.markdown_only {
        params.is_markdown
    } else if script.prompt_only {
        params.is_prompt
    } else {
        true
    };
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

/// 运行单条脚本（runRegexScript 语义）。
fn run_script(script: &RegexScript, raw: &str, macro_fn: MacroFn) -> String {
    if script.disabled || script.find_regex.is_empty() || raw.is_empty() {
        return raw.to_string();
    }
    // findRegex 宏替换模式
    let find = match script.substitute_regex {
        SUB_RAW => macro_fn(&script.find_regex),
        SUB_ESCAPED => regex::escape(&macro_fn(&script.find_regex)),
        _ => script.find_regex.clone(),
    };
    let Ok(re) = regex::Regex::new(&find) else {
        // JS regex 特性（look-around 等）regex crate 不支持 → 跳过该脚本
        return raw.to_string();
    };

    // 替换串：{{match}} → $0（regex crate 用 ${0}）
    let replace_template = script.replace_string.replace("{{match}}", "$0");

    re.replace_all(raw, |caps: &regex::Captures| {
        // $N / $<name> 展开：捕获组值先过滤 trimStrings
        let expanded = expand_groups(&replace_template, caps, &script.trim_strings);
        // 替换结果末尾 substituteParams
        macro_fn(&expanded)
    })
    .to_string()
}

/// 展开 $N / $<name>，组值经 trimStrings 过滤。
fn expand_groups(template: &str, caps: &regex::Captures, trim: &[String]) -> String {
    let mut out = String::new();
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            if bytes[i + 1] == b'<' {
                // $<name>
                if let Some(end) = template[i + 2..].find('>') {
                    let name = &template[i + 2..i + 2 + end];
                    let val = caps.name(name).map(|m| m.as_str()).unwrap_or("");
                    out.push_str(&filter_string(val, trim));
                    i += 2 + end + 1;
                    continue;
                }
            }
            // $N（多位数字）
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 {
                let num: usize = template[i + 1..j].parse().unwrap_or(0);
                let val = caps.get(num).map(|m| m.as_str()).unwrap_or("");
                out.push_str(&filter_string(val, trim));
                i = j;
                continue;
            }
            out.push('$');
            i += 1;
        } else {
            let ch = template[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// filterString：从值中移除 trimStrings 子串（split(t).join('')）。
fn filter_string(s: &str, trim_strings: &[String]) -> String {
    let mut out = s.to_string();
    for t in trim_strings {
        if t.is_empty() {
            continue;
        }
        out = out.replace(t.as_str(), "");
    }
    out
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
        assert_eq!(out, "bar baz bar");
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
        let scripts = vec![script(
            r"(\w+) says",
            "$1 spoke ({{match}})",
            vec![1],
        )];
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
        let scripts = vec![script("hello", "hi {{user}}", vec![1])];
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
        let scoped = RegexScripts {
            global: vec![script("one", "two", vec![1])],
            character: vec![script("two", "three", vec![1])],
            chat: vec![script("three", "four", vec![1])],
        };
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
    fn unsupported_regex_skipped() {
        // look-ahead 在 regex crate 不支持 → 脚本跳过（原文保留）
        let scripts = vec![script(r"foo(?=bar)", "baz", vec![1])];
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
        assert_eq!(out, "foobar");
    }
}
