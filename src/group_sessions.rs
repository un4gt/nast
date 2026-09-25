//! Group chat management with explicit ownership checks and recoverable removal.
use crate::{library::leaf, rpc::{RpcError, RpcResult}, state::SharedState};
use nast_model::chat::{ChatFile, ChatHeader, ChatMetadata};
use serde_json::{json, Value};
use base64::Engine as _;

fn bad(message: impl ToString) -> RpcError { RpcError::BadRequest(message.to_string()) }

pub async fn manage(state: SharedState, method: &str, params: Value) -> RpcResult {
    let id = leaf(params["id"].as_str().ok_or_else(|| bad("missing group id"))?)?;
    let mut group = state.user.list_groups()?.into_iter().find(|group| group.id == id)
        .ok_or_else(|| RpcError::NotFound("group not found".into()))?;
    // Keep the read guard through mutation, so no generation can acquire ownership midway.
    let generation = state.generation.read().await;
    if generation.abort.is_some() && method != "groups.export_chat" {
        return Err(bad("stop generation before editing a group chat"));
    }
    let current = params["chat_id"].as_str().unwrap_or(&group.chat_id).to_string();
    leaf(&current)?;
    if !["groups.new_chat", "groups.import_chat"].contains(&method) && !group.chats.contains(&current) {
        return Err(bad("chat does not belong to this group"));
    }
    match method {
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
                let chat = state.user.read_group_chat(&current)?;
                state.user.save_group_chat(name, &chat, false)?;
                for existing in &mut group.chats { if *existing == current { *existing = name.into(); } }
                if group.chat_id == current { group.chat_id = name.into(); }
                state.user.save_group(&group)?;
                archive(&state, &current)?;
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
