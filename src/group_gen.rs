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

pub async fn auto(state: SharedState, method: &str, params: Value) -> RpcResult {
    if method == "groups.auto_status" {
        return Ok(state.auto_group.read().await.as_ref().map(|(_, data, _)| data.clone()).unwrap_or(Value::Null));
    }
    if method == "groups.auto_stop" {
        if let Some((_, _, token)) = state.auto_group.write().await.take() { token.cancel(); }
        return Ok(json!({"ok":true}));
    }
    let id = param_str(&params, "id")?;
    let chat = param_str(&params, "chat_id")?;
    let mut group = crate::group_sessions::owned(&state, Some(id), chat)?;
    group.chat_id = chat.to_string();
    let token = tokio_util::sync::CancellationToken::new();
    let instance = uuid::Uuid::new_v4().to_string();
    let data = json!({"id":id,"chat_id":chat,"delay":group.auto_mode_delay.max(1)});
    {
        let mut guard = state.auto_group.write().await;
        if let Some((_, _, old)) = guard.take() { old.cancel(); }
        *guard = Some((instance.clone(), data.clone(), token.clone()));
    }
    let worker = state.clone();
    actix_web::rt::spawn(async move {
        let max_turns = params["max_turns"].as_u64().unwrap_or(u64::MAX);
        for _ in 0..max_turns {
            tokio::select! {
                _ = token.cancelled() => break,
                _ = tokio::time::sleep(std::time::Duration::from_secs(group.auto_mode_delay.max(1) as u64)) => {}
            }
            if worker.generation.read().await.abort.is_some() { continue; }
            let result = generate_group(worker.clone(), json!({"id":group.id,"chat_id":chat_id(&group),"trigger":"automatic"})).await;
            if result.is_err() { break; }
        }
        let mut guard = worker.auto_group.write().await;
        if guard.as_ref().is_some_and(|(id,_,_)| id == &instance) { *guard = None; }
        worker.hub.emit("group_auto_changed", guard.as_ref().map(|(_,data,_)|data.clone()).unwrap_or(Value::Null));
    });
    state.hub.emit("group_auto_changed", data.clone());
    Ok(data)
}

fn chat_id(group: &Group) -> &str { &group.chat_id }

pub async fn generate_group(state: SharedState, mut params: Value) -> RpcResult {
    params["task_id"] = json!(crate::routing::task_id(&params));
    if let Some(result) = crate::commands::prepare(state.clone(), &mut params, true).await? { return Ok(result); }
    let owner = crate::group_sessions::owned(&state, Some(param_str(&params, "id")?), param_str(&params, "chat_id")?)?;
    if let Some(member) = params["member"].as_str().filter(|m| !m.is_empty()) {
        if !owner.members.iter().any(|a| a == member) { return Err(RpcError::BadRequest("not a group member".into())); }
        read_character(&state, member)?;
    }
    if !["normal", "regenerate", "swipe", "continue", "quiet", "impersonate"].contains(&params["type"].as_str().unwrap_or("normal")) {
        return Err(RpcError::BadRequest("unsupported group generation type".into()));
    }
    params["batch_id"] = json!(chrono::Utc::now().timestamp_millis());
    let task_id = crate::routing::task_id(&params);
    params["task_id"] = json!(task_id);
    let abort = tokio_util::sync::CancellationToken::new();
    {
        let mut guard = state.generation.write().await;
        if guard.abort.is_some() {
            return Err(RpcError::BadRequest("generation already in progress".into()));
        }
        guard.info = Some(crate::state::GenerationInfo { kind:"group".into(),avatar:params["id"].as_str().unwrap_or_default().into(),chat_file:params["chat_id"].as_str().unwrap_or_default().into(),is_group:true,task_id:task_id.clone() });
        guard.abort = Some(abort.clone());
        guard.text = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    }
    let mut context = json!({"type":"group","is_group":true,"group_id":params["id"],"chat_id":params["chat_id"],"task_id":task_id,
        "batch_id":params["batch_id"],"generation_type":params["type"].as_str().unwrap_or("normal"),
        "conversation":{"kind":"group","group_id":params["id"],"chat_id":params["chat_id"]},
        "group":owner,
        "trigger":params["trigger"].as_str().unwrap_or(if params["member"].is_string() { "member" } else if params["user_message"].as_str().is_some_and(|m| !m.is_empty()) { "user" } else { "automatic" })});
    for event in ["generation_started", "group_batch_started"] {
        state.hub.emit(event, context.clone());
        crate::plugins::dispatch(&state.plugins, event, context.clone()).await;
    }
    let rollback = if params["type"] == "regenerate" {
        state.user.read_group_chat(params["chat_id"].as_str().unwrap_or("")).ok()
    } else { None };
    let result = generate_group_inner(state.clone(), params, abort.clone(), context.clone()).await;
    if result.is_err() {
        if let Some(previous) = rollback {
            let chat_id = context["chat_id"].as_str().unwrap();
            let has_new_reply = state.user.read_group_chat(chat_id).ok().is_some_and(|c| c.0.iter().skip(1).any(|m| m["extra"]["gen_id"] == context["batch_id"]));
            if !has_new_reply { if let Err(error) = state.user.save_group_chat(chat_id, &previous, false) { tracing::error!("group regeneration rollback failed: {error}"); } }
        }
    }
    context["success"] = json!(result.is_ok());
    context["cancelled"] = json!(abort.is_cancelled());
    if let Err(error) = &result { context["error"] = json!(error.to_string()); }
    for event in ["group_batch_ended", "generation_ended"] {
        crate::plugins::dispatch(&state.plugins, event, context.clone()).await;
    }
    {
        let mut guard = state.generation.write().await;
        guard.abort = None;
        guard.info = None;
        guard.group_progress = Value::Null;
        guard.text = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    }
    for event in ["group_batch_ended", "generation_ended"] { state.hub.emit(event, context.clone()); }
    result
}

async fn generate_group_inner(state: SharedState, params: Value, abort: tokio_util::sync::CancellationToken, mut context: Value) -> RpcResult {
    let group_id = param_str(&params, "id")?;
    let chat_id = param_str(&params, "chat_id")?;
    // 触发单成员（force_chid 语义）：仅该成员回复
    let mut force_member = params
        .get("member")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let group = crate::group_sessions::owned(&state, Some(group_id), chat_id)?;
    context["group"] = json!(group);
    if !force_member.is_empty() && !group.members.contains(&force_member) {
        return Err(RpcError::BadRequest("forced character is not a group member".into()));
    }

    // 读群聊天文件（无则用成员开场白初始化）
    match state.user.read_group_chat(chat_id) {
        Ok(_) => {},
        Err(nast_storage::StorageError::NotFound(_)) => { init_group_chat(&state, &group, chat_id)?; },
        Err(error) => return Err(error.into()),
    };

    let task_id = crate::routing::task_id(&params);
    let routing = crate::routing::Routing::prepare(&state, crate::model_catalog::Conversation::Group { group_id:group_id.into(), chat_id:chat_id.into() }, &params, task_id.clone()).await?;
    // prepare persisted the initial model binding; use that same header below.
    let mut chat = state.user.read_group_chat(chat_id)?;
    let generation_type = match params["type"].as_str().unwrap_or("normal") {
        "normal" => GenerationType::Normal,
        "regenerate" => GenerationType::Regenerate,
        "swipe" => GenerationType::Swipe,
        "continue" => GenerationType::Continue,
        "quiet" => GenerationType::Quiet,
        "impersonate" => GenerationType::Impersonate,
        _ => return Err(RpcError::BadRequest("unsupported group generation type".into())),
    };
    if matches!(generation_type, GenerationType::Swipe | GenerationType::Continue) {
        let last = chat.0.last().filter(|m| m["is_user"] == false && m["is_system"] != true)
            .ok_or_else(|| RpcError::BadRequest("last message is not a character reply".into()))?;
        force_member = last["original_avatar"].as_str().filter(|a| group.members.iter().any(|m| m == a))
            .ok_or_else(|| RpcError::BadRequest("reply character is no longer a group member".into()))?.to_string();
    }
    if generation_type == GenerationType::Regenerate {
        let backup = state.user.root.join("backups").join(format!("group-regenerate-{}-{chat_id}.jsonl", uuid::Uuid::new_v4()));
        let bytes = chat.0.iter().map(Value::to_string).collect::<Vec<_>>().join("\n");
        nast_storage::atomic_write(&backup, bytes.as_bytes())?;
        let last = chat.0.last().filter(|m| m["is_user"] == false)
            .ok_or_else(|| RpcError::BadRequest("nothing to regenerate".into()))?;
        let generation_id = last["extra"]["gen_id"].clone();
        loop {
            if chat.0.len() <= 1 { break; }
            let last = chat.0.last().unwrap();
            if last["is_user"] != false || last["is_system"] == true { break; }
            if !generation_id.is_null() && !last["extra"]["gen_id"].is_null()
                && last["extra"]["gen_id"] != generation_id { break; }
            chat.0.pop();
            state.hub.emit("message_deleted", json!(chat.0.len()));
        }
        state.user.save_group_chat(chat_id, &chat, false)?;
    }

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

    if !force_member.is_empty() && !members.iter().any(|(m,_)| m.avatar == force_member) {
        return Err(RpcError::BadRequest("forced member card is missing".into()));
    }
    if force_member.is_empty() && generation_type == GenerationType::Quiet {
        force_member = chat.0.iter().skip(1).rev().find(|m| m["is_user"] != true && m["extra"]["type"] != "narrator")
            .and_then(|m| m["original_avatar"].as_str()).filter(|a| members.iter().any(|(m,_)| &m.avatar == a))
            .map(str::to_string).or_else(||members.first().map(|(m,_)|m.avatar.clone())).unwrap_or_default();
    }
    if force_member.is_empty() && generation_type == GenerationType::Impersonate && !members.is_empty() {
        use rand::Rng;
        force_member = members[rand::thread_rng().gen_range(0..members.len())].0.avatar.clone();
    }

    // persona 名（用户消息署名）
    let user_name = {
        let metadata: ChatMetadata = chat.metadata();
        // 复用单人会话的 persona 解析：构造临时 session 不现实，直接读 settings
        let settings_snapshot = state.settings.read().await.clone();
        let pid = metadata
            .persona
            .clone()
            .or_else(|| {
                settings_snapshot
                    .get("power_user")
                    .and_then(|p| p.get("default_persona"))
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
            .unwrap_or_default();
        let mut name = settings_snapshot
            .get("power_user")
            .and_then(|p| p.get("username"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if !pid.is_empty() {
            if let Some(n) = settings_snapshot
                .get("power_user")
                .and_then(|p| p.get("personas"))
                .and_then(|v| v.as_object())
                .and_then(|m| m.get(&pid))
                .and_then(|v| v.as_str())
            {
                name = n.to_string();
            }
        }
        if name.is_empty() {
            name = "User".into();
        }
        name
    };

    // 用户消息（可选）先落盘；插件 user_input 钩子可改写
    let user_message = params
        .get("user_message")
        .and_then(|v| v.as_str())
        .filter(|_| generation_type == GenerationType::Normal)
        .unwrap_or("");
    let user_message = if user_message.is_empty() {
        String::new()
    } else {
        context["text"] = json!(user_message);
        let transformed = crate::plugins::dispatch(&state.plugins, "user_input", context.clone()).await
            .unwrap_or_else(||user_message.to_string());
        let settings = state.settings.read().await.clone();
        // No character is selected while submitting the group user message.
        // Apply shared global/preset rules once, before the member generation loop.
        let scripts = crate::generate::collect_regex_scripts_for(&settings, &Default::default(), &chat.metadata());
        let transformed = nast_engine::regex_engine::get_regexed_string(&transformed,
            nast_model::regex_script::RP_USER_INPUT, &scripts, &Default::default(),
            &|text| crate::prompt_bridge::substitute_basic(text, &user_name, &group.name));
        let msg = json!({
            "name": user_name,
            "is_user": true,
            "is_system": false,
            "send_date": nast_storage::message_time_stamp(),
            "mes": transformed,
        });
        chat.0.push(msg);
        state.user.save_group_chat(chat_id, &chat, false)?;
        let index = chat.0.len() - 1;
        state.hub.emit("message_sent", json!(index));
        state.hub.emit("user_message_rendered", json!(index));
        context["index"] = json!(index);
        context["message"] = chat.0.last().cloned().unwrap_or(Value::Null);
        crate::plugins::dispatch(&state.plugins, "message_saved", context.clone()).await;
        transformed
    };

    // 最后发言者 & 自上次用户消息后的未发言成员
    let last = chat.0.last().filter(|_|chat.0.len() > 1);
    let last_speaker = last.filter(|m| m["is_user"] != true)
        .and_then(|m| m[if group.activation_strategy == 3 { "original_avatar" } else { "name" }].as_str()).map(str::to_string);
    let activation_text = if user_message.is_empty() {
        last.filter(|m| m["is_system"] != true).and_then(|m|m["mes"].as_str()).unwrap_or("")
    } else { &user_message };
    let unspoken = unspoken_since_user(&chat, &members);

    // 激活成员（触发单成员时跳过策略）
    let mut activated: Vec<String> = if !force_member.is_empty() {
        vec![force_member.clone()]
    } else {
        let mut rng = rand::rngs::StdRng::from_seed(rand::random());
        let member_only: Vec<GroupMember> = members.iter().map(|(gm, _)| gm.clone()).collect();
        nast_engine::group_chat::activate_members(
            &group,
            &member_only,
            activation_text,
            last_speaker.as_deref(),
            &unspoken,
            !user_message.is_empty(),
            &mut rng,
        )
    };
    context["eligible_members"] = json!(members.iter().filter(|(m,_)| !group.disabled_members.contains(&m.avatar)).map(|(m,_)| &m.avatar).collect::<Vec<_>>());
    context["queue"] = json!(activated);
    if force_member.is_empty() && matches!(generation_type, GenerationType::Normal | GenerationType::Regenerate) {
        if let Some(selection) = crate::plugins::dispatch_json(&state.plugins, "group_queue", context.clone()).await {
            activated = serde_json::from_value(selection["queue"].clone()).unwrap_or(activated);
        }
    }
    context["queue"] = json!(activated);
    context["stage"] = json!("queued");
    state.generation.write().await.group_progress = context.clone();
    state.hub.emit("group_progress", context.clone());
    if activated.is_empty() {
        return Ok(json!({"activated": [], "replies":[], "chat_id": chat_id,"task_id":task_id}));
    }

    // 生成会话
    {
        let mut guard = state.generation.write().await;
        guard.abort = Some(abort.clone());
        guard.text = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        guard.info = Some(crate::state::GenerationInfo {
            kind: "group".into(),
            avatar: group.id.clone(),
            chat_file: chat_id.to_string(),
            is_group: true,
            task_id: task_id.clone(),
        });
        guard.phase = routing.phase.clone();
        guard.reasoning = routing.reasoning.clone();
    }
    let progress = state.generation.read().await.text.clone();
    let mut oai: nast_model::preset::OaiSettings = serde_json::from_value(
        state
            .settings
            .read()
            .await
            .get("oai_settings")
            .cloned()
            .unwrap_or(json!({})),
    )
    .unwrap_or_default();
    routing.configure_prompt(&mut oai);
    let settings_snapshot = state.settings.read().await.clone();
    let mut session = GenerateSession {
        shared_settings: &state.settings,
        global_variables: std::cell::RefCell::new(settings_snapshot.pointer("/extension_settings/variables/global").and_then(Value::as_object).cloned().unwrap_or_default()),
        user: &state.user,
        hub: &state.hub,
        oai,
        settings_json: &settings_snapshot,
        routing,
        abort,
        plugins: &state.plugins,
        progress,
    };

    let gen_id = params["batch_id"].as_i64().unwrap();
    let mut replies: Vec<Value> = Vec::new();
    for (position, avatar) in activated.iter().enumerate() {
        if session.abort.is_cancelled() { break; }
        let (_member, member_ch) = members
            .iter()
            .find(|(gm, _)| &gm.avatar == avatar)
            .ok_or_else(|| RpcError::Internal("member vanished".into()))?;
        state
            .hub
            .emit("group_member_drafted", json!(member_ch.name));

        context["queue"] = json!(&activated[position..]);
        context["member"] = json!(avatar);
        context["stage"] = json!("member_started");
        {
            let mut progress = state.generation.write().await;
            progress.group_progress = context.clone();
            progress.text.lock().unwrap().clear();
            progress.reasoning.lock().unwrap().clear();
        }
        state.hub.emit("group_progress", context.clone());
        crate::plugins::dispatch(&state.plugins, "group_member_started", context.clone()).await;
        if session.abort.is_cancelled() { break; }
        let p = GenerateParams {
            group: Some(crate::generate::GroupContext { group: group.clone(), members: members.clone(), generation_id:gen_id, trigger:context["trigger"].as_str().unwrap_or("automatic").into() }),
            generation_type: if generation_type == GenerationType::Regenerate { GenerationType::Normal } else { generation_type },
            avatar: avatar.clone(),
            chat_file: chat_id.to_string(),
            user_message: if generation_type == GenerationType::Quiet { params["user_message"].as_str().unwrap_or("").into() } else { String::new() },
            character: member_ch.clone(),
            is_group: true,
        };
        session.oai = serde_json::from_value(settings_snapshot["oai_settings"].clone()).unwrap_or_default();
        session.routing.configure_prompt(&mut session.oai);
        let result = session.run_content(&p).await;
        context["success"] = json!(result.is_ok());
        context["stage"] = json!("member_ended");
        context["queue"] = json!(&activated[position+1..]);
        state.generation.write().await.group_progress = context.clone();
        state.hub.emit("group_progress", context.clone());
        crate::plugins::dispatch(&state.plugins, "group_member_ended", context.clone()).await;
        let result = result.map_err(RpcError::generation)?;
        replies.push(json!({"avatar":avatar,"name":member_ch.name,"text":result.text,"reasoning":result.reasoning,"routing":result.routing}));
        if result.routing["status"] == "incomplete" { break; }
        if session.abort.is_cancelled() { break; }
    }
    Ok(json!({"activated": activated, "replies": replies, "chat_id": chat_id, "task_id":task_id}))
}

pub(crate) fn init_group_chat(
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
            let greeting_id = uuid::Uuid::new_v4().to_string();
            let msg = json!({
                "name": ch.name,
                "is_user": false,
                "is_system": false,
                "send_date": nast_storage::message_time_stamp(),
                "mes": greeting,
                "swipe_id": 0,
                "swipes": [greeting],
                "swipe_info": [{"extra": {"gen_id": greeting_id}}],
                "force_avatar": format!("thumbnail?type=avatar&file={avatar}"),
                "original_avatar": avatar,
                "extra": {"gen_id": greeting_id},
            });
            chat.0.push(msg);
        }
    }
    crate::model_catalog::bind(&mut chat, &state.catalog.lock().unwrap().default_model, false);
    state.user.save_group_chat(chat_id, &chat, false)?;
    Ok(chat)
}

fn unspoken_since_user(chat: &ChatFile, members: &[(GroupMember, Character)]) -> Vec<String> {
    let mut spoken: Vec<String> = Vec::new();
    for m in chat.0.iter().skip(1).rev() {
        let is_user = m.get("is_user").and_then(|v| v.as_bool()).unwrap_or(false);
        if is_user {
            break;
        }
        if m["is_system"] == true || m["extra"]["type"] == "narrator" { continue; }
        if let Some(name) = m.get("original_avatar").and_then(|n| n.as_str()) {
            if !spoken.contains(&name.to_string()) {
                spoken.push(name.to_string());
            }
        }
    }
    members
        .iter()
        .filter(|(gm, _)| !spoken.contains(&gm.avatar))
        .map(|(gm, _)| gm.avatar.clone())
        .collect()
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
