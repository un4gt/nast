//! 世界书引擎：逐条复刻 world-info.js 的 checkWorldInfo 行为（1.18.0）。
//!
//! 契约锚点（refrence/SillyTavern/public/scripts/world-info.js）：
//! - 枚举：position 0-7、selectiveLogic 0-3（AND_ANY/NOT_ALL/NOT_ANY/AND_ALL）、
//!   insertion strategy 0-2（evenly/character_first/global_first）
//! - 排序：order 降序迭代；before/after 块 unshift 翻转为升序、after 块同样 unshift（world-info.js:5093-5105）
//! - 预算（v1.18 精确语义，4936-4955）：每 pass 累计 newContent（"content\n" 追加），
//!   基线 = tokens(allActivatedText)，`(base + tokens(newContent)) >= budget` 即溢出；
//!   溢出后非 ignoreBudget 条目跳过（前方还有 ignoreBudget 时 continue，否则 break）；
//!   被预算丢弃的条目内容仍进递归扫描文本
//! - timed effects：sticky/cooldown/delay 存 chat_metadata.timedWorldInfo["world.uid"]；
//!   sticky 到期立即武装同 horizon cooldown；protected 隔离不推进的回合（swipe）；
//!   entry hash 变化失效记录（GOTCHA #4）
//! - 递归：world_info_recursive 全局门；preventRecursion 条目内容不进递归扫描文本；
//!   delayUntilRecursion 支持 bool/数字多级（4641-4649：级别 N 在递归层级 ≥N 才放行，
//!   扫描结束后剩余层级驱动继续递归 5010-5014）
//! - key 匹配（4803/4835）：key/keysecondary 先过宏替换；/pattern/flags 正则 key 覆盖全局大小写/全词；无 * 通配
//! - 内容（4939 + 5086）：激活期宏替换；输出装配期过 WORLD_INFO 正则
//!   （depth=atDepth 条目的 depth，isPrompt:true）
//! - per-entry scanDepth（280）：覆盖全局扫描深度切窗口
//! - characterFilter（4726-4746）：names/tags 分别判，isExclude XOR included 则过滤
//! - 输出（5081-5145）：ANTop/ANBottom 独立块（由 AN 链合并）；EM 锚点条目按示例对话
//!   解析前后拼接；outlet 条目按 outletName 分桶（{{outlet::name}} 宏消费）；
//!   atDepth 条目按 (depth, role) 合并、桶内 unshift → 升序 join
//! - 插入组：groupOverride 优先（order 高者胜），否则 groupWeight 加权随机；
//!   useGroupScoring 按关键词命中数筛选，平分后再按优先项或权重选择。

use crate::macros::{evaluate_macros, MacroContext, MacroEnv};
use crate::rng;
use nast_model::regex_script::{RegexScript, RP_WORLD_INFO};
use nast_model::world::{TimedEffect, TimedWorldInfo, WIEntry, WorldInfoBook as WIBook};
use std::collections::{BTreeMap, HashMap, HashSet};

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
    pub generation_type: String,
    /// 最近的消息（旧→新；引擎取末尾 depth 条）
    pub chat: Vec<String>,
    /// 各可扫描字段（由条目的 match_* 开关决定是否并入）
    pub persona_description: String,
    pub char_description: String,
    pub char_personality: String,
    pub char_depth_prompt: String,
    pub scenario: String,
    /// 角色卡 creator notes（matchCreatorNotes）
    pub creator_notes: String,
    /// 当前角色头像文件名（characterFilter.names 匹配）
    pub char_file: String,
    /// 当前角色标签（characterFilter.tags 匹配）
    pub char_tags: Vec<String>,
    /// 注入扫描文本（AN 等允许扫描的扩展提示，buffer.addInject 语义）
    pub extra_scan: String,
}

/// 世界书来源集合（带书名；getSortedEntries 的组装结果由调用方完成）。
pub struct WiBooks<'a> {
    pub chat_lore: Vec<(&'a str, &'a WIBook)>,
    pub persona_lore: Vec<(&'a str, &'a WIBook)>,
    pub global_lore: Vec<(&'a str, &'a WIBook)>,
    pub character_lore: Vec<(&'a str, &'a WIBook)>,
}

/// 激活输出（getWorldInfoPrompt 等价）。
#[derive(Debug, Default, Clone)]
pub struct WiResult {
    /// position=0 (before_char) 块（升序，最高 order 邻近角色描述）
    pub world_info_before: String,
    /// position=1 (after_char) 块（同 unshift → 升序）
    pub world_info_after: String,
    /// atDepth 条目（已按 (depth,role) 合并，桶内升序 join；injection_order=100）
    pub depth_entries: Vec<DepthEntry>,
    /// EM 锚点条目（0=before examples / 1=after examples；升序）
    pub em_entries: Vec<(i64, String)>,
    /// ANTop 块（由 AN 链合并到 AN 文本上方）
    pub an_top: String,
    /// ANBottom 块
    pub an_bottom: String,
    /// outlet 条目（outletName → join 后内容；{{outlet::name}} 宏消费）
    pub outlet_entries: BTreeMap<String, String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanPhase {
    Initial,
    Recursion,
    MinActivations,
}

/// 主入口：等价 checkWorldInfo + getWorldInfoPrompt。
/// `max_context` 为本次生成的 context 上限（预算计算基准）；
/// `regex_scripts` 为输出装配期 WORLD_INFO pass 的脚本集合。
#[allow(clippy::too_many_arguments)]
pub fn check_world_info(
    books: &WiBooks,
    settings: &WiSettings,
    source: &ScanSource,
    state: &mut WiState,
    env: &MacroEnv,
    regex_scripts: &[RegexScript],
    max_context: i64,
) -> WiResult {
    check_world_info_with_context(books, settings, source, state, env, regex_scripts, max_context,
        &std::cell::RefCell::new(MacroContext::default()))
}

#[allow(clippy::too_many_arguments)]
pub fn check_world_info_with_context(
    books: &WiBooks, settings: &WiSettings, source: &ScanSource, state: &mut WiState,
    env: &MacroEnv, regex_scripts: &[RegexScript], max_context: i64,
    context: &std::cell::RefCell<MacroContext>,
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
    let mut seen = HashSet::new();
    all.retain(|(w, e)| seen.insert(format!("{w}#{}", e.uid)));

    check_timed_effects(state, &all);

    // ---------- 2. delay 层级（4646-4656） ----------
    let mut available_delay_levels: Vec<i64> = all
        .iter()
        .map(|(_, e)| e.delay_until_recursion.level())
        .filter(|l| *l > 0)
        .collect();
    available_delay_levels.sort_unstable();
    available_delay_levels.dedup();
    let mut current_delay_level = if available_delay_levels.is_empty() {
        0
    } else {
        available_delay_levels.remove(0)
    };

    // ---------- 3. 预算 ----------
    let mut budget = ((settings.budget as f64) * (max_context as f64) / 100.0).round() as i64;
    if budget < 1 {
        budget = 1;
    }
    if settings.budget_cap > 0 && budget > settings.budget_cap {
        budget = settings.budget_cap;
    }

    // ---------- 4. 状态机（INITIAL → RECURSION → MIN_ACTIVATIONS） ----------
    let mut all_activated: Vec<(String, WIEntry)> = Vec::new();
    let mut failed_probability: HashSet<String> = HashSet::new();
    let mut all_activated_text = String::new();
    let mut recurse_buffer: Vec<String> = Vec::new();
    let mut budget_overflowed = false;
    let mut skew: i64 = 0; // buffer.getDepth() = depth + skew
    let mut completed_passes: i64 = 0;
    let mut scan_phase = Some(ScanPhase::Initial);

    while let Some(phase) = scan_phase {
        // max_recursion_steps：非 0 时统计 pass 数封顶（同时禁用 min_activations）
        if settings.max_recursion_steps > 0 && completed_passes >= settings.max_recursion_steps {
            break;
        }
        completed_passes += 1;

        let mut new_entries: Vec<(String, WIEntry)> = Vec::new();

        for (world, entry) in &all {
            let key = format!("{world}.{}", entry.uid);
            if failed_probability.contains(&key)
                || all_activated
                    .iter()
                    .any(|(w, e)| w == world && e.uid == entry.uid)
            {
                continue;
            }
            if entry.disable {
                continue;
            }
            if let Some(triggers) = entry.extra.get("triggers").and_then(serde_json::Value::as_array) {
                if !triggers.is_empty() && !source.generation_type.is_empty()
                    && !triggers.iter().any(|trigger| trigger.as_str() == Some(source.generation_type.as_str())) {
                    continue;
                }
            }

            // characterFilter（4726-4746）：names/tags 分别判定
            if let Some(cf) = &entry.character_filter {
                if !cf.names.is_empty() {
                    let included = cf.names.iter().any(|n| n == &source.char_file);
                    let filtered = if cf.is_exclude { included } else { !included };
                    if filtered {
                        continue;
                    }
                }
                if !cf.tags.is_empty() {
                    let includes = source.char_tags.iter().any(|t| cf.tags.contains(t));
                    let filtered = if cf.is_exclude { includes } else { !includes };
                    if filtered {
                        continue;
                    }
                }
            }

            let entry_hash = string_hash_entry(entry);
            let cooldown_active = state
                .timed
                .cooldown
                .get(&key)
                .map(|rec| state.chat_length < rec.end && rec.hash == entry_hash)
                .unwrap_or(false);
            let sticky_active = state
                .timed
                .sticky
                .get(&key)
                .map(|rec| state.chat_length < rec.end && rec.hash == entry_hash)
                .unwrap_or(false);

            // delay：绝对消息数
            if entry.delay > 0 && state.chat_length < entry.delay {
                continue;
            }
            if cooldown_active && !sticky_active {
                continue;
            }

            // delayUntilRecursion 层级（4684-4694）
            let delay_level = entry.delay_until_recursion.level();
            if phase != ScanPhase::Recursion && delay_level > 0 && !sticky_active {
                continue;
            }
            if phase == ScanPhase::Recursion
                && delay_level > current_delay_level
                && !sticky_active
            {
                continue;
            }
            if phase == ScanPhase::Recursion
                && settings.recursive
                && entry.exclude_recursion
                && !sticky_active
            {
                continue;
            }

            // decorator：@@dont_activate / @@activate
            let (content_decorated, decorator_active) = parse_decorators(&entry.content);
            if decorator_active == Some(false) {
                continue;
            }

            // sticky 激活期内 / @@activate：跳过 key 直接成为候选
            if !sticky_active && decorator_active != Some(true) {
                // 扫描文本（per-entry scanDepth 覆盖全局窗口）
                let scan_text = build_scan_text(entry, source, settings.depth + skew, &recurse_buffer);
                // key 匹配（constant 条目跳过 key 检查）
                let primary_ok =
                    entry.constant || match_keys(&entry.key, &scan_text, entry, settings, env, context);
                if !primary_ok {
                    continue;
                }
                if entry.selective && !entry.keysecondary.is_empty() {
                    if !check_secondary(entry, &scan_text, settings, env, context) {
                        continue;
                    }
                }
            }

            let mut candidate = entry.clone();
            candidate.content = content_decorated;
            new_entries.push((world.clone(), candidate));
        }

        // ---------- 插入组过滤（filterByInclusionGroups，仅本 pass 候选） ----------
        filter_inclusion_groups(&mut new_entries, settings, &all_activated, state, &|entry| {
            let scan = build_scan_text(entry, source, settings.depth + skew, &recurse_buffer);
            let sensitive = entry.case_sensitive.unwrap_or(settings.case_sensitive);
            let whole = entry.match_whole_words.unwrap_or(settings.match_whole_words);
            let primary = entry.key.iter().filter(|key| key_matches(key, &scan, sensitive, whole)).count();
            let secondary = entry.keysecondary.iter().filter(|key| key_matches(key, &scan, sensitive, whole)).count();
            if entry.key.is_empty() { return 0; }
            primary + if entry.selective_logic == 0 || (entry.selective_logic == 3 && secondary == entry.keysecondary.len()) { secondary } else { 0 }
        });

        // ---------- probability + 预算 + 激活（4899-4958） ----------
        let text_to_scan_tokens = tok_cost(&all_activated_text);
        let mut new_content = String::new();
        let mut ignores_budget_left = new_entries.iter().filter(|(_, e)| e.ignore_budget).count();
        let mut successful_for_recurse: Vec<String> = Vec::new();

        for (world, entry) in &new_entries {
            if entry.ignore_budget {
                ignores_budget_left = ignores_budget_left.saturating_sub(1);
            }
            if budget_overflowed && !entry.ignore_budget {
                if ignores_budget_left > 0 {
                    continue;
                }
                break;
            }
            // probability（sticky 免掷骰）
            if entry.use_probability && entry.probability < 100 && !sticky_active_for(state, world, entry)
            {
                let roll = rand::random::<f64>() * 100.0;
                if roll > entry.probability as f64 {
                    failed_probability.insert(format!("{world}.{}", entry.uid));
                    continue;
                }
            }
            // 内容宏替换（激活期）
            let content = substitute_entry(env, context, &entry.content);
            new_content.push_str(&content);
            new_content.push('\n');
            // 预算：累计制 >= 溢出（D5）
            if !entry.ignore_budget
                && text_to_scan_tokens + tok_cost(&new_content) >= budget
            {
                budget_overflowed = true;
                result.budget_overflowed = true;
                continue;
            }
            // 激活
            arm_timed_effects(state, world, entry, entry_hash(entry));
            let mut activated = entry.clone();
            activated.content = content.clone();
            all_activated.push((world.clone(), activated));
            if !entry.prevent_recursion {
                successful_for_recurse.push(content);
            }
        }

        // ---------- 下一状态判定（5008-5032） ----------
        let mut next: Option<ScanPhase> = None;
        if settings.recursive
            && !budget_overflowed
            && !successful_for_recurse.is_empty()
        {
            next = Some(ScanPhase::Recursion);
        }
        if settings.recursive
            && !budget_overflowed
            && phase == ScanPhase::MinActivations
            && !recurse_buffer.is_empty()
        {
            next = Some(ScanPhase::Recursion);
        }
        if next.is_none()
            && !budget_overflowed
            && settings.min_activations > 0
            && all_activated.len() < settings.min_activations as usize
            && settings.max_recursion_steps == 0
        {
            let current_depth = settings.depth + skew;
            let over_max = (settings.min_activations_depth_max > 0
                && current_depth > settings.min_activations_depth_max)
                || current_depth > state.chat_length;
            if !over_max {
                next = Some(ScanPhase::MinActivations);
                skew += 1;
            }
        }
        if next.is_none() && !available_delay_levels.is_empty() {
            next = Some(ScanPhase::Recursion);
            current_delay_level = available_delay_levels.remove(0);
        }

        if next.is_some() {
            if !successful_for_recurse.is_empty() {
                let text = successful_for_recurse.join("\n");
                recurse_buffer.push(text.clone());
                all_activated_text = format!("{}\n{}", text, all_activated_text);
            }
        }
        scan_phase = next;
    }

    // ---------- 5. 输出装配（5081-5145，order 降序迭代 → unshift 翻转） ----------
    let mut sorted_final = all_activated.clone();
    sorted_final.sort_by(|a, b| b.1.order.cmp(&a.1.order));

    let mut before_list: Vec<String> = Vec::new();
    let mut after_list: Vec<String> = Vec::new();
    let mut em_list: Vec<(i64, String)> = Vec::new();
    let mut an_top_list: Vec<String> = Vec::new();
    let mut an_bottom_list: Vec<String> = Vec::new();
    let mut depth_map: BTreeMap<(i64, i64), Vec<String>> = BTreeMap::new();
    let mut outlet_map: BTreeMap<String, Vec<String>> = BTreeMap::new();

    let macro_fn = |s: &str| substitute_entry(env, context, s);
    for (_, entry) in &sorted_final {
        let (pos, at_d) = entry.position.resolve();
        let regex_depth = if pos == 4 {
            Some(at_d.as_ref().map(|(d, _)| *d).unwrap_or(entry.depth))
        } else {
            None
        };
        let params = crate::regex_engine::RegexParams {
            depth: regex_depth,
            is_prompt: true,
            ..Default::default()
        };
        let content = crate::regex_engine::get_regexed_string(
            &entry.content,
            RP_WORLD_INFO,
            regex_scripts,
            &params,
            &macro_fn,
        );
        if content.is_empty() {
            continue;
        }
        match pos {
            0 => before_list.insert(0, content),
            1 => after_list.insert(0, content),
            4 => {
                let (depth, role) = match &at_d {
                    Some((d, r)) => (*d, r.unwrap_or(entry.role.unwrap_or(0))),
                    None => (entry.depth, entry.role.unwrap_or(0)),
                };
                depth_map.entry((depth, role)).or_default().insert(0, content);
            }
            5 => em_list.insert(0, (0, content)),
            6 => em_list.insert(0, (1, content)),
            2 => an_top_list.insert(0, content),
            3 => an_bottom_list.insert(0, content),
            7 => {
                if let Some(name) = &entry.outlet_name {
                    if !name.is_empty() {
                        outlet_map.entry(name.clone()).or_default().push(content);
                    }
                }
            }
            _ => {}
        }
    }

    result.world_info_before = before_list.join("\n");
    result.world_info_after = after_list.join("\n");
    result.em_entries = em_list;
    result.an_top = an_top_list.join("\n");
    result.an_bottom = an_bottom_list.join("\n");
    for ((depth, role), contents) in depth_map {
        result.depth_entries.push(DepthEntry {
            depth,
            role,
            content: contents.join("\n"),
            order: 100, // 扩展注入默认 injection_order
        });
    }
    for (name, contents) in outlet_map {
        result.outlet_entries.insert(name, contents.join("\n"));
    }
    result.activated_count = all_activated.len();
    result
}

/// Evaluate once per generation, before filtering disabled/other-character entries.
fn check_timed_effects(state: &mut WiState, entries: &[(String, WIEntry)]) {
    for sticky in [true, false] {
        let records = if sticky { state.timed.sticky.clone() } else { state.timed.cooldown.clone() };
        for (key, record) in records {
            let entry = entries.iter().find(|(_, entry)| string_hash_entry(entry) == record.hash);
            let rewind = state.chat_length <= record.start && !record.protected;
            let expired = state.chat_length >= record.end;
            let invalid = entry.is_some_and(|(_, entry)| if sticky { entry.sticky <= 0 } else { entry.cooldown <= 0 });
            if rewind || expired || invalid {
                if sticky { state.timed.sticky.remove(&key); } else { state.timed.cooldown.remove(&key); }
                if sticky && expired && !rewind && !invalid {
                    if let Some((world, entry)) = entry.filter(|(_, entry)| entry.cooldown > 0) {
                        state.timed.cooldown.insert(format!("{world}.{}", entry.uid), TimedEffect {
                            hash: record.hash, start: state.chat_length,
                            end: state.chat_length + entry.cooldown, protected: true,
                        });
                    }
                }
            }
        }
    }
}

fn sticky_active_for(state: &WiState, world: &str, entry: &WIEntry) -> bool {
    let key = format!("{world}.{}", entry.uid);
    let entry_hash = string_hash_entry(entry);
    state
        .timed
        .sticky
        .get(&key)
        .map(|rec| state.chat_length < rec.end && rec.hash == entry_hash)
        .unwrap_or(false)
}

fn entry_hash(entry: &WIEntry) -> i64 {
    string_hash_entry(entry)
}

/// 激活时武装 timed effects（sticky 记录 / 纯 cooldown 记录）。
fn arm_timed_effects(state: &mut WiState, world: &str, entry: &WIEntry, entry_hash: i64) {
    let key = format!("{world}.{}", entry.uid);
    let chat_len = state.chat_length;
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
                    protected: false,
                },
            );
        }
    }
    if entry.cooldown > 0 {
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
        }
    }
}

/// 扫描文本：per-entry scanDepth 覆盖窗口 + 递归缓冲 + match_* 字段 + 注入文本。
fn build_scan_text(
    entry: &WIEntry,
    source: &ScanSource,
    global_depth: i64,
    recurse_buffer: &[String],
) -> String {
    let depth = entry
        .scan_depth
        .unwrap_or(global_depth)
        .clamp(0, nast_model::world::WI_MAX_SCAN_DEPTH)
        .max(0) as usize;
    let chat_len = source.chat.len();
    let start = chat_len.saturating_sub(depth);
    let mut parts: Vec<String> = vec![source.chat[start..].to_vec().join("\n")];
    for (flag, text) in [
        (entry.match_persona_description, &source.persona_description),
        (entry.match_character_description, &source.char_description),
        (entry.match_character_personality, &source.char_personality),
        (entry.match_character_depth_prompt, &source.char_depth_prompt),
        (entry.match_scenario, &source.scenario),
        (entry.match_creator_notes, &source.creator_notes),
    ] {
        if flag && !text.is_empty() {
            parts.push(text.clone());
        }
    }
    if !source.extra_scan.is_empty() {
        parts.push(source.extra_scan.clone());
    }
    for r in recurse_buffer {
        parts.push(r.clone());
    }
    parts.join("\n")
}

fn sorted_entries(
    books: &[(&str, &WIBook)],
    sort_fn: impl Fn(&WIEntry, &WIEntry) -> std::cmp::Ordering + Copy,
) -> Vec<(String, WIEntry)> {
    let mut out: Vec<(String, WIEntry)> = Vec::new();
    for (name, book) in books {
        for (_uid_s, entry) in &book.entries {
            let mut entry = entry.clone();
            entry.scan_hash = None;
            entry.world = Some(name.to_string());
            entry.scan_hash = Some(string_hash_entry(&entry));
            out.push((name.to_string(), entry));
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

/// 激活期内容宏替换。
fn substitute_entry(env: &MacroEnv, context: &std::cell::RefCell<MacroContext>, text: &str) -> String {
    evaluate_macros(text, env, &mut context.borrow_mut())
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

/// key 匹配：先宏替换（4803）；/pattern/flags 正则 key；多词/子串按全局大小写与全词设置。
fn match_keys(
    keys: &[String],
    scan: &str,
    entry: &WIEntry,
    settings: &WiSettings,
    env: &MacroEnv,
    context: &std::cell::RefCell<MacroContext>,
) -> bool {
    if keys.is_empty() {
        return false;
    }
    let case_sensitive = entry.case_sensitive.unwrap_or(settings.case_sensitive);
    let whole_words = entry.match_whole_words.unwrap_or(settings.match_whole_words);
    for key in keys {
        let key = substitute_entry(env, context, key);
        if key_matches(&key, scan, case_sensitive, whole_words) {
            return true;
        }
    }
    false
}

fn key_matches(key: &str, scan: &str, case_sensitive: bool, whole_words: bool) -> bool {
    // /pattern/flags → 正则 key（4762-4772 parseRegexFromString 语义）。
    // 正则 key 不受全局/条目大小写设置影响（覆盖语义），大小写只由自身 flags 决定。
    if key.starts_with('/') {
        if parse_regex_key(key).is_some() {
            return crate::regex_engine::ecma_is_match(key, scan).unwrap_or(false);
        }
    }
    let _ = case_sensitive;
    let scan_lc;
    let key_lc;
    let (scan_eff, key_eff) = if case_sensitive {
        (scan, key)
    } else {
        scan_lc = scan.to_lowercase();
        key_lc = key.to_lowercase();
        (scan_lc.as_str(), key_lc.as_str())
    };
    if whole_words && !key.contains(' ') {
        let re = regex::Regex::new(&format!(
            r"(?:^|\W)({})(?:$|\W)",
            regex::escape(key_eff)
        ))
        .unwrap();
        re.is_match(scan_eff)
    } else {
        scan_eff.contains(key_eff)
    }
}

/// "/pattern/flags" → (pattern, flags)；无闭合斜杠返回 None。
fn parse_regex_key(key: &str) -> Option<(String, String)> {
    let rest = key.strip_prefix('/')?;
    let last_slash = rest.rfind('/')?;
    let (pattern, flags) = rest.split_at(last_slash);
    let flags = &flags[1..];
    if pattern.is_empty() {
        return None;
    }
    if !flags.chars().all(|c| "dgimsuvy".contains(c)) {
        return None;
    }
    Some((pattern.to_string(), flags.to_string()))
}

/// secondary logic 判定（4835：keysecondary 先宏替换）：0 AND_ANY / 1 NOT_ALL / 2 NOT_ANY / 3 AND_ALL。
fn check_secondary(
    entry: &WIEntry,
    scan: &str,
    settings: &WiSettings,
    env: &MacroEnv,
    context: &std::cell::RefCell<MacroContext>,
) -> bool {
    let case_sensitive = entry.case_sensitive.unwrap_or(settings.case_sensitive);
    let whole_words = entry.match_whole_words.unwrap_or(settings.match_whole_words);
    let hits: Vec<bool> = entry
        .keysecondary
        .iter()
        .map(|k| {
            let k = substitute_entry(env, context, k);
            key_matches(&k, scan, case_sensitive, whole_words)
        })
        .collect();
    match entry.selective_logic {
        1 => !hits.iter().all(|h| *h),           // NOT_ALL
        2 => !hits.iter().any(|h| *h),           // NOT_ANY
        3 => hits.iter().all(|h| *h),            // AND_ALL
        _ => hits.iter().any(|h| *h),            // AND_ANY
    }
}

/// 插入组过滤（filterByInclusionGroups，本 pass 候选）：
/// groupOverride 优先（order 高者胜）；否则 groupWeight 加权随机；
/// useGroupScoring 先移除低分条目；未启用评分的条目保留参与选择。
fn filter_inclusion_groups(candidates: &mut Vec<(String, WIEntry)>, settings: &WiSettings,
    activated: &[(String, WIEntry)], state: &WiState, score: &dyn Fn(&WIEntry) -> usize) {
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    let mut keep = vec![true; candidates.len()];
    for (i, (_, e)) in candidates.iter().enumerate() {
        for g in e.group.split(',') {
            let g = g.trim();
            if !g.is_empty() {
                groups.entry(g.to_string()).or_default().push(i);
            }
        }
    }
    for (group_name, mut members) in groups {
        let sticky: Vec<usize> = members.iter().copied().filter(|i|
            sticky_active_for(state, &candidates[*i].0, &candidates[*i].1)).collect();
        if !sticky.is_empty() {
            for i in members { if !sticky.contains(&i) { keep[i] = false; } }
            continue;
        }
        if activated.iter().any(|(_, entry)| entry.group == group_name) {
            for i in members { keep[i] = false; }
            continue;
        }
        if members.len() <= 1 {
            continue;
        }
        let use_scoring = settings.use_group_scoring
            || members
                .iter()
                .any(|i| candidates[*i].1.use_group_scoring.unwrap_or(false));
        if use_scoring {
            let max_score = members.iter().map(|i| score(&candidates[*i].1)).max().unwrap_or(0);
            members.retain(|i| {
                let entry = &candidates[*i].1;
                let retain = !entry.use_group_scoring.unwrap_or(settings.use_group_scoring) || score(entry) == max_score;
                if !retain { keep[*i] = false; }
                retain
            });
        }
        if members.len() <= 1 { continue; }
        let winner: Option<usize> = {
            let override_member = members
                .iter()
                .copied()
                .filter(|i| candidates[*i].1.group_override)
                .max_by_key(|i| candidates[*i].1.order);
            if override_member.is_some() {
                override_member
            } else {
                // 加权随机
                let total: i64 = members
                    .iter()
                    .map(|i| candidates[*i].1.group_weight.max(0))
                    .sum();
                if total <= 0 {
                    None
                } else {
                    let pick = (rand::random::<f64>() * total as f64) as i64;
                    let mut acc = 0;
                    let mut chosen = None;
                    for i in &members {
                        acc += candidates[*i].1.group_weight.max(0);
                        if pick < acc {
                            chosen = Some(*i);
                            break;
                        }
                    }
                    chosen
                }
            }
        };
        for i in members {
            keep[i] &= winner == Some(i);
        }
    }
    let mut out = Vec::new();
    for (i, item) in candidates.drain(..).enumerate() {
        if keep[i] {
            out.push(item);
        }
    }
    *candidates = out;
}

fn tok_cost(s: &str) -> i64 {
    crate::tokens::count_tokens(s, crate::tokens::resolve_tokenizer("gpt-4o")) as i64
}

/// Stable identity of normalized entry data; ST's raw JSON field order is not retained.
fn string_hash_entry(entry: &WIEntry) -> i64 {
    if let Some(hash) = entry.scan_hash { return hash; }
    // serde 序列化（BTreeMap 字段序稳定）；与 JS 的字段序不同但 hash 失效判定
    // 只要求「同 entry 同 hash、entry 变更后 hash 变」，故实现内自洽即可
    let json = serde_json::to_string(entry).unwrap_or_default();
    rng::string_hash(&json)
}

/// 从书中提取匹配条目（供测试注入简化）。
pub fn book_entries(book: &WIBook) -> Vec<(String, WIEntry)> {
    book.entries
        .iter()
        .map(|(k, e)| (k.clone(), e.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regex_key_flags_parsing() {
        assert_eq!(
            parse_regex_key("/abc/i"),
            Some(("abc".to_string(), "i".to_string()))
        );
        assert_eq!(
            parse_regex_key("/a\\/b/"),
            Some(("a\\/b".to_string(), "".to_string()))
        );
        assert_eq!(parse_regex_key("/abc"), None);
        assert_eq!(parse_regex_key("abc"), None);
    }
}
