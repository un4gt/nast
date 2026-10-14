//! Group chat management with explicit ownership checks and recoverable removal.
use crate::{library::leaf, rpc::{RpcError, RpcResult}, state::SharedState};
use nast_model::chat::{ChatFile, ChatHeader, ChatMetadata};
use serde_json::{json, Value};
use base64::Engine as _;

fn bad(message: impl ToString) -> RpcError { RpcError::BadRequest(message.to_string()) }

pub async fn from_private(state: SharedState, params: Value) -> RpcResult {
    let guard = state.generation.write().await;
    if guard.abort.is_some() { return Err(bad("stop generation before converting chat")); }
    let avatar = leaf(params["avatar"].as_str().ok_or_else(||bad("missing avatar"))?)?;
    let file = leaf(params["chat_file"].as_str().ok_or_else(||bad("missing chat_file"))?)?;
    let character = crate::rpc::read_character(&state, avatar)?;
    let mut chat = state.user.read_chat(avatar, file)?;
    let chat_id = uuid::Uuid::new_v4().to_string();
    let mut members = vec![avatar.to_string()];
    for member in params["members"].as_array().into_iter().flatten() {
        let member = leaf(member.as_str().ok_or_else(||bad("invalid member"))?)?;
        crate::rpc::read_character(&state, member)?;
        if !members.iter().any(|a|a == member) { members.push(member.into()); }
    }
    for message in chat.0.iter_mut().skip(1) {
        if message["is_user"] != true && message["is_system"] != true {
            message["original_avatar"] = json!(avatar);
            message["force_avatar"] = json!(format!("/thumbnail?file={avatar}"));
        }
    }
    chat.0[0]["chat_metadata"]["integrity"] = json!(uuid::Uuid::new_v4().to_string());
    let group = nast_model::group::Group { name:params["name"].as_str().unwrap_or(&character.name).into(),
        members, chat_id:chat_id.clone(), chats:vec![chat_id.clone()], ..Default::default() };
    state.user.save_group_chat(&chat_id, &chat, false)?;
    state.user.save_group(&group)?;
    Ok(json!(group))
}

pub fn owned(state: &SharedState, id: Option<&str>, chat: &str) -> Result<nast_model::group::Group, RpcError> {
    leaf(chat)?;
    if let Some(id) = id { leaf(id)?; }
    let groups = state.user.list_groups()?;
    let mut matches = groups.into_iter().filter(|g| g.chats.iter().any(|c| c == chat) || g.chat_id == chat);
    let group = matches.next().ok_or_else(|| bad("chat has no owning group"))?;
    if matches.next().is_some() || id.is_some_and(|id| id != group.id) {
        return Err(bad("chat does not belong to this group"));
    }
    Ok(group)
}

pub async fn manage(state: SharedState, method: &str, params: Value) -> RpcResult {
    let id = leaf(params["id"].as_str().ok_or_else(|| bad("missing group id"))?)?;
    // Read the group under the mutation lock to avoid overwriting concurrent session changes.
    let generation = state.generation.write().await;
    if generation.abort.is_some() && method != "groups.export_chat" {
        return Err(bad("stop generation before editing a group chat"));
    }
    let mut group = state.user.list_groups()?.into_iter().find(|group| group.id == id)
        .ok_or_else(|| RpcError::NotFound("group not found".into()))?;
    let current = params["chat_id"].as_str().unwrap_or(&group.chat_id).to_string();
    leaf(&current)?;
    if !["groups.new_chat", "groups.import_chat"].contains(&method) && !group.chats.contains(&current) {
        return Err(bad("chat does not belong to this group"));
    }
    match method {
        "groups.history" => {
            let summaries: Vec<_> = group.chats.iter().map(|id| {
                let chat = state.user.read_group_chat(id).ok();
                json!({"chat_id":id,"messages":chat.as_ref().map(|c|c.0.len().saturating_sub(1)).unwrap_or(0),
                    "last_message":chat.as_ref().and_then(|c|c.0.last()).and_then(|m|m["mes"].as_str()).unwrap_or(""),
                    "metadata":chat.as_ref().and_then(|c|c.0.first()).map(|h|h["chat_metadata"].clone())})
            }).collect();
            return Ok(json!(summaries));
        }
        "groups.set_metadata" | "groups.set_persona" | "groups.set_note" => {
            let mut chat = state.user.read_group_chat(&current)?;
            let metadata = chat.0.first_mut().and_then(|h|h["chat_metadata"].as_object_mut()).ok_or_else(||bad("missing metadata"))?;
            if method == "groups.set_persona" {
                let persona = params["persona"].as_str().filter(|s|!s.is_empty());
                if let Some(persona) = persona {
                    if !state.settings.read().await.pointer("/power_user/personas").is_some_and(|p|p.get(persona).is_some()) { return Err(bad("persona not found")); }
                    metadata.insert("persona".into(), json!(persona));
                } else { metadata.remove("persona"); }
            } else if method == "groups.set_note" {
                for key in ["prompt", "interval", "depth", "position", "role"] {
                    if let Some(value) = params["note"].get(key) { metadata.insert(format!("note_{key}"), value.clone()); }
                }
            } else {
                let patch = params["metadata"].as_object().ok_or_else(||bad("missing metadata patch"))?;
                for (key, value) in patch {
                    if ["integrity", "nast_model", "main_chat"].contains(&key.as_str()) { return Err(bad("protected metadata field")); }
                    if value.is_null() { metadata.remove(key); } else { metadata.insert(key.clone(), value.clone()); }
                }
            }
            state.user.save_group_chat(&current, &chat, false)?;
            return Ok(json!({"ok":true,"metadata":chat.0[0]["chat_metadata"]}));
        }
        "groups.add_message" | "groups.hide_message" | "groups.delete_swipe" => {
            let mut chat = state.user.read_group_chat(&current)?;
            if method == "groups.add_message" {
                let text = params["text"].as_str().ok_or_else(||bad("missing text"))?;
                let avatar = params["member"].as_str();
                let message = if let Some(avatar) = avatar {
                    if !group.members.iter().any(|a|a == avatar) { return Err(bad("not a group member")); }
                    let character = crate::rpc::read_character(&state, avatar)?;
                    json!({"name":character.name,"original_avatar":avatar,"force_avatar":format!("/thumbnail?file={avatar}"),"is_user":false,"is_system":false,"mes":text,"send_date":nast_storage::message_time_stamp(),"swipes":[text],"swipe_id":0,"swipe_info":[{"extra":{}}],"extra":{}})
                } else { json!({"name":"System","is_user":false,"is_system":true,"mes":text,"send_date":nast_storage::message_time_stamp(),"extra":{"type":"narrator"}}) };
                chat.0.push(message);
            } else {
                let index = params["index"].as_u64().map(|i|i as usize).unwrap_or(chat.0.len().saturating_sub(2));
                let message = chat.0.get_mut(index + 1).ok_or_else(||bad("index out of range"))?;
                if method == "groups.hide_message" {
                    message["is_system"] = json!(params["hidden"].as_bool().unwrap_or(true));
                } else {
                    let current = message["swipe_id"].as_u64().unwrap_or(0) as usize;
                    let swipes = message["swipes"].as_array_mut().ok_or_else(||bad("no candidates"))?;
                    if swipes.len() <= 1 || current >= swipes.len() { return Err(bad("cannot delete the only candidate")); }
                    swipes.remove(current);
                    let selected = current.min(swipes.len()-1);
                    message["mes"] = swipes[selected].clone();
                    message["swipe_id"] = json!(selected);
                    if let Some(infos) = message["swipe_info"].as_array_mut() { if current < infos.len() { infos.remove(current); } }
                    let info = message["swipe_info"].get(selected).cloned().unwrap_or(json!({}));
                    message["extra"] = info.get("extra").cloned().unwrap_or(json!({}));
                    for key in ["send_date","gen_started","gen_finished"] {
                        if let Some(value) = info.get(key) { message[key] = value.clone(); }
                        else if key != "send_date" { message.as_object_mut().unwrap().remove(key); }
                    }
                }
            }
            state.user.save_group_chat(&current, &chat, false)?;
            return Ok(json!({"ok":true}));
        }
        "groups.branch" => {
            let mut chat = state.user.read_group_chat(&current)?;
            let index = params["index"].as_u64().map(|i|i as usize).unwrap_or(chat.0.len().saturating_sub(2));
            if index + 1 >= chat.0.len() { return Err(bad("index out of range")); }
            chat.0.truncate(index + 2);
            let name = format!("{}-{}", nast_storage::humanized_date_time(), uuid::Uuid::new_v4());
            chat.0[0]["chat_metadata"]["main_chat"] = json!(current);
            chat.0[0]["chat_metadata"]["integrity"] = json!(uuid::Uuid::new_v4().to_string());
            state.user.save_group_chat(&name, &chat, false)?;
            if params["bookmark"] == true {
                let mut parent = state.user.read_group_chat(&current)?;
                parent.0[index+1]["extra"]["bookmark_link"] = json!(name);
                state.user.save_group_chat(&current, &parent, false)?;
            }
            group.chats.push(name.clone()); group.chat_id = name;
        }
        "groups.parent" => {
            let chat = state.user.read_group_chat(&current)?;
            let parent = chat.0[0]["chat_metadata"]["main_chat"].as_str().ok_or_else(||bad("not a branch"))?;
            if !group.chats.iter().any(|c|c == parent) { return Err(bad("parent chat is unavailable")); }
            state.user.read_group_chat(parent)?; group.chat_id = parent.to_string();
        }
        "groups.swipe" => {
            let mut chat = state.user.read_group_chat(&current)?;
            let result = crate::rpc::select_swipe(&mut chat, params["direction"].as_str().unwrap_or("right"))?;
            state.user.save_group_chat(&current, &chat, false)?;
            return Ok(result);
        }
        "groups.new_chat" | "groups.import_chat" => {
            let name = format!("{}-{}", nast_storage::humanized_date_time(), &uuid::Uuid::new_v4().to_string()[..8]);
            if method == "groups.import_chat" {
                let text = params["text"].as_str().ok_or_else(|| bad("missing JSONL text"))?;
                let mut values: Vec<Value> = text.lines().filter(|line| !line.trim().is_empty())
                    .map(serde_json::from_str).collect::<Result<_, _>>().map_err(bad)?;
                if values.is_empty() { return Err(bad("empty chat")); }
                if !values[0].get("chat_metadata").is_some() {
                    values.insert(0, json!(ChatHeader { user_name:"unused".into(), character_name:"unused".into(), chat_metadata:ChatMetadata::default() }));
                }
                if !values[0]["chat_metadata"].is_object() { return Err(bad("invalid chat metadata")); }
                for message in values.iter().skip(1) {
                    if !message["mes"].is_string() || !message["name"].is_string() { return Err(bad("invalid chat message")); }
                }
                values[0]["chat_metadata"]["integrity"] = json!(uuid::Uuid::new_v4().to_string());
                let mut imported = ChatFile(values);
                crate::model_catalog::bind(&mut imported,&state.catalog.lock().unwrap().default_model,true);
                state.user.save_group_chat(&name, &imported, false)?;
            } else { crate::group_gen::init_group_chat(&state, &group, &name)?; }
            group.chats.push(name.clone()); group.chat_id = name;
        }
        "groups.open_chat" => { state.user.read_group_chat(&current)?; group.chat_id = current; }
        "groups.rename_chat" => {
            let name = leaf(params["name"].as_str().ok_or_else(|| bad("missing name"))?)?;
            if name != current {
                if state.user.group_chat_path(name).exists() { return Err(bad("chat name already exists")); }
                let original_group = group.clone();
                let snapshots: Vec<_> = group.chats.iter().map(|id|state.user.read_group_chat(id).map(|chat|(id.clone(),chat))).collect::<Result<_,_>>()?;
                let backup = state.user.root.join("backups").join(format!("group-rename-{}.json",uuid::Uuid::new_v4()));
                nast_storage::atomic_write(&backup, &serde_json::to_vec(&json!({"group":original_group,"chats":snapshots.iter().map(|(id,chat)|json!({"id":id,"rows":chat.0})).collect::<Vec<_>>()})).map_err(bad)?)?;
                for existing in &mut group.chats { if *existing == current { *existing = name.into(); } }
                if group.chat_id == current { group.chat_id = name.into(); }
                let rename = (|| -> Result<(), RpcError> {
                    for (id, original) in &snapshots {
                        let mut chat = original.clone();
                        if chat.0[0]["chat_metadata"]["main_chat"] == current { chat.0[0]["chat_metadata"]["main_chat"] = json!(name); }
                        for message in chat.0.iter_mut().skip(1) {
                            if message["extra"]["bookmark_link"] == current { message["extra"]["bookmark_link"] = json!(name); }
                        }
                        if id == &current || chat.0 != original.0 { state.user.save_group_chat(if id == &current {name} else {id}, &chat, false)?; }
                    }
                    state.user.save_group(&group)?;
                    archive(&state, &current)?;
                    Ok(())
                })();
                if let Err(error) = rename {
                    for (id,chat) in &snapshots { if let Err(restore) = state.user.save_group_chat(id,chat,false) { tracing::error!("rename rollback {id}: {restore}; backup {}",backup.display()); } }
                    if let Err(restore) = state.user.save_group(&original_group) { tracing::error!("rename group rollback: {restore}; backup {}",backup.display()); }
                    return Err(error);
                }
            }
        }
        "groups.delete_chat" => {
            group.chats.retain(|name| name != &current);
            if group.chats.is_empty() {
                let name = uuid::Uuid::new_v4().to_string();
                crate::group_gen::init_group_chat(&state, &group, &name)?;
                group.chats.push(name);
            }
            if group.chat_id == current { group.chat_id = group.chats[0].clone(); }
            state.user.save_group(&group)?;
            archive(&state, &current)?;
        }
        "groups.export_chat" => {
            let chat = state.user.read_group_chat(&current)?;
            let text = chat.0.iter().map(Value::to_string).collect::<Vec<_>>().join("\n");
            return Ok(json!({"filename":format!("{current}.jsonl"),"mime":"application/x-ndjson",
                "data_base64":base64::engine::general_purpose::STANDARD.encode(text)}));
        }
        "groups.set_world" => {
            let mut chat = state.user.read_group_chat(&current)?;
            let world = params["world"].as_str().filter(|name| !name.is_empty());
            if let Some(name) = world { leaf(name)?; state.user.read_world(name)?; }
            let metadata = chat.0.first_mut().and_then(|header| header.get_mut("chat_metadata"))
                .and_then(Value::as_object_mut).ok_or_else(|| bad("missing chat metadata"))?;
            metadata.remove("world"); metadata.remove("world_info");
            if let Some(world) = world { metadata.insert("world_info".into(), json!(world)); }
            state.user.save_group_chat(&current, &chat, false)?;
        }
        _ => return Err(bad("unknown group chat operation")),
    }
    state.user.save_group(&group)?;
    Ok(json!(group))
}

fn archive(state: &SharedState, name: &str) -> Result<(), RpcError> {
    let source = state.user.group_chat_path(name);
    if source.exists() {
        let backup = state.user.root.join("backups").join(format!("group-{}-{name}.jsonl", uuid::Uuid::new_v4()));
        let bytes = std::fs::read(&source).map_err(bad)?;
        nast_storage::atomic_write(&backup, &bytes)?;
        std::fs::remove_file(source).map_err(bad)?;
    }
    Ok(())
}
