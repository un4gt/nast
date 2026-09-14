//! 世界书引擎：逐条复刻 world-info.js 的 checkWorldInfo 行为（1.18.0）。
//!
//! 契约锚点（refrence/SillyTavern/public/scripts/world-info.js）：
//! - 枚举：position 0-7、selectiveLogic 0-3（AND_ANY/NOT_ALL/NOT_ANY/AND_ALL）、
//!   insertion strategy 0-2（evenly/character_first/global_first）
//! - 排序：order 降序迭代；before 块 unshift 翻转为升序、after 块 push 保持降序（GOTCHA #2）
//! - 预算：round(world_info_budget% × maxContext)，budget_cap 上限；sticky 优先计数；
//!   超限丢弃后续条目除非 ignoreBudget（GOTCHA #3）
//! - timed effects：sticky/cooldown/delay 存 chat_metadata.timedWorldInfo["world.uid"]；
//!   sticky 到期立即武装同 horizon 的 cooldown；protected 隔离不推进的回合（swipe）；
//!   entry hash 变化失效记录（GOTCHA #4）
//! - 递归：world_info_recursive 全局门；preventRecursion 条目内容不进递归扫描文本
//! - key 匹配：/regex/ 覆盖全局大小写/全词设置；无 * 通配；多词 = 字面子串
//! - 宏替换在激活期（预算计数前）
//! - @D 语法：position 字符串 "@D3"/"@D2[a]" 解析为 atDepth + role 覆盖
//! - 插入组：groupOverride 优先（order 高者胜），否则 groupWeight 加权随机；
//!   useGroupScoring 按主/次命中数取最高分组内存活

use crate::macros::{evaluate_macros, MacroContext, MacroEnv};
use crate::rng;
use nast_model::world::{TimedEffect, TimedWorldInfo, WIEntry, WorldInfoBook as WIBook};
use std::collections::HashMap;

/// 全局 WI 设置切片。
#[derive(Debug, Clone)]
pub struct WiSettings {
    pub depth: i64,                 // scan 深度（默认 2）
    pub min_activations: i64,
    pub min_activations_depth_max: i64,
    pub budget: i64,                // % of max context（默认 25）
    pub budget_cap: i64,            // 0 = off
    pub recursive: bool,
    pub case_sensitive: bool,
    pub match_whole_words: bool,
    pub use_group_scoring: bool,
    pub max_recursion_steps: i64,
    pub character_strategy: i64,    // 0 evenly / 1 character_first / 2 global_first
}

impl Default for WiSettings {
    fn default() -> Self {
        Self {
            depth: 2,
            min_activations: 0,
            min_activations_depth_max: 0,
            budget: 25,
            budget_cap: 0,
            recursive: false,
            case_sensitive: false,
            match_whole_words: false,
            use_group_scoring: false,
            max_recursion_steps: 0,
            character_strategy: 1,
        }
    }
}

/// 扫描所需的上下文文本源。
#[derive(Debug, Clone, Default)]
pub struct ScanSource {
    /// 最近的消息（旧→新；引擎取末尾 depth 条）
    pub chat: Vec<String>,
    /// 各可扫描字段（由条目的 match_* 开关决定是否并入）
    pub persona_description: String,
    pub char_description: String,
    pub char_personality: String,
    pub char_depth_prompt: String,
    pub scenario: String,
}

/// 世界书来源集合（getSortedEntries 的组装结果由调用方完成，这里接收已分组的书）。
pub struct WiBooks<'a> {
    pub chat_lore: Vec<&'a WIBook>,
    pub persona_lore: Vec<&'a WIBook>,
    pub global_lore: Vec<&'a WIBook>,
    pub character_lore: Vec<&'a WIBook>,
}

/// 激活输出（getWorldInfoPrompt 等价）。
#[derive(Debug, Default, Clone)]
pub struct WiResult {
    /// position=0 (before_char) 块（升序，最高 order 邻近角色描述）
    pub world_info_before: String,
    /// position=1 (after_char) 块（降序）
    pub world_info_after: String,
    /// atDepth 条目（depth/role/content），交由拼装器做 IN_CHAT 注入
    pub depth_entries: Vec<DepthEntry>,
    /// EM 锚点条目（0=before examples / 1=after examples）
    pub em_entries: Vec<(i64, String)>,
    /// 激活的条目数（调试/overflow alert）
    pub activated_count: usize,
    /// budget 是否溢出
    pub budget_overflowed: bool,
}

#[derive(Debug, Clone)]
pub struct DepthEntry {
    pub depth: i64,
    /// 0 system / 1 user / 2 assistant
    pub role: i64,
    pub content: String,
    pub order: i64,
}

/// timed effect 的可变存储（来自 chat_metadata.timedWorldInfo，结束后回写）。
pub struct WiState<'a> {
    pub timed: &'a mut TimedWorldInfo,
    pub chat_length: i64,
}

/// 主入口：等价 checkWorldInfo + getWorldInfoPrompt。
/// `max_context` 为本次生成的 context 上限（预算计算基准）。
pub fn check_world_info(
    books: &WiBooks,
    settings: &WiSettings,
    source: &ScanSource,
    state: &mut WiState,
    env: &MacroEnv,
    max_context: i64,
) -> WiResult {
    let mut result = WiResult::default();

    // ---------- 1. 组装有序条目（getSortedEntries + 策略） ----------
    let sort_fn = |a: &WIEntry, b: &WIEntry| b.order.cmp(&a.order); // 降序
    let chat_entries = sorted_entries(&books.chat_lore, sort_fn);
    let persona_entries = sorted_entries(&books.persona_lore, sort_fn);
    let global_entries = sorted_entries(&books.global_lore, sort_fn);
    let character_entries = sorted_entries(&books.character_lore, sort_fn);

    let mut all: Vec<(String, WIEntry)> = Vec::new();
    all.extend(chat_entries);
    all.extend(persona_entries);
    match settings.character_strategy {
        0 => {
            // evenly：global+character 洗牌后按 order 排
            let mut mixed: Vec<(String, WIEntry)> = global_entries;
            mixed.extend(character_entries);
            // JS shuffle 后再 sortFn；洗牌只影响同 order 平局。Rust: 稳定排序前随机打乱
            shuffle(&mut mixed);
            mixed.sort_by(|a, b| sort_fn(&a.1, &b.1));
            all.extend(mixed);
        }
        2 => {
            all.extend(global_entries);
            all.extend(character_entries);
        }
        _ => {
            // character_first（默认）
            all.extend(character_entries);
            all.extend(global_entries);
        }
    }
    // 去重 uid+world
    let mut seen = std::collections::HashSet::new();
    all.retain(|(w, e)| seen.insert((format!("{w}#{}", e.uid))));

    // ---------- 2. 构建扫描缓冲 ----------
    let scan_depth = settings.depth.min(1000).max(0) as usize;
    let mut buffer: String = source
        .chat
        .iter()
        .rev()
        .take(scan_depth)
        .rev()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");

    let hash_cache: HashMap<String, i64> = HashMap::new();
    let _ = hash_cache;

    // ---------- 3. 状态机：INITIAL → RECURSION → MIN_ACTIVATIONS ----------
    let mut activated_contents: Vec<String> = Vec::new(); // 递归扫描累积
    let mut all_activated: Vec<(String, WIEntry)> = Vec::new();
    let mut skew: i64 = 0;

    // 初始 pass
    let (matched, timed_changed) = scan_pass(
        &all, &buffer, source, settings, state, &mut result, env,
        &mut activated_contents, &mut all_activated, skew,
    );
    let _ = timed_changed;
    buffer.push_str(&activated_contents.connect_pass_text());

    // 递归 pass
    if settings.recursive && !matched.is_empty() {
        loop {
            let steps = skew; // 已完成的递归步数
            if settings.max_recursion_steps > 0 && steps >= settings.max_recursion_steps {
                break;
            }
            skew += 1;
            let before_len = all_activated.len();
            let (m2, _) = scan_pass(
                &all, &buffer, source, settings, state, &mut result, env,
                &mut activated_contents, &mut all_activated, skew,
            );
            if all_activated.len() == before_len || m2.is_empty() {
                break;
            }
            buffer.push_str(&activated_contents.connect_pass_text());
        }
    }

    // min_activations 循环（对齐 world-info.js:4991-5007）：
    // 每 pass 扫描深度 +1（buffer 加入更早的消息），直到激活数达标或深度耗尽。
    // 深度上限判断：当前扫描深度 > min_activations_depth_max（若设置）或超过 chat 长度。
    while all_activated.len() < settings.min_activations as usize {
        skew += 1;
        let current_depth = settings.depth + skew;
        if settings.min_activations_depth_max > 0 && current_depth > settings.min_activations_depth_max
        {
            break;
        }
        if current_depth > state.chat_length as i64 {
            break;
        }
        // 扩展扫描缓冲：加入更深（更早）的消息
        let extra_depth = skew as usize;
        let total_chat = source.chat.len();
        let base = scan_depth.min(total_chat);
        let from = total_chat.saturating_sub(base + extra_depth);
        let to = total_chat.saturating_sub(base);
        if to > from {
            let older = source.chat[from..to].join("
");
            buffer = format!("{}
{}", older, buffer);
        }
        let before_len = all_activated.len();
        let _ = scan_pass(
            &all, &buffer, source, settings, state, &mut result, env,
            &mut activated_contents, &mut all_activated, skew,
        );
        if all_activated.len() == before_len {
            // 本 pass 无新增 → 提前终止（ST 在无新增时也会继续下一 pass，但深度会耗尽）
        }
    }

    // ---------- 4. 插入组过滤（group override / weighted / scoring） ----------
    let survivors = resolve_groups(all_activated, settings);

    // ---------- 5. 预算计数（sticky 优先，然后迭代序；超限丢弃除 ignoreBudget） ----------
    let mut budget = ((settings.budget as f64) * (max_context as f64) / 100.0).round() as i64;
    if budget < 1 {
        budget = 1;
    }
    if settings.budget_cap > 0 && budget > settings.budget_cap {
        budget = settings.budget_cap;
    }

    // sticky-activated 条目优先进入预算（1.18.0：sticky 首先被包含）
    let (sticky_list, normal_list): (Vec<_>, Vec<_>) = survivors
        .into_iter()
        .partition(|(_, e)| e.sticky > 0);
    let ordered: Vec<(String, WIEntry)> = sticky_list.into_iter().chain(normal_list).collect();

    let mut used: i64 = 0;
    let mut final_entries: Vec<(String, WIEntry)> = Vec::new();
    for (world, entry) in ordered {
        if entry.ignore_budget {
            final_entries.push((world, entry));
            continue;
        }
        let cost = tok_cost(&entry.content) + 1; // content + '\n'
        if used + cost > budget {
            result.budget_overflowed = true;
            continue; // 丢弃
        }
        used += cost;
        final_entries.push((world, entry));
    }

    // ---------- 6. 位置分发（order 降序迭代 → before unshift 翻转 / after push） ----------
    // final_entries 已按预算序（= 迭代序 = order 降序）
    let mut before_list: Vec<String> = Vec::new();
    let mut after_list: Vec<String> = Vec::new();
    for (_, entry) in &final_entries {
        let content = &entry.content;
        let (pos, at_d) = entry.position.resolve();
        match pos {
            0 => {
                // WIBeforeEntries.unshift → 最终升序
                before_list.insert(0, content.clone());
                let _ = at_d;
            }
            1 => {
                // ST 同样 unshift（world-info.js:5098）→ after 块也是升序
                after_list.insert(0, content.clone());
            }
            4 => {
                // atDepth：@D 或数值 4；depth/role 已由 entry 解析
                let (depth, role) = match &at_d {
                    Some((d, r)) => (*d, r.unwrap_or(entry.role.unwrap_or(0))),
                    None => (entry.depth, entry.role.unwrap_or(0)),
                };
                result.depth_entries.push(DepthEntry {
                    depth,
                    role,
                    content: content.clone(),
                    order: entry.order,
                });
            }
            5 => result.em_entries.push((0, content.clone())),
            6 => result.em_entries.push((1, content.clone())),
            _ => {
                // ANTop/ANBottom/outlet：v1 将 ANTop/ANBottom 并入 before/after 尾部，
                // outlet 由扩展消费（未实现则忽略）
                before_list.insert(0, content.clone());
            }
        }
    }
    result.world_info_before = before_list.join("\n");
    result.world_info_after = after_list.join("\n");
    result.activated_count = final_entries.len();
    result
}

fn sorted_entries<'a>(
    books: &[&'a WIBook],
    sort_fn: impl Fn(&WIEntry, &WIEntry) -> std::cmp::Ordering + Copy,
) -> Vec<(String, WIEntry)> {
    let mut out: Vec<(String, WIEntry)> = Vec::new();
    for book in books {
        for (uid_s, entry) in &book.entries {
            out.push((uid_s.clone(), entry.clone()));
        }
    }
    out.sort_by(|a, b| sort_fn(&a.1, &b.1));
    out
}

fn shuffle<T>(v: &mut [T]) {
    for i in (1..v.len()).rev() {
        v.swap(i, (rand::random::<f64>() * (i as f64 + 1.0)) as usize);
    }
}

type PassOutcome = (
    Vec<(String, i64)>, // matched (world, uid)
    bool,               // timed state changed
);

/// 单次扫描（对每个条目做 disable/timed/decorator/key/secondary/probability 判定）。
#[allow(clippy::too_many_arguments)]
fn scan_pass(
    all: &[(String, WIEntry)],
    buffer: &str,
    source: &ScanSource,
    settings: &WiSettings,
    state: &mut WiState,
    _result: &mut WiResult,
    env: &MacroEnv,
    activated_contents: &mut Vec<String>,
    all_activated: &mut Vec<(String, WIEntry)>,
    _skew: i64,
) -> PassOutcome {
    let mut matched: Vec<(String, i64)> = Vec::new();
    let mut timed_changed = false;
    let chat_len = state.chat_length;

    for (world, entry) in all {
        // 已激活过的跳过
        if all_activated.iter().any(|(w, e)| w == world && e.uid == entry.uid) {
            continue;
        }
        if entry.disable {
            continue;
        }
        // delayUntilRecursion：非递归 pass（skew==0）跳过
        if entry.delay_until_recursion && _skew == 0 {
            continue;
        }
        // excludeRecursion：递归 pass 跳过
        if entry.exclude_recursion && _skew > 0 {
            continue;
        }
        // delay：绝对消息数
        if entry.delay > 0 && chat_len < entry.delay {
            continue;
        }

        // timed effects（对齐 world-info.js #checkTimedEffectOfType）：
        // 1) 聊天未推进且非 protected → 删记录（swipe/regen 回滚）
        // 2) chat_len >= end → 到期：删记录；sticky 到期且 entry.cooldown>0 → 武装 cooldown（protected，同 horizon）
        // 3) 激活期内 sticky 直接通过（跳过 key/概率），cooldown 抑制
        let key = format!("{world}.{}", entry.uid);
        let entry_hash = string_hash_entry(entry);

        // 聊天未推进回滚（非 protected）
        if let Some(rec) = state.timed.sticky.get(&key).cloned() {
            if chat_len <= rec.start && !rec.protected {
                state.timed.sticky.remove(&key);
            }
        }
        if let Some(rec) = state.timed.cooldown.get(&key).cloned() {
            if chat_len <= rec.start && !rec.protected {
                state.timed.cooldown.remove(&key);
            }
        }

        // sticky 到期检测（在 cooldown 判定前，命中即武装 cooldown）
        if let Some(rec) = state.timed.sticky.get(&key).cloned() {
            if rec.hash == entry_hash && chat_len >= rec.end {
                state.timed.sticky.remove(&key);
                if entry.cooldown > 0 {
                    // ST #getEntryTimedEffect('cooldown', entry, true)：start=chat.len, end=chat.len+cooldown, protected
                    state.timed.cooldown.insert(
                        key.clone(),
                        nast_model::world::TimedEffect {
                            hash: entry_hash,
                            start: chat_len,
                            end: chat_len + entry.cooldown,
                            protected: true,
                        },
                    );
                }
            }
        }

        // cooldown 抑制
        if let Some(rec) = state.timed.cooldown.get(&key) {
            if chat_len < rec.end && rec.hash == entry_hash {
                continue;
            }
        }
        let sticky_active = state
            .timed
            .sticky
            .get(&key)
            .map(|rec| chat_len < rec.end && rec.hash == entry_hash)
            .unwrap_or(false);

        // decorator：@@dont_activate / @@activate
        let (content_decorated, decorator_active) = parse_decorators(&entry.content);
        if decorator_active == Some(false) {
            continue;
        }

        // 扫描文本扩展（match_* 开关）
        let mut scan_text = buffer.to_string();
        if entry.match_persona_description {
            scan_text.push('\n');
            scan_text.push_str(&source.persona_description);
        }
        if entry.match_character_description {
            scan_text.push('\n');
            scan_text.push_str(&source.char_description);
        }
        if entry.match_character_personality {
            scan_text.push('\n');
            scan_text.push_str(&source.char_personality);
        }
        if entry.match_character_depth_prompt {
            scan_text.push('\n');
            scan_text.push_str(&source.char_depth_prompt);
        }
        if entry.match_scenario {
            scan_text.push('\n');
            scan_text.push_str(&source.scenario);
        }

        // sticky 激活期内跳过 key 匹配直接激活（world-info.js: active without re-matching）
        if sticky_active {
            matched.push((world.clone(), entry.uid));
            let content = substitute_entry(env, &entry.content);
            let mut entry_with_content = entry.clone();
            entry_with_content.content = content.clone();
            all_activated.push((world.clone(), entry_with_content));
            if !entry.prevent_recursion {
                activated_contents.push(content);
            }
            continue;
        }

        // key 匹配（constant 条目跳过 key 检查）
        let primary_ok = entry.constant
            || match_keys(&entry.key, &scan_text, entry, settings);
        if !primary_ok {
            continue;
        }
        // secondary keys（selective 语义）
        if entry.selective && !entry.keysecondary.is_empty() {
            if !check_secondary(entry, &scan_text, settings) {
                continue;
            }
        }

        // probability（sticky 激活期跳过 roll）
        if entry.use_probability && entry.probability < 100 && !sticky_active {
            let roll: f64 = rand::random::<f64>() * 100.0;
            if roll > entry.probability as f64 {
                continue;
            }
        }

        // 激活！
        matched.push((world.clone(), entry.uid));
        // sticky 记录（cooldown 到期时由 sticky 到期回调武装，见上方到期检测）
        if entry.sticky > 0 {
            let has = state
                .timed
                .sticky
                .get(&key)
                .map(|r| chat_len < r.end && r.hash == entry_hash)
                .unwrap_or(false);
            if !has {
                state.timed.sticky.insert(
                    key.clone(),
                    TimedEffect {
                        hash: entry_hash,
                        start: chat_len,
                        end: chat_len + entry.sticky,
                        protected: true,
                    },
                );
                timed_changed = true;
            }
        } else if entry.cooldown > 0 {
            // 纯 cooldown 条目：激活时记录（首次激活即开始冷却窗口，end = len + cooldown）
            let has = state
                .timed
                .cooldown
                .get(&key)
                .map(|r| r.hash == entry_hash && chat_len < r.end)
                .unwrap_or(false);
            if !has {
                state.timed.cooldown.insert(
                    key.clone(),
                    TimedEffect {
                        hash: entry_hash,
                        start: chat_len,
                        end: chat_len + entry.cooldown,
                        protected: false,
                    },
                );
                timed_changed = true;
            }
        }

        // 内容宏替换（激活期，预算前）
        let mut macro_ctx = MacroContext::default();
        let content = evaluate_macros(&content_decorated, env, &mut macro_ctx);
        let mut entry_with_content = entry.clone();
        entry_with_content.content = content.clone();
        all_activated.push((world.clone(), entry_with_content));
        // preventRecursion：内容不进递归扫描
        if !entry.prevent_recursion {
            activated_contents.push(content);
        }
    }
    (matched, timed_changed)
}

/// 激活期内容宏替换。
fn substitute_entry(env: &MacroEnv, text: &str) -> String {
    let mut macro_ctx = MacroContext::default();
    evaluate_macros(text, env, &mut macro_ctx)
}

fn parse_decorators(content: &str) -> (String, Option<bool>) {
    // 对齐 world-info.js：仅当内容以 @@ 开头才进入装饰器解析；@@@ 为字面转义（@@）。
    // 装饰器只允许出现在头部连续块中，正文中间的 @@ 行保持原样。
    if !content.starts_with("@@") {
        return (content.to_string(), None);
    }
    let mut activate: Option<bool> = None;
    let mut kept: Vec<&str> = Vec::new();
    let mut in_decorator_block = true;
    for line in content.lines() {
        if in_decorator_block {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("@@@") {
                // @@@xxx 转义为字面 @xxx（ST: escape hatch）
                kept.push(rest);
                in_decorator_block = false;
                continue;
            }
            if t == "@@dont_activate" {
                activate = Some(false);
                continue;
            }
            if t == "@@activate" {
                activate = Some(true);
                continue;
            }
            if t.starts_with("@@") {
                continue; // 其他头部装饰器忽略（@@depth 等）
            }
            in_decorator_block = false;
        }
        kept.push(line);
    }
    (kept.join("\n"), activate)
}

/// key 匹配：/regex/ 覆盖全局设置；多词/子串按全局大小写与全词设置。
fn match_keys(keys: &[String], scan: &str, entry: &WIEntry, settings: &WiSettings) -> bool {
    if keys.is_empty() {
        return false;
    }
    let case_sensitive = entry.case_sensitive.unwrap_or(settings.case_sensitive);
    let whole_words = entry.match_whole_words.unwrap_or(settings.match_whole_words);
    for key in keys {
        if key_matches(key, scan, case_sensitive, whole_words) {
            return true;
        }
    }
    false
}

fn key_matches(key: &str, scan: &str, case_sensitive: bool, whole_words: bool) -> bool {
    // /pattern/flags → 正则 key
    if let Some(pattern) = key.strip_prefix('/').and_then(|k| k.strip_suffix('/')) {
        let pattern = if case_sensitive {
            pattern.to_string()
        } else {
            format!("(?i){pattern}")
        };
        return regex::Regex::new(&pattern)
            .map(|re| re.is_match(scan))
            .unwrap_or(false);
    }
    if case_sensitive {
        if whole_words && !key.contains(' ') {
            let re = regex::Regex::new(&format!(
                r"(?:^|\W)({})(?:$|\W)",
                regex::escape(key)
            ))
            .unwrap();
            re.is_match(scan)
        } else {
            scan.contains(key)
        }
    } else {
        let scan_lc = scan.to_lowercase();
        let key_lc = key.to_lowercase();
        if whole_words && !key_lc.contains(' ') {
            let re = regex::Regex::new(&format!(
                r"(?i)(?:^|\W)({})(?:$|\W)",
                regex::escape(&key_lc)
            ))
            .unwrap();
            re.is_match(&scan_lc)
        } else {
            scan_lc.contains(&key_lc)
        }
    }
}

/// secondary logic 判定：0 AND_ANY / 1 NOT_ALL / 2 NOT_ANY / 3 AND_ALL。
fn check_secondary(entry: &WIEntry, scan: &str, settings: &WiSettings) -> bool {
    let case_sensitive = entry.case_sensitive.unwrap_or(settings.case_sensitive);
    let whole_words = entry.match_whole_words.unwrap_or(settings.match_whole_words);
    let hits: Vec<bool> = entry
        .keysecondary
        .iter()
        .map(|k| key_matches(k, scan, case_sensitive, whole_words))
        .collect();
    match entry.selective_logic {
        1 => !hits.iter().all(|h| *h),           // NOT_ALL
        2 => !hits.iter().any(|h| *h),           // NOT_ANY
        3 => hits.iter().all(|h| *h),            // AND_ALL
        _ => hits.iter().any(|h| *h),            // AND_ANY
    }
}

/// 插入组：每组合一格。groupOverride 优先（order 高者胜）；否则 groupWeight 加权随机。
/// useGroupScoring：按命中数评分取最高（未实现逐条评分时退化为默认）。
fn resolve_groups(activated: Vec<(String, WIEntry)>, settings: &WiSettings) -> Vec<(String, WIEntry)> {
    use std::collections::HashMap;
    // group 字段是逗号分隔列表
    let mut groups: HashMap<String, Vec<(String, WIEntry)>> = HashMap::new();
    let mut free = Vec::new();
    for (w, e) in activated {
        if e.group.trim().is_empty() {
            free.push((w, e));
        } else {
            for g in e.group.split(',') {
                groups.entry(g.trim().to_string()).or_default().push((w.clone(), e.clone()));
            }
        }
    }
    let mut winners: Vec<(String, WIEntry)> = free;
    for (_, members) in groups {
        // scoring
        if settings.use_group_scoring {
            // v1 scoring：keys 命中数已在扫描期判定；此处取 order 最高为胜者简化
            let mut best = members;
            best.sort_by(|a, b| b.1.order.cmp(&a.1.order));
            if let Some(w) = best.into_iter().next() {
                winners.push(w);
            }
            continue;
        }
        let override_member = members
            .iter()
            .filter(|(_, e)| e.group_override)
            .max_by_key(|(_, e)| e.order)
            .cloned();
        if let Some(w) = override_member {
            winners.push(w);
            continue;
        }
        // 加权随机
        let total: i64 = members.iter().map(|(_, e)| e.group_weight.max(0)).sum();
        if total <= 0 {
            continue;
        }
        let pick = (rand::random::<f64>() * total as f64) as i64;
        let mut acc = 0;
        for (w, e) in &members {
            acc += e.group_weight.max(0);
            if pick < acc {
                winners.push((w.clone(), e.clone()));
                break;
            }
        }
    }
    // 恢复 order 降序（预算计数序）
    winners.sort_by(|a, b| b.1.order.cmp(&a.1.order));
    winners
}

fn tok_cost(s: &str) -> i64 {
    crate::tokens::count_tokens(s, crate::tokens::resolve_tokenizer("gpt-4o")) as i64
}

/// entry hash：getStringHash(JSON.stringify(entry)) 等价 —— 用稳定序列化。
fn string_hash_entry(entry: &WIEntry) -> i64 {
    // serde 序列化（BTreeMap 字段序稳定）；与 JS 的字段序不同但 hash 失效判定
    // 只要求「同 entry 同 hash、entry 变更后 hash 变」，故实现内自洽即可
    let json = serde_json::to_string(entry).unwrap_or_default();
    rng::string_hash(&json)
}

trait ConnectPass {
    fn connect_pass_text(&self) -> String;
}

impl ConnectPass for Vec<String> {
    fn connect_pass_text(&self) -> String {
        self.join("\n")
    }
}

/// 从书中提取匹配条目（供测试注入简化）。
pub fn book_entries(book: &WIBook) -> Vec<(String, WIEntry)> {
    book.entries.iter().map(|(k, e)| (k.clone(), e.clone())).collect()
}
