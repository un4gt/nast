//! Character-library operations shared by RPC and import/export workflows.
use base64::Engine as _;
use serde_json::{json, Value};
use nast_model::{Character, WorldInfoBook};
use crate::{rpc::{RpcError, RpcResult, read_character}, state::SharedState};

fn bad(error: impl ToString) -> RpcError { RpcError::BadRequest(error.to_string()) }
fn internal(error: impl ToString) -> RpcError { RpcError::Internal(error.to_string()) }
pub fn leaf(value: &str) -> Result<&str, RpcError> {
    if value.is_empty() || value == "." || value == ".."
        || value.chars().any(|c| matches!(c, '/' | '\\' | ':' | '\0')) {
        return Err(bad("invalid file name"));
    }
    Ok(value)
}
fn required<'a>(params: &'a Value, key: &str) -> Result<&'a str, RpcError> {
    params[key].as_str().filter(|s| !s.trim().is_empty()).ok_or_else(|| bad(format!("missing {key}")))
}

pub fn sync_book(state: &SharedState, character: &mut Character) -> Result<(), RpcError> {
    if let Some(world) = character.data.extensions.world.as_deref().filter(|v| !v.is_empty()) {
        leaf(world)?;
        match state.user.read_world(world) {
            Ok(book) => character.data.character_book = Some(book.to_embedded(world).map_err(bad)?),
            Err(nast_storage::StorageError::NotFound(_)) => {},
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub fn write_character(state: &SharedState, avatar: &str, character: &Character, image: &[u8]) -> Result<(), RpcError> {
    leaf(avatar)?;
    character.validate().map_err(bad)?;
    let value = serde_json::to_value(character).map_err(internal)?;
    let v3 = (character.spec.as_deref() == Some("chara_card_v3")).then_some(&value);
    let bytes = nast_cards::write_card(image, &value, v3).map_err(bad)?;
    nast_storage::atomic_write(&state.user.character_dir().join(avatar), &bytes)?;
    Ok(())
}

pub fn import_character(state: SharedState, params: Value) -> RpcResult {
    let bytes = base64::engine::general_purpose::STANDARD.decode(required(&params,"data_base64")?).map_err(bad)?;
    let filename = params["filename"].as_str().or(params["file_name"].as_str()).unwrap_or("");
    let mut imported = nast_cards::import::parse(&bytes, filename, params["user_name"].as_str().unwrap_or("User")).map_err(bad)?;
    let replacement = params["avatar"].as_str();
    let avatar = if let Some(avatar) = replacement {
        leaf(avatar)?;
        if params["overwrite"] != true { return Err(bad("replacing a character requires overwrite=true")); }
        read_character(&state, avatar)?;
        avatar.to_string()
    } else {
        let display = imported.character.data.extensions.extra.get("display_name").and_then(Value::as_str)
            .filter(|s| !s.is_empty()).unwrap_or(&imported.character.name);
        state.user.unique_character_file(display)
    };
    imported.character.avatar = Some(avatar.clone());
    let folder = nast_cards::sanitize_filename(&imported.character.name);
    let mut resources = Vec::new();
    let mut background_paths = std::collections::BTreeMap::new();
    let mut asset_map = serde_json::Map::new();
    for asset in &imported.assets {
        if imported.format == "byaf" && replacement.is_some() { continue; }
        let directory = match asset.category {
            "sprite" => format!("characters/{folder}"),
            "background" => format!("characters/{folder}/backgrounds"),
            "byaf_background" => format!("user/images/{avatar}"),
            _ => format!("user/images/{folder}"),
        };
        let mut name = nast_cards::sanitize_filename(&asset.name);
        // BYAF auxiliary icons never replace existing files.
        if imported.format == "byaf" {
            let original = name.clone();
            let mut suffix = 1;
            while state.user.root.join(&directory).join(&name).exists() {
                let path = std::path::Path::new(&original);
                name = format!("{} {suffix}.{}", path.file_stem().unwrap().to_string_lossy(),
                    path.extension().unwrap_or_default().to_string_lossy());
                suffix += 1;
            }
        }
        let relative = format!("{directory}/{name}");
        nast_storage::atomic_write(&state.user.root.join(&relative), &asset.bytes)?;
        if asset.category == "byaf_background" { background_paths.insert(asset.source.clone(), relative.clone()); }
        asset_map.insert(asset.source.clone(), json!(relative));
        resources.push(json!({"path":relative,"category":asset.category}));
    }
    if !asset_map.is_empty() {
        imported.character.data.extensions.extra.insert("nast_assets".into(), Value::Object(asset_map));
    }
    let mut chats = Vec::new();
    if replacement.is_none() {
        for (label, mut chat) in imported.chats {
            let metadata = &mut chat.0[0]["chat_metadata"];
            let background = metadata.as_object_mut().and_then(|m| m.remove("nast_import_background"));
            if let Some(path) = background.as_ref().and_then(Value::as_str).and_then(|v| background_paths.get(v)) {
                metadata["chat_backgrounds"] = json!([path]);
                metadata["custom_background"] = json!(format!("url(\"/assets/{path}\")"));
            }
            let name = format!("{label} - {} imported.jsonl", nast_storage::humanized_date_time());
            state.user.save_chat(&avatar, &name, &chat, false)?;
            chats.push(name);
        }
        if let Some(first) = chats.first() { imported.character.chat = Some(first.trim_end_matches(".jsonl").into()); }
    }
    write_character(&state, &avatar, &imported.character, &imported.avatar)?;
    state.hub.emit("chat_changed", json!({"avatar":avatar}));
    Ok(json!({"avatar":avatar,"name":imported.character.name,"format":imported.format,
        "resources":resources,"chats":chats,"warnings":imported.warnings,
        "embedded_book":imported.character.data.character_book.is_some()}))
}

pub fn create_character(state: SharedState, params: Value) -> RpcResult {
    let card = params.get("card").cloned().unwrap_or_else(|| json!({
        "spec":"chara_card_v2", "spec_version":"2.0", "data":params["data"]}));
    let mut request = params;
    request["data_base64"] = json!(base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&card).map_err(bad)?));
    request["filename"] = json!("card.json");
    import_character(state, request)
}

pub fn edit_character(state: SharedState, params: Value) -> RpcResult {
    let avatar = leaf(required(&params,"avatar")?)?;
    let mut character = read_character(&state, avatar)?;
    let old_world = character.data.extensions.world.clone();
    let changes = params["data"].as_object().ok_or_else(|| bad("missing data"))?;
    let mut data = serde_json::to_value(&character.data).map_err(internal)?;
    for (key, value) in changes {
        match key.as_str() {
            "fav" | "talkativeness" => data["extensions"][key] = value.clone(),
            "extensions" => {
                let extension = value.as_object().ok_or_else(|| bad("extensions must be an object"))?;
                data["extensions"].as_object_mut().unwrap().extend(extension.clone());
            }
            _ => data[key] = value.clone(),
        }
    }
    character.data = serde_json::from_value(data).map_err(bad)?;
    character.name = character.data.name.clone();
    character.description = character.data.description.clone();
    character.personality = character.data.personality.clone();
    character.scenario = character.data.scenario.clone();
    character.first_mes = character.data.first_mes.clone();
    character.mes_example = character.data.mes_example.clone();
    character.creator_notes = character.data.creator_notes.clone();
    character.tags = character.data.tags.clone();
    character.fav = character.data.extensions.fav;
    character.talkativeness = character.data.extensions.talkativeness.clone();
    if old_world.is_some() && character.data.extensions.world.as_deref().is_none_or(str::is_empty) {
        character.data.character_book = None;
    }
    sync_book(&state, &mut character)?;
    let image = std::fs::read(state.user.character_dir().join(avatar)).map_err(internal)?;
    write_character(&state, avatar, &character, &image)?;
    Ok(json!({"ok":true,"avatar":avatar,"character":character}))
}

pub fn export_character(state: SharedState, params: Value) -> RpcResult {
    let avatar = leaf(required(&params,"avatar")?)?;
    let mut character = read_character(&state,avatar)?;
    sync_book(&state,&mut character)?;
    // Shareable exports omit private selection/favorite state, matching ST.
    let mut value = serde_json::to_value(&character).map_err(internal)?;
    for key in ["chat","avatar","json_data","fav","fav_checked"] { value.as_object_mut().unwrap().remove(key); }
    value["data"]["extensions"].as_object_mut().unwrap().remove("fav");
    let format = params["format"].as_str().unwrap_or("png");
    let (bytes, mime) = match format {
        "json" => (serde_json::to_vec_pretty(&value).map_err(internal)?, "application/json"),
        "png" => {
            let image = std::fs::read(state.user.character_dir().join(avatar)).map_err(internal)?;
            let v3 = (character.spec.as_deref() == Some("chara_card_v3")).then_some(&value);
            (nast_cards::write_card(&image,&value,v3).map_err(bad)?, "image/png")
        }
        _ => return Err(bad("format must be png or json")),
    };
    Ok(json!({"filename":format!("{}.{format}",nast_cards::sanitize_filename(&character.name)),
        "mime":mime,"data_base64":base64::engine::general_purpose::STANDARD.encode(bytes)}))
}

pub fn import_book(state: SharedState, params: Value) -> RpcResult {
    let avatar = leaf(required(&params,"avatar")?)?;
    let mut character = read_character(&state, avatar)?;
    let embedded = character.data.character_book.as_ref().ok_or_else(|| bad("character has no embedded book"))?;
    let name = params["name"].as_str().filter(|s| !s.trim().is_empty()).map(str::to_owned)
        .or_else(|| embedded.name.clone().filter(|s| !s.is_empty())).unwrap_or_else(|| format!("{}'s Lorebook",character.name));
    leaf(&name)?;
    if state.user.list_worlds()?.contains(&name) && params["overwrite"] != true { return Err(bad("world exists; choose another name or confirm overwrite")); }
    let book = WorldInfoBook::from_embedded(embedded).map_err(bad)?;
    state.user.save_world(&name,&book)?;
    character.data.extensions.world = Some(name.clone());
    sync_book(&state,&mut character)?;
    let image = std::fs::read(state.user.character_dir().join(avatar)).map_err(internal)?;
    write_character(&state,avatar,&character,&image)?;
    Ok(json!({"ok":true,"name":name,"entries":book.entries.len()}))
}

/// Rename identity and update its explicit references. Originals are archived first.
pub async fn rename_character(state: SharedState, params: Value) -> RpcResult {
    let avatar = leaf(required(&params, "avatar")?)?;
    let name = required(&params, "name")?.trim();
    let generation = state.generation.read().await;
    if generation.abort.is_some() { return Err(bad("stop generation before renaming a character")); }
    let mut character = read_character(&state, avatar)?;
    if character.name == name { return Ok(json!({"avatar":avatar,"name":name})); }
    let new_avatar = state.user.unique_character_file(name);
    let old_stem = avatar.trim_end_matches(".png");
    let new_stem = new_avatar.trim_end_matches(".png");
    let old_name = character.name.clone();
    character.name = name.into(); character.data.name = name.into(); character.avatar = Some(new_avatar.clone());
    let mut settings_guard = state.settings.write().await;
    let mut settings = settings_guard.clone();
    for pointer in ["/world_info/char_lore", "/world_info/charLore", "/world_info_settings/world_info/charLore"] {
        if let Some(lore) = settings.pointer_mut(pointer).and_then(Value::as_array_mut) {
            for item in lore { if item["name"] == old_stem { item["name"] = json!(new_stem); } }
        }
    }
    for pointer in ["/extension_settings/character_allowed_regex"] {
        if let Some(values) = settings.pointer_mut(pointer).and_then(Value::as_array_mut) {
            for value in values { if value == avatar { *value = json!(new_avatar); } }
        }
    }
    if let Some(tags) = settings.get_mut("tag_map").and_then(Value::as_object_mut) {
        if let Some(value) = tags.remove(avatar) { tags.insert(new_avatar.clone(), value); }
    }
    let mut writes: Vec<(std::path::PathBuf, Vec<u8>)> = Vec::new();
    let mut removals = Vec::new();
    let old_card = state.user.character_dir().join(avatar);
    let image = std::fs::read(&old_card).map_err(internal)?;
    let card = json!(character);
    let bytes = nast_cards::write_card(&image, &card, (character.spec.as_deref() == Some("chara_card_v3")).then_some(&card)).map_err(bad)?;
    writes.push((state.user.character_dir().join(&new_avatar), bytes));
    removals.push(old_card);
    let rename_messages = |chat: &mut nast_model::ChatFile, private: bool| {
        for message in chat.0.iter_mut().skip(1) {
            let ours = message["original_avatar"] == avatar || (private && message["is_user"] == false && message["name"] == old_name);
            if ours {
                message["name"] = json!(name);
                if message.get("original_avatar").is_some() { message["original_avatar"] = json!(new_avatar); }
                if message.get("force_avatar").is_some() { message["force_avatar"] = json!(format!("/thumbnail?file={new_avatar}")); }
            }
        }
    };
    let encode_chat = |chat: &nast_model::ChatFile| chat.0.iter().map(Value::to_string).collect::<Vec<_>>().join("\n").into_bytes();
    for file in state.user.list_chats(avatar)? {
        let mut chat = state.user.read_chat(avatar, &file)?;
        rename_messages(&mut chat, true);
        let relative = std::path::PathBuf::from("chats").join(new_stem).join(&file);
        if state.user.root.join(&relative).exists() { return Err(bad("target chat already exists")); }
        writes.push((state.user.root.join(relative), encode_chat(&chat)));
        removals.push(state.user.root.join("chats").join(old_stem).join(file));
    }
    for mut group in state.user.list_groups()? {
        let mut changed = false;
        for member in group.members.iter_mut().chain(group.disabled_members.iter_mut()) {
            if member == avatar { *member = new_avatar.clone(); changed = true; }
        }
        if changed {
            writes.push((state.user.root.join("groups").join(format!("{}.json", group.id)), serde_json::to_vec_pretty(&group).map_err(internal)?));
        }
        for chat_id in &group.chats {
            let existing = match state.user.read_group_chat(chat_id) {
                Ok(chat) => Some(chat),
                Err(nast_storage::StorageError::NotFound(_)) => None,
                Err(error) => return Err(error.into()),
            };
            if let Some(mut chat) = existing {
                if chat.0.iter().any(|message| message["original_avatar"] == avatar) {
                    rename_messages(&mut chat, false);
                    writes.push((state.user.group_chat_path(chat_id), encode_chat(&chat)));
                }
            }
        }
    }
    for world in state.user.list_worlds()? {
        let mut book = state.user.read_world(&world)?;
        let mut changed = false;
        for entry in book.entries.values_mut() {
            if let Some(filter) = &mut entry.character_filter {
                for value in &mut filter.names { if value == old_stem { *value = new_stem.into(); changed = true; } }
            }
        }
        if changed { writes.push((state.user.root.join("worlds").join(format!("{world}.json")), serde_json::to_vec_pretty(&book).map_err(internal)?)); }
    }
    writes.push((state.user.settings_path(), serde_json::to_vec_pretty(&settings).map_err(internal)?));
    let backup = state.user.root.join("backups").join(format!("character-rename-{}", uuid::Uuid::new_v4()));
    let mut originals = std::collections::BTreeMap::new();
    for path in writes.iter().map(|(path, _)| path).chain(removals.iter()) {
        if originals.contains_key(path) { continue; }
        let previous = if path.exists() { Some(std::fs::read(path).map_err(internal)?) } else { None };
        if let Some(bytes) = &previous {
            let relative = path.strip_prefix(&state.user.root).map_err(internal)?;
            nast_storage::atomic_write(&backup.join(relative), bytes)?;
        }
        originals.insert(path.clone(), previous);
    }
    let result = (|| -> Result<(), RpcError> {
        for (path, bytes) in &writes { nast_storage::atomic_write(path, bytes)?; }
        for path in &removals { std::fs::remove_file(path).map_err(internal)?; }
        Ok(())
    })();
    if let Err(error) = result {
        for (path, bytes) in originals {
            match bytes {
                Some(bytes) => { let _ = nast_storage::atomic_write(&path, &bytes); }
                None => { let _ = std::fs::remove_file(path); }
            }
        }
        return Err(internal(format!("{error}; originals archived at {}", backup.display())));
    }
    *settings_guard = settings;
    state.hub.emit("character_renamed", json!({"old_avatar":avatar,"avatar":new_avatar,"name":name}));
    Ok(json!({"avatar":new_avatar,"name":name,"backup":backup}))
}
