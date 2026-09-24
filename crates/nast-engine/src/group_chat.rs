//! 群聊调度：复刻 group-chats.js 的激活策略与卡片合并（1.18.0）。
//!
//! 契约（GOTCHA #13/#14）：
//! - 激活策略（谁回复）：NATURAL=0 提及词命中 + talkativeness 概率 roll +
//!   禁最后发言者（除非 allow_self_responses）+ 随机兜底；LIST=1 全员按序；
//!   MANUAL=2 非用户触发时随机一人；POOLED=3 自上次用户消息未发言者优先
//! - 生成模式（卡片怎么用）：SWAP=0 用被选中成员的卡；APPEND=1 拼接全部启用成员
//!   字段（join prefix/suffix 包裹）；APPEND_DISABLED=2 连禁言成员也拼
//! - 成员深度提示：SWAP 只取选中成员；APPEND 取全部启用成员
//! - 开场白：随机取 [first_mes, ...alternate_greetings]
//! - 群批次 gen_id：每批一个 Date.now() 值

use nast_model::card::Character;
use nast_model::group::{Group, GA_LIST, GA_MANUAL, GA_NATURAL, GA_POOLED};
use rand::RngCore;

/// [0, n) 随机整数（避开 gen 保留字，封装 RngCore）。
fn rng_range(rng: &mut impl RngCore, n: usize) -> usize {
    (rng.next_u64() % n as u64) as usize
}

/// [0,1) 随机浮点。
fn rng_f64(rng: &mut impl RngCore) -> f64 {
    (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

/// 成员上下文（调度所需的角色数据切片）。
#[derive(Debug, Clone)]
pub struct GroupMember {
    pub avatar: String,
    pub name: String,
    /// 0..1；NaN → 默认 0.5
    pub talkativeness: f64,
}

/// NATURAL 激活：提及 + talkativeness roll + 禁最后发言者 + 随机兜底。
pub fn activate_natural(
    group: &Group,
    members: &[GroupMember],
    input_words: &[String],
    last_speaker: Option<&str>,
    rng: &mut impl RngCore,
) -> Vec<String> {
    let mut activated: Vec<String> = Vec::new();
    let allow_self = group.allow_self_responses;

    let candidates: Vec<&GroupMember> = members
        .iter()
        .filter(|m| !group.disabled_members.contains(&m.avatar))
        .filter(|m| {
            if !allow_self {
                last_speaker.map(|ls| m.name != ls).unwrap_or(true)
            } else {
                true
            }
        })
        .collect();

    // 1. 提及命中：输入词中包含成员名（大小写不敏感）
    for m in &candidates {
        let mentioned = input_words
            .iter()
            .any(|w| w.to_lowercase() == m.name.to_lowercase());
        if mentioned {
            activated.push(m.avatar.clone());
        }
    }

    // 2. talkativeness 概率 roll（洗牌后的顺序）
    let mut shuffled: Vec<&GroupMember> = candidates.clone();
    shuffle_refs(&mut shuffled, rng);
    for m in shuffled {
        let talk = if m.talkativeness.is_nan() { 0.5 } else { m.talkativeness };
        if rng_f64(rng) < talk && !activated.contains(&m.avatar) {
            activated.push(m.avatar.clone());
        }
    }

    // 3. 兜底：无人激活 → 从 talkativeness > 0 的成员中随机挑一个
    if activated.is_empty() {
        let pool: Vec<&GroupMember> = candidates
            .iter()
            .copied()
            .filter(|m| m.talkativeness > 0.0)
            .collect();
        let pool = if pool.is_empty() { candidates.clone() } else { pool };
        if let Some(m) = pool.get(rng_range(rng, pool.len().max(1))) {
            activated.push(m.avatar.clone());
        }
    }
    activated
}

/// LIST 激活：全部启用成员，按 members 数组顺序。
pub fn activate_list(group: &Group, members: &[GroupMember]) -> Vec<String> {
    members
        .iter()
        .filter(|m| !group.disabled_members.contains(&m.avatar))
        .map(|m| m.avatar.clone())
        .collect()
}

/// MANUAL 激活：非用户触发 → 随机一人。
pub fn activate_manual(group: &Group, members: &[GroupMember], rng: &mut impl RngCore) -> Vec<String> {
    let candidates: Vec<&GroupMember> = members
        .iter()
        .filter(|m| !group.disabled_members.contains(&m.avatar))
        .collect();
    if candidates.is_empty() {
        return vec![];
    }
    let m = &candidates[rng_range(rng, candidates.len())];
    let _ = GA_NATURAL;
    vec![m.avatar.clone()]
}

/// POOLED 激活：自上次用户消息后未发言者优先（随机），否则排除最后发言者随机。
pub fn activate_pooled(
    group: &Group,
    members: &[GroupMember],
    unspoken_since_user: &[String],
    last_speaker: Option<&str>,
    rng: &mut impl RngCore,
) -> Vec<String> {
    let enabled: Vec<&GroupMember> = members
        .iter()
        .filter(|m| !group.disabled_members.contains(&m.avatar))
        .collect();
    // 优先：未发言者
    let unspoken: Vec<&GroupMember> = enabled
        .iter()
        .filter(|m| unspoken_since_user.contains(&m.avatar))
        .copied()
        .collect();
    let pool: Vec<&GroupMember> = if !unspoken.is_empty() {
        unspoken
    } else {
        enabled
            .iter()
            .filter(|m| last_speaker.map(|ls| m.name != ls).unwrap_or(true))
            .copied()
            .collect()
    };
    if pool.is_empty() {
        return vec![];
    }
    let m = &pool[rng_range(rng, pool.len())];
    vec![m.avatar.clone()]
}

/// 统一调度入口（generateGroupWrapper 的激活部分）。
/// `is_user_input`：本轮是否由用户消息触发。
pub fn activate_members(
    group: &Group,
    members: &[GroupMember],
    input: &str,
    last_speaker: Option<&str>,
    unspoken_since_user: &[String],
    is_user_input: bool,
    rng: &mut impl RngCore,
) -> Vec<String> {
    let words: Vec<String> = input
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | '.' | '!' | '?' | ';' | ':'))
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    match group.activation_strategy {
        GA_NATURAL => {
            activate_natural(group, members, &words, last_speaker, rng)
        }
        GA_LIST => activate_list(group, members),
        GA_MANUAL => {
            if is_user_input {
                // MANUAL 策略下用户触发不自动选人（仅非用户触发或 force）
                vec![]
            } else {
                activate_manual(group, members, rng)
            }
        }
        GA_POOLED => {
            activate_pooled(group, members, unspoken_since_user, last_speaker, rng)
        }
        _ => vec![],
    }
}

// ---------- 卡片合并（APPEND 模式） ----------

/// ST's FIELDNAME is the field label; character macros use each contributing member.
pub fn append_field(group: &Group, members: &[(GroupMember, Character)], selected: &str,
    label: &str, field: fn(&Character) -> &str) -> String {
    members.iter().filter(|(member,_)| member.avatar == selected || group.generation_mode == 2
        || !group.disabled_members.contains(&member.avatar)).filter_map(|(member, character)| {
        let mut value = field(character).trim().to_string();
        if value.is_empty() { return None; }
        if label == "Example Messages" && !value.starts_with("<START>") { value = format!("<START>\n{value}"); }
        let transform = |text: &str| {
            let field_re = regex::Regex::new("(?i)<FIELDNAME>").unwrap();
            field_re.replace_all(text, label).replace("{{char}}", &member.name)
        };
        Some(format!("{}{}{}", transform(group.generation_mode_join_prefix.as_deref().unwrap_or("")),
            transform(&value), transform(group.generation_mode_join_suffix.as_deref().unwrap_or(""))))
    }).collect::<Vec<_>>().join("\n")
}

/// 合并成员卡片字段（getGroupCharacterCards 语义）。
/// 每个成员字段用 join_prefix/suffix 包裹，<FIELDNAME> 替换为成员名，宏由调用方处理。
pub fn append_cards(
    group: &Group,
    members: &[(GroupMember, Character)],
    field: fn(&Character) -> &str,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (member, ch) in members {
        let is_disabled = group.disabled_members.contains(&member.avatar);
        if is_disabled && group.generation_mode != 2 {
            continue; // APPEND_DISABLED(2) 才拼禁言成员
        }
        let value = field(ch);
        if value.trim().is_empty() {
            continue;
        }
        let prefix = group.generation_mode_join_prefix.as_deref().unwrap_or("");
        let suffix = group.generation_mode_join_suffix.as_deref().unwrap_or("");
        let wrapped = format!(
            "{}{}{}",
            prefix.replace("<FIELDNAME>", &member.name),
            value,
            suffix.replace("<FIELDNAME>", &member.name)
        );
        parts.push(wrapped);
    }
    parts.join("\n")
}

/// 收集成员深度提示（getGroupDepthPrompts：SWAP 模式为空）。
pub fn group_depth_prompts(
    group: &Group,
    members: &[(GroupMember, Character)],
) -> Vec<(i64, String, String)> {
    group_depth_prompts_for(group, members, None)
}

pub fn group_depth_prompts_for(
    group: &Group, members: &[(GroupMember, Character)], current_avatar: Option<&str>,
) -> Vec<(i64, String, String)> {
    // (depth, role, prompt)
    if group.generation_mode == 0 {
        return vec![]; // SWAP：不收
    }
    let mut out = Vec::new();
    for (member, ch) in members {
        if group.disabled_members.contains(&member.avatar) && group.generation_mode != 2
            && current_avatar != Some(member.avatar.as_str()) {
            continue;
        }
        if let Some(dp) = &ch.data.extensions.depth_prompt {
            if !dp.prompt.is_empty() {
                out.push((dp.depth, dp.role.clone(), dp.prompt.clone()));
            }
        }
    }
    out
}

/// 开场白：随机取 [first_mes, ...alternate_greetings]。
pub fn pick_greeting(ch: &Character, rng: &mut impl RngCore) -> String {
    let mut greetings = vec![ch.first_mes.clone()];
    greetings.extend(ch.data.alternate_greetings.clone());
    let idx = rng_range(rng, greetings.len());
    greetings.remove(idx)
}

fn shuffle_refs<'a, T: 'a>(v: &mut Vec<&'a T>, rng: &mut impl RngCore) {
    for i in (1..v.len()).rev() {
        v.swap(i, rng_range(rng, i + 1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;


    fn member(avatar: &str, name: &str, talk: f64) -> GroupMember {
        GroupMember { avatar: avatar.into(), name: name.into(), talkativeness: talk }
    }

    fn group(mode: i64) -> Group {
        Group {
            activation_strategy: mode,
            ..Default::default()
        }
    }

    #[test]
    fn list_activates_all_in_order() {
        let g = group(1); // LIST
        let members = vec![
            member("a.png", "A", 0.0),
            member("b.png", "B", 0.0),
            member("c.png", "C", 0.0),
        ];
        let out = activate_list(&g, &members);
        assert_eq!(out, vec!["a.png", "b.png", "c.png"]);
    }

    #[test]
    fn list_skips_disabled() {
        let mut g = group(1);
        g.disabled_members = vec!["b.png".into()];
        let members = vec![member("a.png", "A", 0.0), member("b.png", "B", 0.0)];
        let out = activate_list(&g, &members);
        assert_eq!(out, vec!["a.png"]);
    }

    #[test]
    fn natural_mention_overrides_everything() {
        let mut g = group(0);
        g.allow_self_responses = false;
        let members = vec![
            member("a.png", "Seraphina", 0.0), // talk 0 → 只能靠提及
            member("b.png", "Other", 0.0),
        ];
        let mut rng = StdRng::seed_from_u64(42);
        let out = activate_natural(&g, &members, &["seraphina".to_string()], None, &mut rng);
        assert_eq!(out, vec!["a.png"]);
    }

    #[test]
    fn natural_bans_last_speaker() {
        let mut g = group(0);
        g.allow_self_responses = false;
        let members = vec![member("a.png", "A", 1.0)];
        let mut rng = StdRng::seed_from_u64(42);
        // A 是最后发言者且 talk=1（无提及）→ 兜底也不选 A？
        // 兜底池 = 候选（已排除 A）→ 空 → 无激活
        let out = activate_natural(&g, &members, &[], Some("A"), &mut rng);
        assert_eq!(out, Vec::<String>::new());
        // allow_self_responses = true → 可以连发
        g.allow_self_responses = true;
        let out = activate_natural(&g, &members, &[], Some("A"), &mut rng);
        assert_eq!(out, vec!["a.png"]);
    }

    #[test]
    fn natural_talkativeness_roll() {
        let g = group(0);
        let members = vec![
            member("a.png", "A", 1.0),  // 必激活
            member("b.png", "B", 0.0),  // 永不激活
            member("c.png", "C", 0.5),  // 50%
        ];
        let mut rng = StdRng::seed_from_u64(7);
        let out = activate_natural(&g, &members, &[], None, &mut rng);
        assert!(out.contains(&"a.png".to_string()));
        assert!(!out.contains(&"b.png".to_string()));
    }

    #[test]
    fn pooled_prefers_unspoken() {
        let g = group(3);
        let members = vec![
            member("a.png", "A", 1.0),
            member("b.png", "B", 1.0),
        ];
        let mut rng = StdRng::seed_from_u64(3);
        // A 已发言 → B 优先
        let out = activate_pooled(&g, &members, &["b.png".to_string()], Some("A"), &mut rng);
        assert_eq!(out, vec!["b.png"]);
    }

    #[test]
    fn manual_picks_one() {
        let g = group(2);
        let members = vec![member("a.png", "A", 1.0), member("b.png", "B", 1.0)];
        let mut rng = StdRng::seed_from_u64(1);
        let out = activate_manual(&g, &members, &mut rng);
        assert_eq!(out.len(), 1);
        assert!(["a.png", "b.png"].contains(&out[0].as_str()));
    }

    #[test]
    fn append_merges_with_wrappers() {
        let mut g = group(1); // APPEND
        g.generation_mode_join_prefix = Some("[[".into());
        g.generation_mode_join_suffix = Some("]]".into());
        let a = member("a.png", "A", 1.0);
        let mut ch_a = Character::default();
        ch_a.name = "A".into();
        ch_a.data.description = "desc A".into();
        let b = member("b.png", "B", 1.0);
        let mut ch_b = Character::default();
        ch_b.name = "B".into();
        ch_b.data.description = "desc B".into();
        let out = append_cards(&g, &[(a, ch_a), (b, ch_b)], |c| &c.data.description);
        assert!(out.contains("[[desc A]]"));
        assert!(out.contains("[[desc B]]"));
    }

    #[test]
    fn append_skips_disabled_unless_mode2() {
        let mut g = group(1); // APPEND
        g.disabled_members = vec!["b.png".into()];
        let a = member("a.png", "A", 1.0);
        let mut ch_a = Character::default();
        ch_a.data.description = "desc A".into();
        let b = member("b.png", "B", 1.0);
        let mut ch_b = Character::default();
        ch_b.data.description = "desc B".into();

        let out = append_cards(&g, &[(a.clone(), ch_a.clone()), (b.clone(), ch_b.clone())], |c| &c.data.description);
        assert!(out.contains("desc A"));
        assert!(!out.contains("desc B"));

        // APPEND_DISABLED
        g.generation_mode = 2;
        let out = append_cards(&g, &[(a, ch_a), (b, ch_b)], |c| &c.data.description);
        assert!(out.contains("desc B"));
    }

    #[test]
    fn depth_prompts_swaps_vs_append() {
        let mut g = group(0); // SWAP
        g.disabled_members = vec![];
        let a = member("a.png", "A", 1.0);
        let mut ch_a = Character::default();
        ch_a.data.extensions.depth_prompt = Some(nast_model::card::DepthPrompt {
            prompt: "A depth".into(),
            depth: 2,
            role: "system".into(),
        });
        let b = member("b.png", "B", 1.0);
        let mut ch_b = Character::default();
        ch_b.data.extensions.depth_prompt = Some(nast_model::card::DepthPrompt {
            prompt: "B depth".into(),
            depth: 4,
            role: "system".into(),
        });
        let members = vec![(a.clone(), ch_a.clone()), (b.clone(), ch_b.clone())];
        assert!(group_depth_prompts(&g, &members).is_empty());
        g.generation_mode = 1; // APPEND
        let out = group_depth_prompts(&g, &members);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn greeting_random_from_alternates() {
        let mut ch = Character::default();
        ch.first_mes = "main greeting".into();
        ch.data.alternate_greetings = vec!["alt 1".into(), "alt 2".into()];
        let mut rng = StdRng::seed_from_u64(99);
        for _ in 0..20 {
            let g = pick_greeting(&ch, &mut rng);
            assert!(["main greeting", "alt 1", "alt 2"].contains(&g.as_str()));
        }
    }
}
