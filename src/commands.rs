//! Server-owned slash command dispatch. Command discovery is never an execution prerequisite.
use crate::{rpc::{RpcError, RpcResult}, state::SharedState};
use serde_json::{json, Value};

fn bad(text: impl ToString) -> RpcError { RpcError::BadRequest(text.to_string()) }

pub async fn execute(state: SharedState, params: Value) -> RpcResult {
    let c = &params["conversation"];
    let input = params["input"].as_str().ok_or_else(|| bad("missing input"))?;
    if !input.trim_start().starts_with('/') { return Ok(json!({"status":"unhandled","command":false})); }
    let mut p = json!({"type":"normal", "user_message":input});
    if let Some(task) = params.get("task_id") { p["task_id"] = task.clone(); }
    let mut result = match c["kind"].as_str() {
        Some("group") => {
            p["id"] = c["group_id"].clone(); p["chat_id"] = c["chat_id"].clone();
            crate::group_gen::generate_group(state, p).await?
        }
        Some("private") => {
            p["avatar"] = c["avatar"].clone(); p["chat_file"] = c["chat_file"].clone();
            crate::rpc::generate_run(state, p).await?
        }
        _ => return Err(bad("invalid conversation")),
    };
    result["command"] = json!(true);
    if result.get("status").is_none() { result["status"] = json!("handled"); }
    Ok(result)
}

/// A result consumes the input. None means params now describe exactly one generation.
pub async fn prepare(state: SharedState, p: &mut Value, group_chat: bool) -> Result<Option<Value>, RpcError> {
    if p["type"].as_str().unwrap_or("normal") != "normal" { return Ok(None); }
    let input = p["user_message"].as_str().unwrap_or("").trim().to_string();
    let Some(body) = input.strip_prefix('/') else { return Ok(None) };
    p["trigger"] = json!("command");
    let mut parts = body.splitn(2, char::is_whitespace);
    let raw_name = parts.next().unwrap_or("").to_ascii_lowercase();
    let name = match raw_name.as_str() {
        "getmember" | "memberget" => "member-get",
        "disable" | "disablemember" | "memberdisable" => "member-disable",
        "enable" | "enablemember" | "memberenable" => "member-enable",
        "addmember" | "memberadd" => "member-add",
        "removemember" | "memberremove" => "member-remove",
        "upmember" | "memberup" => "member-up",
        "downmember" | "memberdown" => "member-down",
        "peek" | "memberpeek" | "peekmember" => "member-peek",
        "countmember" | "membercount" => "member-count",
        "delswipe" | "swipedel" => "swipe-delete",
        "renamechat" => "name", "imp" => "impersonate",
        _ => &raw_name,
    }.to_string();
    let mut args = parts.next().unwrap_or("").trim();
    let mut field = "name";
    if name == "member-get" {
        if let Some(rest) = args.strip_prefix("field=") {
            let (value, remaining) = rest.split_once(char::is_whitespace).unwrap_or((rest,""));
            field = value.trim_matches('"'); args = remaining.trim();
        }
    }
    if ["trigger","gen","generate","impersonate"].contains(&name.as_str()) {
        if args.starts_with("await=") { args = args.split_once(char::is_whitespace).map(|(_,v)|v.trim()).unwrap_or(""); }
    }
    args = args.strip_prefix('"').and_then(|s|s.strip_suffix('"')).unwrap_or(args);
    let persona;
    if name == "persona" && !args.eq_ignore_ascii_case("none") {
        let settings = state.settings.read().await;
        let personas = settings.pointer("/power_user/personas").and_then(Value::as_object).ok_or_else(||bad("no personas"))?;
        persona = if personas.contains_key(args) { args.to_string() } else {
            let hits: Vec<_> = personas.iter().filter(|(_,v)|v.as_str().is_some_and(|s|s.eq_ignore_ascii_case(args))).collect();
            match hits.as_slice() { [(id,_)] => (*id).clone(), _ => return Err(bad("persona not found or ambiguous")) }
        };
        args = &persona;
    }
    let conversation = if group_chat { json!({"kind":"group","group_id":p["id"],"chat_id":p["chat_id"]}) }
        else { json!({"kind":"private","avatar":p["avatar"],"chat_file":p["chat_file"]}) };
    // Validate ownership before calling any plugin or mutating a conversation.
    let group = if group_chat {
        Some(crate::group_sessions::owned(&state, p["id"].as_str(), p["chat_id"].as_str().ok_or_else(|| bad("missing chat_id"))?)?)
    } else {
        state.user.read_chat(p["avatar"].as_str().ok_or_else(||bad("missing avatar"))?, p["chat_file"].as_str().ok_or_else(||bad("missing chat_file"))?)?;
        None
    };
    if name == "model" { return crate::rpc::model_command(state, json!({"conversation":conversation,"argument":args})).await.map(Some); }
    let mut target = json!({"id":p["id"],"chat_id":p["chat_id"]});
    let handled = |value: Value| Some(json!({"status":"handled","command":true,"saved":false,"result":value}));
    if group.is_none() {
        let avatar = p["avatar"].as_str().unwrap();
        let file = p["chat_file"].as_str().unwrap();
        let mut target = json!({"avatar":avatar,"file_name":file});
        let method = match name.as_str() {
            "trigger" | "gen" | "generate" | "regenerate" | "continue" | "impersonate" | "quiet" => {
                p["type"] = json!(if ["trigger","gen","generate"].contains(&name.as_str()) { "normal" } else { &name });
                p["user_message"] = json!(if name == "quiet" { args } else { "" });
                return Ok(None);
            }
            "newchat" => { target["greeting_index"] = json!(args.parse::<i64>().unwrap_or(-1)); "chats.new" }
            "del" => {
                let chat = state.user.read_chat(avatar, file)?;
                target["index"] = json!(if args.is_empty() { chat.0.len().saturating_sub(2) } else { args.parse::<usize>().map_err(bad)? });
                "chats.delete_message"
            }
            "name" => { target["original_file"] = json!(file); target["renamed_file"] = json!(args); "chats.rename" }
            "world" => { target["world"] = if args == "none" { Value::Null } else { json!(args) }; "chats.set_world" }
            "persona" => { target["persona"] = if args == "none" { Value::Null } else { json!(args) }; "chats.set_persona" }
            "swipe" => {
                let chat = state.user.read_chat(avatar,file)?;
                let last = chat.0.last().ok_or_else(||bad("empty chat"))?;
                if args == "right" && last["swipe_id"].as_u64().unwrap_or(0)+1 >= last["swipes"].as_array().map(|a|a.len() as u64).unwrap_or(1) {
                    p["type"] = json!("swipe"); p["user_message"] = json!(""); return Ok(None);
                }
                target["direction"] = json!(args); "chats.swipe"
            }
            "sys" | "narrator" => {
                let guard = state.generation.write().await;
                if guard.abort.is_some() { return Err(bad("stop generation before editing chat")); }
                let mut chat = state.user.read_chat(avatar,file)?;
                chat.0.push(json!({"name":"System","is_user":false,"is_system":true,"mes":args,"send_date":nast_storage::message_time_stamp(),"extra":{"type":"narrator"}}));
                state.user.save_chat(avatar,file,&chat,false)?;
                return Ok(handled(json!({"ok":true})));
            }
            _ => "",
        };
        if !method.is_empty() { return Box::pin(crate::rpc::dispatch(state,method,target)).await.map(handled); }
    }
    if let Some(mut group) = group {
        let member = |query: &str| -> Result<String, RpcError> {
            if group.members.iter().any(|a| a == query) { return Ok(query.to_string()); }
            if let Ok(index) = query.parse::<usize>() { return group.members.get(index).cloned().ok_or_else(||bad("member index out of range")); }
            let hits: Vec<_> = group.members.iter().filter(|a| crate::rpc::read_character(&state, a).is_ok_and(|c| c.name.eq_ignore_ascii_case(query))).collect();
            match hits.as_slice() { [avatar] => Ok((*avatar).clone()), [] => Err(bad("member not found")), _ => Err(bad("ambiguous member name; use avatar")) }
        };
        match name.as_str() {
            "trigger" | "gen" | "generate" | "member-trigger" => {
                if !args.is_empty() { p["member"] = json!(member(args)?); }
                p["user_message"] = json!(""); return Ok(None);
            }
            "regenerate" | "continue" | "impersonate" | "quiet" => {
                p["type"] = json!(name); p["user_message"] = json!(if name == "quiet" { args } else { "" }); return Ok(None);
            }
            "member-list" => return Ok(Some(json!({"status":"handled","command":true,"saved":false,"result":group.members,"text":group.members.join("\n")}))),
            "member-count" => return Ok(Some(json!({"status":"handled","command":true,"result":group.members.len(),"text":group.members.len().to_string()}))),
            "member-add" | "member-remove" | "member-disable" | "member-enable" | "member-up" | "member-down" | "member-get" | "member-peek" => {
                let avatar = if name == "member-add" {
                    let avatars = state.user.list_characters()?;
                    if avatars.iter().any(|a|a == args) { args.to_string() } else {
                        let hits:Vec<_> = avatars.iter().filter(|a|crate::rpc::read_character(&state,a).is_ok_and(|c|c.name.eq_ignore_ascii_case(args))).collect();
                        match hits.as_slice() { [avatar] => (*avatar).clone(), _ => return Err(bad("character not found or ambiguous; use avatar")) }
                    }
                } else { member(args)? };
                if name == "member-get" || name == "member-peek" {
                    let card = crate::rpc::read_character(&state, &avatar)?;
                    let value = if name == "member-peek" { json!(card) } else { match field {
                        "name" => json!(card.name), "avatar" => json!(avatar),
                        "index" => json!(group.members.iter().position(|a|a == &avatar)),
                        "id" => json!(state.user.list_characters()?.iter().position(|a|a == &avatar)),
                        _ => return Err(bad("field must be name, avatar, index or id")),
                    }};
                    return Ok(Some(json!({"status":"handled","command":true,"saved":false,"text":value.as_str().map(str::to_string).unwrap_or_else(||value.to_string()),"result":value})));
                }
                let guard = state.generation.write().await;
                if guard.abort.is_some() { return Err(bad("stop generation before editing members")); }
                group = crate::group_sessions::owned(&state, p["id"].as_str(), p["chat_id"].as_str().unwrap())?;
                if name != "member-add" && !group.members.contains(&avatar) { return Err(bad("member was removed")); }
                match name.as_str() {
                    "member-add" => { if !group.members.contains(&avatar) { group.members.push(avatar); } }
                    "member-remove" => { group.members.retain(|a| a != &avatar); group.disabled_members.retain(|a| a != &avatar); }
                    "member-disable" => { if !group.disabled_members.contains(&avatar) { group.disabled_members.push(avatar); } }
                    "member-enable" => group.disabled_members.retain(|a| a != &avatar),
                    _ => { let i = group.members.iter().position(|a| a == &avatar).unwrap();
                        let j = if name == "member-up" { i.saturating_sub(1) } else { (i+1).min(group.members.len()-1) }; group.members.swap(i,j); }
                }
                state.user.save_group(&group)?; return Ok(handled(json!(group)));
            }
            "newchat" | "name" | "go" | "world" | "persona" | "sys" | "narrator" | "branch" | "bookmark" | "parent" | "swipe" | "swipe-delete" | "del" => {
                let method = match name.as_str() {
                    "newchat" => "groups.new_chat",
                    "name" => { target["name"] = json!(args); "groups.rename_chat" }
                    "go" => { target["chat_id"] = json!(args); "groups.open_chat" }
                    "world" => { target["world"] = json!(if args == "none" { "" } else { args }); "groups.set_world" }
                    "persona" => { target["persona"] = if args == "none" { Value::Null } else { json!(args) }; "groups.set_persona" }
                    "sys" | "narrator" => { target["text"] = json!(args); "groups.add_message" }
                    "branch" | "bookmark" => { if !args.is_empty() { target["index"] = json!(args.parse::<usize>().map_err(bad)?); } target["bookmark"] = json!(name == "bookmark"); "groups.branch" }
                    "parent" => "groups.parent",
                    "swipe" => { target["direction"] = json!(args); "groups.swipe" }
                    "swipe-delete" => "groups.delete_swipe",
                    _ => { let chat = state.user.read_group_chat(p["chat_id"].as_str().unwrap())?; target["index"] = json!(if args.is_empty() { chat.0.len().saturating_sub(2) } else { args.parse::<usize>().map_err(bad)? }); "groups.delete_message" }
                };
                let result = Box::pin(crate::rpc::dispatch(state, method, target)).await?;
                if name == "swipe" && result["needs_generation"] == true {
                    p["type"] = json!("swipe"); p["user_message"] = json!(""); return Ok(None);
                }
                return Ok(handled(result));
            }
            _ => {}
        }
    }
    // Hold generation ownership stable while a command runs; the worker wait is asynchronous.
    let guard = state.generation.read().await;
    if guard.abort.is_some() { return Err(bad("generation already in progress")); }
    let context = json!({"conversation":conversation,"group_id":p["id"],"chat_id":p["chat_id"],"task_id":p["task_id"],"trigger":"command","generation_type":"normal","is_group":group_chat});
    if let Some(text) = crate::plugins::command(&state.plugins, input, context).await.map_err(bad)? {
        if text.is_empty() { return Ok(handled(Value::Null)); }
        p["user_message"] = json!(text); return Ok(None);
    }
    let settings = state.settings.read().await;
    if let Some(custom) = settings.pointer("/power_user/custom_commands").and_then(Value::as_array).and_then(|list| list.iter().find(|c| c["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(&name)))) {
        p["user_message"] = custom["text"].clone(); return Ok(None);
    }
    Ok(Some(json!({"status":"unknown","command":true,"saved":false,"text":format!("未知命令：/{name}")})))
}
