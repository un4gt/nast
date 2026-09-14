//! 群生成 RPC 处理：generate_group 流程。
//!
//! 契约（group-chats.js generateGroupWrapper + src/endpoints/chats.js group 部分）：
//! - 群聊天文件平铺于 "group chats/<chat_id>.jsonl"，首行 header 含 integrity
//! - 新群聊天：每个成员随机一条开场白（含 force_avatar/original_avatar）
//! - 用户消息先落盘；激活策略选成员；逐成员生成并落盘（extra.gen_id 批次标记）
//! - 事件：group_member_drafted → stream/message_received 等

use crate::generate::{GenerateParams, GenerateSession};
use crate::rpc::{read_character, RpcError, RpcResult};
use crate::state::SharedState;
use nast_model::card::Character;
use nast_model::chat::{ChatFile, ChatHeader, ChatMetadata, GenerationType, MessageExtra};
use nast_engine::group_chat::GroupMember;
use rand::SeedableRng;
use nast_model::group::Group;
use serde_json::{json, Value};

pub async fn generate_group(state: SharedState, params: Value) -> RpcResult {
    let group_id = param_str(&params, "id")?;
    let chat_id = param_str(&params, "chat_id")?;
    let groups = state.user.list_groups()?;
    let group = groups
        .iter()
        .find(|g| g.id == group_id)
        .ok_or_else(|| RpcError::NotFound(format!("group {group_id}")))?
        .clone();

    // 读群聊天文件（无则用成员开场白初始化）
    let mut chat = match state.user.read_group_chat(chat_id) {
        Ok(c) => c,
        Err(_) => init_group_chat(&state, &group, chat_id)?,
    };

    // 成员数据
    let mut members: Vec<(GroupMember, Character)> = Vec::new();
    for avatar in &group.members {
        if let Ok(ch) = read_character(&state, avatar) {
            let talk = ch
                .talkativeness
                .as_deref()
                .and_then(|t| t.parse::<f64>().ok())
                .unwrap_or(0.5);
            members.push((
                GroupMember {
                    avatar: avatar.clone(),
                    name: ch.name.clone(),
                    talkativeness: talk,
                },
                ch,
            ));
        }
    }

    // 用户消息（可选）先落盘
    let user_message = params
        .get("user_message")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !user_message.is_empty() {
        let msg = json!({
            "name": "User",
            "is_user": true,
            "is_system": false,
            "send_date": nast_storage::message_time_stamp(),
            "mes": user_message,
        });
        chat.0.push(msg);
        state.user.save_group_chat(chat_id, &chat, false)?;
    }

    // 最后发言者 & 自上次用户消息后的未发言成员
    let last_speaker = find_last_speaker(&chat);
    let unspoken = unspoken_since_user(&chat, &members);

    // 激活成员
    let mut rng = rand::rngs::StdRng::from_seed(rand::random());
    let member_only: Vec<GroupMember> = members.iter().map(|(gm, _)| gm.clone()).collect();
    let activated = nast_engine::group_chat::activate_members(
        &group,
        &member_only,
        user_message,
        last_speaker.as_deref(),
        &unspoken,
        !user_message.is_empty(),
        &mut rng,
    );
    if activated.is_empty() {
        state.user.save_group_chat(chat_id, &chat, false)?;
        return Ok(json!({"activated": [], "chat_id": chat_id}));
    }

    // 生成会话
    let abort = tokio_util::sync::CancellationToken::new();
    {
        let mut guard = state.generation.write().await;
        guard.abort = Some(abort.clone());
    }
    let oai: nast_model::preset::OaiSettings = serde_json::from_value(
        state
            .settings
            .read()
            .await
            .get("oai_settings")
            .cloned()
            .unwrap_or(json!({})),
    )
    .unwrap_or_default();
    let provider = make_provider(&oai);
    let settings_snapshot = state.settings.read().await.clone();
    let session = GenerateSession {
        user: &state.user,
        hub: &state.hub,
        oai,
        settings_json: &settings_snapshot,
        provider,
        abort,
        plugins: &state.plugins,
    };

    // 逐成员生成
    let gen_id = chrono::Utc::now().timestamp_millis();
    let mut replies: Vec<Value> = Vec::new();
    for avatar in &activated {
        let (member, member_ch) = members
            .iter()
            .find(|(gm, _)| &gm.avatar == avatar)
            .ok_or_else(|| RpcError::Internal("member vanished".into()))?;
        state
            .hub
            .emit("group_member_drafted", json!(member_ch.name));

        let p = GenerateParams {
            generation_type: GenerationType::Normal,
            avatar: avatar.clone(),
            chat_file: String::new(),
            user_message: String::new(),
            character: member_ch.clone(),
            persona_description: String::new(),
            persona_position_in_prompt: true,
            is_group: true,
            extra_injections: Vec::new(),
        };
        let history: Vec<nast_model::chat::ChatMessage> = chat
            .0
            .iter()
            .skip(1)
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect();

        let input =
            build_group_input(&session, &p, &history, &group, members.as_slice(), member);
        let assembled = crate::prompt_bridge::assemble_with_macros(&session.oai, &input);
        let text = session
            .call_provider(&assembled, None)
            .await
            .map_err(RpcError::Internal)?;

        // 落盘群消息（gen_id 批次 + 身份字段）
        let msg = json!({
            "name": member_ch.name,
            "is_user": false,
            "is_system": false,
            "send_date": nast_storage::message_time_stamp(),
            "mes": text,
            "force_avatar": format!("thumbnail?type=avatar&file={avatar}"),
            "original_avatar": avatar,
            "extra": {
                "gen_id": gen_id,
                "api": session.oai.chat_completion_source,
                "model": session.oai.openai_model,
                "gen_started": nast_storage::message_time_stamp(),
            },
        });
        chat.0.push(msg.clone());
        state.user.save_group_chat(chat_id, &chat, false)?;
        let chat_idx = chat.0.len() as i64 - 1;
        state.hub.emit("message_received", json!(chat_idx));
        state.hub.emit("character_message_rendered", json!(chat_idx));
        replies.push(json!({"name": member_ch.name, "text": text}));
    }
    {
        let mut guard = state.generation.write().await;
        guard.abort = None;
    }
    Ok(json!({"activated": activated, "replies": replies, "chat_id": chat_id}))
}

/// 群输入构建：APPEND 模式合并成员描述/个性/场景，SWAP 用被选中成员的卡。
fn build_group_input<'a>(
    session: &'a GenerateSession,
    p: &'a GenerateParams,
    history: &'a [nast_model::chat::ChatMessage],
    group: &'a Group,
    members: &'a [(GroupMember, Character)],
    selected: &'a GroupMember,
) -> crate::prompt_bridge::BridgeInput<'a> {
    let metadata = session
        .current_chat_metadata(&p.avatar, &p.chat_file)
        .unwrap_or_default();
    let regex_scripts = session.collect_regex_scripts(&p.character, &metadata);
    let total = history.len();

    let messages: Vec<crate::prompt_bridge::HistoryMessage> = history
        .iter()
        .filter(|m| !m.is_system)
        .enumerate()
        .map(|(idx, m)| {
            let depth = (total - 1 - idx) as i64;
            let placement = if m.is_user {
                nast_model::regex_script::RP_USER_INPUT
            } else {
                nast_model::regex_script::RP_AI_OUTPUT
            };
            let macro_fn =
                |s: &str| crate::prompt_bridge::substitute_basic(s, "User", &p.character.name);
            let params = nast_engine::regex_engine::RegexParams {
                depth: Some(depth),
                is_prompt: true,
                ..Default::default()
            };
            let role = if m.is_user { "user" } else { "assistant" };
            let mut content = nast_engine::regex_engine::get_regexed_string(
                &m.mes,
                placement,
                &regex_scripts,
                &params,
                &macro_fn,
            );
            let is_narrator = m.extra.kind.as_deref() == Some("narrator");
            let names_behavior = session.oai.character_names_behavior;
            if names_behavior == 2 && !is_narrator {
                content = format!("{}: {}", m.name, content);
            } else if names_behavior == 0 && m.name != "User" && !is_narrator {
                // 群聊中非用户消息全部带名（DEFAULT 语义的群分支）
                content = format!("{}: {}", m.name, content);
            }
            content = content.replace(String::from("\r").as_str(), "");
            crate::prompt_bridge::HistoryMessage {
                role: role.into(),
                content,
                name: None,
                is_narrator,
                injected: false,
            }
        })
        .collect();

    // 卡片字段：SWAP 用选中成员；APPEND 拼接全部
    let (description, personality, scenario) = if group.generation_mode == 0 {
        (
            p.character.data.description.clone(),
            p.character.data.personality.clone(),
            p.character.data.scenario.clone(),
        )
    } else {
        let pairs: Vec<(GroupMember, Character)> = members
            .iter()
            .map(|(gm, ch)| (gm.clone(), ch.clone()))
            .collect();
        (
            nast_engine::group_chat::append_cards(group, &pairs, |c| &c.data.description),
            nast_engine::group_chat::append_cards(group, &pairs, |c| &c.data.personality),
            nast_engine::group_chat::append_cards(group, &pairs, |c| &c.data.scenario),
        )
    };

    // 成员深度提示（SWAP 为空）
    let mut injections: Vec<crate::prompt_bridge::InChatInjection> = Vec::new();
    if group.generation_mode != 0 {
        for (depth, role, prompt) in
            nast_engine::group_chat::group_depth_prompts(group, members)
        {
            let role_num = match role.as_str() {
                "user" => 1,
                "assistant" => 2,
                _ => 0,
            };
            injections.push(crate::prompt_bridge::InChatInjection {
                content: prompt,
                depth,
                role: role_num,
                injection_order: 100,
            });
        }
    }
    injections.extend(crate::generate::build_injections(p));

    crate::prompt_bridge::BridgeInput {
        oai: &session.oai,
        generation_type: p.generation_type.as_str(),
        name1: "User",
        name2: &selected.name,
        is_group: true,
        char_description: description,
        char_personality: personality,
        scenario,
        persona_description: String::new(),
        persona_position_in_prompt: false, // 群聊 persona 不进 prompt（ST 默认仅单人）
        world_info_before: String::new(),
        world_info_after: String::new(),
        messages,
        message_examples: crate::generate::parse_examples(&p.character.data.mes_example),
        pin_examples: false,
        in_chat_injections: injections,
        system_prompt_override: {
            let sp = &p.character.data.system_prompt;
            if !sp.is_empty() { Some(sp.clone()) } else { None }
        },
        jailbreak_prompt_override: {
            let phi = &p.character.data.post_history_instructions;
            if !phi.is_empty() { Some(phi.clone()) } else { None }
        },
        cycle_prompt: None,
        last_role: None,
    }
}

fn init_group_chat(
    state: &SharedState,
    group: &Group,
    chat_id: &str,
) -> Result<ChatFile, RpcError> {
    let metadata = ChatMetadata {
        integrity: Some(uuid::Uuid::new_v4().to_string()),
        ..Default::default()
    };
    let header = ChatHeader {
        user_name: "unused".into(),
        character_name: "unused".into(),
        chat_metadata: metadata,
    };
    let mut chat = ChatFile(vec![serde_json::to_value(&header).unwrap()]);
    let mut rng = rand::rngs::StdRng::from_seed(rand::random());
    for avatar in &group.members {
        if let Ok(ch) = read_character(state, avatar) {
            let greeting = nast_engine::group_chat::pick_greeting(&ch, &mut rng);
            let msg = json!({
                "name": ch.name,
                "is_user": false,
                "is_system": false,
                "send_date": nast_storage::message_time_stamp(),
                "mes": greeting,
                "force_avatar": format!("thumbnail?type=avatar&file={avatar}"),
                "original_avatar": avatar,
                "extra": {"gen_id": null},
            });
            chat.0.push(msg);
        }
    }
    state.user.save_group_chat(chat_id, &chat, false)?;
    Ok(chat)
}

fn find_last_speaker(chat: &ChatFile) -> Option<String> {
    chat.0
        .iter()
        .skip(1)
        .rev()
        .find(|m| m.get("is_user").and_then(|v| v.as_bool()) == Some(false))
        .and_then(|m| m.get("name"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string())
}

fn unspoken_since_user(chat: &ChatFile, members: &[(GroupMember, Character)]) -> Vec<String> {
    let mut spoken: Vec<String> = Vec::new();
    for m in chat.0.iter().skip(1).rev() {
        let is_user = m.get("is_user").and_then(|v| v.as_bool()).unwrap_or(false);
        if is_user {
            break;
        }
        if let Some(name) = m.get("name").and_then(|n| n.as_str()) {
            if !spoken.contains(&name.to_string()) {
                spoken.push(name.to_string());
            }
        }
    }
    members
        .iter()
        .filter(|(gm, _)| !spoken.contains(&gm.name))
        .map(|(gm, _)| gm.avatar.clone())
        .collect()
}

fn make_provider(oai: &nast_model::preset::OaiSettings) -> nast_providers::Provider {
    let kind = match oai.chat_completion_source.as_str() {
        "claude" => nast_providers::ProviderKind::Anthropic {
            api_key: std::env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
        },
        "makersuite" => nast_providers::ProviderKind::Gemini {
            api_key: std::env::var("GOOGLE_API_KEY").unwrap_or_default(),
        },
        _ => nast_providers::ProviderKind::OpenAiCompat {
            base_url: std::env::var("NAST_OPENAI_BASE")
                .unwrap_or_else(|_| "https://api.openai.com/v1".into()),
            api_key: std::env::var("OPENAI_API_KEY").unwrap_or_default(),
        },
    };
    nast_providers::Provider::new(kind)
}

fn param_str<'a>(params: &'a Value, key: &str) -> Result<&'a str, RpcError> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| RpcError::BadRequest(format!("missing param {key}")))
}

// MessageExtra 在此模块仅用于类型引入的兼容引用
#[allow(dead_code)]
type _Extra = MessageExtra;
