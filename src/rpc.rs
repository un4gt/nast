//! RPC 方法分发：characters/worlds/settings/chats。
//!
//! 方法名风格 `<域>.<动作>`。所有 IO 走 nast-storage（ST 同构布局）。

use crate::events;
use crate::state::SharedState;
use base64::Engine as _;
use nast_cards::CardSpec;
use nast_model::card::Character;
use nast_model::chat::{ChatFile, ChatHeader};
use nast_model::world::WorldInfoBook;
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    BadRequest(String),
    #[error("integrity")]
    Integrity,
    #[error("internal: {0}")]
    Internal(String),
}

impl From<nast_storage::StorageError> for RpcError {
    fn from(e: nast_storage::StorageError) -> Self {
        match e {
            nast_storage::StorageError::IntegrityMismatch => RpcError::Integrity,
            nast_storage::StorageError::NotFound(s) => RpcError::NotFound(s),
            other => RpcError::Internal(other.to_string()),
        }
    }
}

pub type RpcResult = Result<Value, RpcError>;

/// 主分发器。
pub async fn dispatch(state: SharedState, method: &str, params: Value) -> RpcResult {
    match method {
        "settings.get" => settings_get(state).await,
        "settings.save" => settings_save(state, params).await,
        "secrets.get" => secrets_get(state).await,
        "secrets.set" => secrets_set(state, params).await,
        "models.list" => models_list(state, params).await,
        "characters.all" => characters_all(state),
        "characters.get" => characters_get(state, params),
        "characters.import" => characters_import(state, params),
        "characters.delete" => characters_delete(state, params),
        "characters.chats" => characters_chats(state, params),
        "characters.edit" => characters_edit(state, params),
        "characters.duplicate" => characters_duplicate(state, params),
        "chats.stats" => chats_stats(state, params).await,
        "chats.get" => chats_get(state, params),
        "chats.save" => chats_save(state, params),
        "chats.delete" => chats_delete(state, params),
        "chats.rename" => chats_rename(state, params),
        "chats.delete_message" => chats_delete_message(state, params),
        "chats.export" => chats_export(state, params),
        "chats.set_note" => chats_set_note(state, params),
        "chats.set_persona" => chats_set_persona(state, params),
        "chats.set_world" => chats_set_world(state, params),
        "chats.update_message" => chats_update_message(state, params),
        "plugins.list" => plugins_list(state),
        "plugins.reload" => plugins_reload(state),
        "generate.run" => generate_run(state, params).await,
        "generate.stop" => generate_stop(state).await,
        "groups.create" => groups_create(state, params),
        "groups.edit" => groups_edit(state, params),
        "groups.delete" => groups_delete(state, params),
        "groups.get" => groups_get(state),
        "groups.chats" => groups_chats(state, params),
        "generate.group" => crate::group_gen::generate_group(state, params).await,
        "chats.swipe" => chats_swipe(state, params),
        "chats.new" => chats_new(state, params),
        "worlds.list" => worlds_list(state),
        "worlds.get" => worlds_get(state, params),
        "worlds.save" => worlds_save(state, params),
        "worlds.delete" => worlds_delete(state, params),
        "groups.all" => groups_all(state),
        _ => Err(RpcError::NotFound(format!("method {method}"))),
    }
}

// ---------- settings ----------

async fn settings_get(state: SharedState) -> RpcResult {
    Ok(state.settings.read().await.clone())
}

async fn settings_save(state: SharedState, params: Value) -> RpcResult {
    let settings = params
        .get("settings")
        .ok_or_else(|| RpcError::BadRequest("missing settings".into()))?;
    state.user.save_settings(settings)?;
    *state.settings.write().await = settings.clone();
    state.hub.emit(events::SETTINGS_UPDATED, json!({}));
    Ok(json!({"ok": true}))
}

// ---------- secrets / models ----------

/// 掩码：仅保留尾 4 位，其余打码（UI 显示用；值本身永不下发）。
fn mask_secret(v: &str) -> String {
    let trimmed = v.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let tail: String = trimmed.chars().skip(trimmed.chars().count().saturating_sub(4)).collect();
    format!("••••{tail}")
}

/// 密钥清单（掩码视图）。ST secrets.json 形态：{key: [{id, value, label, active}]}。
async fn secrets_get(state: SharedState) -> RpcResult {
    let secrets = state.secrets.read().await;
    let mut out = serde_json::Map::new();
    if let Some(map) = secrets.as_object() {
        for (k, v) in map {
            let entries: Vec<Value> = v
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .map(|e| {
                            json!({
                                "id": e.get("id").cloned().unwrap_or(Value::Null),
                                "label": e.get("label").and_then(|l| l.as_str()).unwrap_or(""),
                                "active": e.get("active").and_then(|a| a.as_bool()).unwrap_or(false),
                                "masked": mask_secret(e.get("value").and_then(|s| s.as_str()).unwrap_or("")),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            out.insert(k.clone(), Value::Array(entries));
        }
    }
    Ok(Value::Object(out))
}

/// 写入单密钥（单条目形态，保持 ST 数组兼容）。value 为空 = 清除。
async fn secrets_set(state: SharedState, params: Value) -> RpcResult {
    let key = param_str(&params, "key")?.to_string();
    let value = params.get("value").and_then(|v| v.as_str()).unwrap_or("");
    if !key.starts_with("api_key_") {
        return Err(RpcError::BadRequest("key must be an api_key_* name".into()));
    }
    let mut secrets = state.secrets.write().await;
    if value.trim().is_empty() {
        if let Some(map) = secrets.as_object_mut() {
            map.remove(&key);
        }
    } else {
        let entry = json!({
            "id": Uuid::new_v4().to_string(),
            "value": value.trim(),
            "label": "",
            "active": true,
        });
        secrets[key] = json!([entry]);
    }
    state.user.save_secrets(&secrets)?;
    Ok(json!({"ok": true, "masked": mask_secret(value)}))
}

/// 模型列表：默认用已保存配置；连接测试可传 url/key 覆盖（不必先保存）。
async fn models_list(state: SharedState, params: Value) -> RpcResult {
    let oai: nast_model::preset::OaiSettings = serde_json::from_value(
        state.settings.read().await.get("oai_settings").cloned().unwrap_or(json!({})),
    )
    .unwrap_or_default();
    let saved_key = {
        let secrets = state.secrets.read().await;
        crate::connection::active_secret(&secrets, crate::connection::SECRET_CUSTOM)
            .or_else(|| {
                std::env::var("OPENAI_API_KEY")
                    .ok()
                    .filter(|v| !v.trim().is_empty())
            })
            .unwrap_or_default()
    };
    let base_url = params
        .get("url")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| crate::connection::custom_base_url(&oai));
    let api_key = params
        .get("key")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or(saved_key);
    let provider = nast_providers::Provider::new(nast_providers::ProviderKind::OpenAiCompat {
        base_url,
        api_key,
    });
    let models = provider
        .list_models()
        .await
        .map_err(|e| RpcError::Internal(e.to_string()))?;
    Ok(json!({"data": models}))
}

// ---------- characters ----------

pub fn read_character(state: &SharedState, avatar: &str) -> Result<Character, RpcError> {
    let path = state.user.character_dir().join(avatar);
    let bytes = std::fs::read(&path).map_err(|_| RpcError::NotFound(avatar.into()))?;
    nast_cards::read_card(&bytes).map_err(|e| RpcError::BadRequest(e.to_string()))
}

/// 角色列表（浅）：解码每张 PNG 的 chara chunk（无索引文件，GOTCHA #18）。
fn characters_all(state: SharedState) -> RpcResult {
    let mut out = Vec::new();
    for avatar in state.user.list_characters()? {
        match read_character(&state, &avatar) {
            Ok(ch) => out.push(json!({
                "avatar": avatar,
                "name": ch.name,
                "description": ch.data.description,
                "tags": ch.data.tags,
                "fav": ch.fav,
                "chat": ch.chat,
            })),
            Err(e) => out.push(json!({"avatar": avatar, "error": e.to_string()})),
        }
    }
    Ok(Value::Array(out))
}

fn characters_get(state: SharedState, params: Value) -> RpcResult {
    let avatar = param_str(&params, "avatar")?;
    let ch = read_character(&state, &avatar)?;
    Ok(serde_json::to_value(ch).map_err(|e| RpcError::Internal(e.to_string()))?)
}

/// 导入：base64 PNG 或 JSON 卡。
fn characters_import(state: SharedState, params: Value) -> RpcResult {
    let data_b64 = param_str(&params, "data_base64")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_b64)
        .map_err(|_| RpcError::BadRequest("data_base64 is not valid base64".into()))?;
    let character: Character = if bytes.starts_with(&[0x89, b'P']) {
        nast_cards::read_card(&bytes).map_err(|e| RpcError::BadRequest(e.to_string()))?
    } else {
        let json: Value = serde_json::from_slice(&bytes)
            .map_err(|e| RpcError::BadRequest(format!("invalid card json: {e}")))?;
        Character::from_card_json(&json).map_err(|e| RpcError::BadRequest(e.to_string()))?
    };
    character.validate().map_err(|e| RpcError::BadRequest(e.to_string()))?;

    let file_name = state.user.unique_character_file(&character.name);
    // 卡 JSON 双写：V2 形态 = 整个 character（顶层+data），V3 原样存 ccv3
    let v2_json = serde_json::to_value(&character)
        .map_err(|e| RpcError::Internal(e.to_string()))?;
    let png = if bytes.starts_with(&[0x89, b'P']) {
        bytes // 原样保留图像
    } else {
        nast_cards::minimal_png()
    };
    // V3 卡同时写 ccv3 chunk（ST write 总是 chara + ccv3 双写）
    let ccv3_json = if v2_json.get("spec").and_then(|s| s.as_str()) == Some("chara_card_v3") {
        Some(&v2_json)
    } else {
        None
    };
    let out = nast_cards::write_card(&png, &v2_json, ccv3_json)
        .map_err(|e| RpcError::Internal(e.to_string()))?;
    std::fs::write(state.user.character_dir().join(&file_name), out)
        .map_err(|e| RpcError::Internal(e.to_string()))?;

    // 自动建首个聊天文件
    let chat_name = format!(
        "{} - {}",
        character.name,
        nast_storage::humanized_date_time()
    );
    let header = ChatHeader {
        user_name: "unused".into(),
        character_name: "unused".into(),
        chat_metadata: Default::default(),
    };
    let chat = ChatFile(vec![serde_json::to_value(&header).unwrap()]);
    state
        .user
        .save_chat(&file_name, &format!("{chat_name}.jsonl"), &chat, false)?;

    state.hub.emit(events::CHAT_CHANGED, json!({"avatar": file_name}));
    Ok(json!({"avatar": file_name, "name": character.name}))
}

fn characters_delete(state: SharedState, params: Value) -> RpcResult {
    let avatar = param_str(&params, "avatar")?;
    let path = state.user.character_dir().join(&avatar);
    std::fs::remove_file(&path).map_err(|_| RpcError::NotFound(avatar.to_string()))?;
    // 删除聊天目录
    let chat_dir = state.user.chat_dir_for_character(&avatar);
    if chat_dir.exists() {
        std::fs::remove_dir_all(chat_dir).map_err(|e| RpcError::Internal(e.to_string()))?;
    }
    Ok(json!({"ok": true}))
}

/// 编辑角色卡：部分更新 data 字段（description/personality/scenario/first_mes/mes_example/
/// system_prompt/post_history_instructions/tags 等），写回 PNG chara chunk。
fn characters_edit(state: SharedState, params: Value) -> RpcResult {
    let avatar = param_str(&params, "avatar")?;
    let mut ch = read_character(&state, avatar)?;

    let data = params
        .get("data")
        .and_then(|v| v.as_object())
        .ok_or_else(|| RpcError::BadRequest("missing data".into()))?;
    // 收藏开关（顶层 fav，非 data 字段）
    if let Some(fav) = data.get("fav").and_then(|v| v.as_bool()) {
        ch.fav = fav;
    }

    for (k, v) in data {
        // 白名单：仅允许编辑卡内容字段
        if matches!(
            k.as_str(),
            "description" | "personality" | "scenario" | "first_mes" | "mes_example"
                | "system_prompt" | "post_history_instructions" | "creator_notes"
                | "character_version" | "tags" | "alternate_greetings"
        ) {
            if let Ok(v) = serde_json::from_value::<serde_json::Value>(v.clone()) {
                ch.data.extra.remove(k);                // 直接写 typed 字段
                match k.as_str() {
                    "description" => ch.data.description = v.as_str().unwrap_or_default().to_string(),
                    "personality" => ch.data.personality = v.as_str().unwrap_or_default().to_string(),
                    "scenario" => ch.data.scenario = v.as_str().unwrap_or_default().to_string(),
                    "first_mes" => ch.data.first_mes = v.as_str().unwrap_or_default().to_string(),
                    "mes_example" => ch.data.mes_example = v.as_str().unwrap_or_default().to_string(),
                    "system_prompt" => ch.data.system_prompt = v.as_str().unwrap_or_default().to_string(),
                    "post_history_instructions" => {
                        ch.data.post_history_instructions = v.as_str().unwrap_or_default().to_string()
                    }
                    "creator_notes" => ch.data.creator_notes = v.as_str().unwrap_or_default().to_string(),
                    "character_version" => {
                        ch.data.character_version = v.as_str().unwrap_or_default().to_string()
                    }
                    "tags" => {
                        ch.data.tags = v
                            .as_array()
                            .map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect())
                            .unwrap_or_default()
                    }
                    "alternate_greetings" => {
                        ch.data.alternate_greetings = v
                            .as_array()
                            .map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect())
                            .unwrap_or_default()
                    }
                    _ => {}
                }
                // 同步顶层（ST 内存形态双写）
                match k.as_str() {
                    "description" => ch.description = ch.data.description.clone(),
                    "personality" => ch.personality = ch.data.personality.clone(),
                    "scenario" => ch.scenario = ch.data.scenario.clone(),
                    "first_mes" => ch.first_mes = ch.data.first_mes.clone(),
                    "mes_example" => ch.mes_example = ch.data.mes_example.clone(),
                    _ => {}
                }
            }
        }
    }

    // 写回 PNG（剔除旧 chunk + 双写 chara）
    let path = state.user.character_dir().join(avatar);
    let png_bytes = std::fs::read(&path).map_err(|e| RpcError::Internal(e.to_string()))?;
    let v2_json =
        serde_json::to_value(&ch).map_err(|e| RpcError::Internal(e.to_string()))?;
    let ccv3 = if v2_json.get("spec").and_then(|s| s.as_str()) == Some("chara_card_v3") {
        Some(&v2_json)
    } else {
        None
    };
    let out = nast_cards::write_card(&png_bytes, &v2_json, ccv3)
        .map_err(|e| RpcError::Internal(e.to_string()))?;
    std::fs::write(&path, out).map_err(|e| RpcError::Internal(e.to_string()))?;
    Ok(json!({"ok": true}))
}

fn characters_chats(state: SharedState, params: Value) -> RpcResult {
    let avatar = param_str(&params, "avatar")?;
    let chats = state.user.list_chats(&avatar)?;
    Ok(json!(chats))
}

/// 复制角色（含卡内容；聊天不复制）。
fn characters_duplicate(state: SharedState, params: Value) -> RpcResult {
    let avatar = param_str(&params, "avatar")?;
    let src = read_character(&state, avatar)?;
    let mut copy = src.clone();
    copy.name = format!("{} (copy)", src.name);
    let file_name = state.user.unique_character_file(&copy.name);
    let v2_json = serde_json::to_value(&copy).map_err(|e| RpcError::Internal(e.to_string()))?;
    let png_bytes = std::fs::read(state.user.character_dir().join(avatar))
        .map_err(|e| RpcError::Internal(e.to_string()))?;
    let out = nast_cards::write_card(&png_bytes, &v2_json, None)
        .map_err(|e| RpcError::Internal(e.to_string()))?;
    std::fs::write(state.user.character_dir().join(&file_name), out)
        .map_err(|e| RpcError::Internal(e.to_string()))?;
    state.hub.emit(events::CHAT_CHANGED, json!({"avatar": file_name}));
    Ok(json!({"avatar": file_name, "name": copy.name}))
}

/// 聊天 token 统计：按当前源 tokenizer 计历史消息与预算。
async fn chats_stats(state: SharedState, params: Value) -> RpcResult {
    let avatar = param_str(&params, "avatar")?;
    let file_name = param_str(&params, "file_name")?;
    let oai: nast_model::preset::OaiSettings = serde_json::from_value(
        state.settings.read().await.get("oai_settings").cloned().unwrap_or(json!({})),
    )
    .unwrap_or_default();
    let chat = state.user.read_chat(&avatar, &file_name)?;
    let model = crate::connection::model_for(&oai);
    let tok = nast_engine::prompt::tok_for;
    let mut per_message: Vec<i64> = Vec::new();
    let mut total: i64 = 0;
    for v in chat.0.iter().skip(1) {
        let mes = v.get("mes").and_then(|m| m.as_str()).unwrap_or_default();
        let t = tok(mes, &oai.chat_completion_source, &model);
        per_message.push(t);
        total += t;
    }
    Ok(json!({
        "total": total,
        "budget": oai.openai_max_context - oai.openai_max_tokens,
        "per_message": per_message,
    }))
}

// ---------- chats ----------

fn chats_get(state: SharedState, params: Value) -> RpcResult {
    let avatar = param_str(&params, "avatar")?;
    let file_name = param_str(&params, "file_name")?;
    let chat = state.user.read_chat(&avatar, &file_name)?;
    Ok(serde_json::to_value(chat.0).map_err(|e| RpcError::Internal(e.to_string()))?)
}

fn chats_save(state: SharedState, params: Value) -> RpcResult {
    let avatar = param_str(&params, "avatar")?;
    let file_name = param_str(&params, "file_name")?;
    let force = params.get("force").and_then(|v| v.as_bool()).unwrap_or(false);
    let lines = params
        .get("chat")
        .and_then(|v| v.as_array())
        .ok_or_else(|| RpcError::BadRequest("missing chat array".into()))?;
    let chat = ChatFile(lines.clone());
    state.user.save_chat(&avatar, &file_name, &chat, force)?;
    Ok(json!({"ok": true}))
}

fn chats_delete(state: SharedState, params: Value) -> RpcResult {
    let avatar = param_str(&params, "avatar")?;
    let file_name = param_str(&params, "file_name")?;
    state.user.delete_chat(&avatar, &file_name)?;
    Ok(json!({"ok": true}))
}

fn chats_rename(state: SharedState, params: Value) -> RpcResult {
    let avatar = param_str(&params, "avatar")?;
    let original = param_str(&params, "original_file")?;
    let renamed = param_str(&params, "renamed_file")?;
    let sanitized = state.user.rename_chat(&avatar, &original, &renamed)?;
    Ok(json!({"name": sanitized}))
}

// ---------- worlds ----------

fn groups_create(state: SharedState, params: Value) -> RpcResult {
    let mut group: nast_model::group::Group = serde_json::from_value(
        params.get("group").cloned().unwrap_or(json!({})),
    )
    .unwrap_or_default();
    group.id = uuid::Uuid::new_v4().to_string();
    if group.name.is_empty() {
        group.name = "New Group".into();
    }
    let chat_id = nast_storage::humanized_date_time();
    group.chat_id = chat_id.clone();
    group.chats = vec![chat_id];
    state.user.save_group(&group)?;
    serde_json::to_value(&group).map_err(|e| RpcError::Internal(e.to_string()))
}

fn groups_edit(state: SharedState, params: Value) -> RpcResult {
    let group: nast_model::group::Group = serde_json::from_value(
        params
            .get("group")
            .cloned()
            .ok_or_else(|| RpcError::BadRequest("missing group".into()))?,
    )
    .map_err(|e| RpcError::BadRequest(format!("invalid group: {e}")))?;
    state.user.save_group(&group)?;
    Ok(json!({"ok": true}))
}

fn groups_delete(state: SharedState, params: Value) -> RpcResult {
    let id = param_str(&params, "id")?;
    state.user.delete_group(id)?;
    Ok(json!({"ok": true}))
}

fn groups_get(state: SharedState) -> RpcResult {
    let groups = state.user.list_groups()?;
    serde_json::to_value(groups).map_err(|e| RpcError::Internal(e.to_string()))
}

fn groups_chats(state: SharedState, params: Value) -> RpcResult {
    let id = param_str(&params, "id")?;
    let groups = state.user.list_groups()?;
    let group = groups
        .iter()
        .find(|g| g.id == id)
        .ok_or_else(|| RpcError::NotFound(format!("group {id}")))?;
    Ok(json!(group.chats))
}

/// 删除指定索引的消息（ST 双击删除语义；索引不含 header）。
fn chats_delete_message(state: SharedState, params: Value) -> RpcResult {
    let avatar = param_str(&params, "avatar")?;
    let file_name = param_str(&params, "file_name")?;
    let index = params
        .get("index")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RpcError::BadRequest("missing index".into()))? as usize;

    let mut chat = state.user.read_chat(avatar, file_name)?;
    // +1 跳过 header
    let pos = index + 1;
    if pos >= chat.0.len() {
        return Err(RpcError::BadRequest("index out of range".into()));
    }
    chat.0.remove(pos);
    state.user.save_chat(avatar, file_name, &chat, false)?;
    Ok(json!({"ok": true}))
}

/// 导出聊天为 jsonl 文本（ST 导出格式原样）。
fn chats_export(state: SharedState, params: Value) -> RpcResult {
    let avatar = param_str(&params, "avatar")?;
    let file_name = param_str(&params, "file_name")?;
    let raw = std::fs::read_to_string(state.user.chat_dir_for_character(avatar).join(file_name))
        .map_err(|_| RpcError::NotFound(file_name.to_string()))?;
    Ok(json!({"content": raw}))
}

/// 设置聊天级 Author's Note（AN.js metadata_keys：prompt/interval/depth/position/role）。
/// note.prompt 为 null/缺失 = 清除。
fn chats_set_note(state: SharedState, params: Value) -> RpcResult {
    let avatar = param_str(&params, "avatar")?;
    let file_name = param_str(&params, "file_name")?;
    let note = params
        .get("note")
        .cloned()
        .unwrap_or(Value::Null);
    let mut chat = state.user.read_chat(&avatar, &file_name)?;
    if let Some(header) = chat.0.first_mut() {
        let metadata = header
            .get_mut("chat_metadata")
            .ok_or_else(|| RpcError::BadRequest("missing chat_metadata".into()))?;
        let prompt = note.get("prompt").cloned().unwrap_or(Value::Null);
        if prompt.is_null() || prompt.as_str() == Some("") {
            metadata.as_object_mut().map(|m| {
                m.remove("note_prompt");
                m.remove("note_interval");
                m.remove("note_depth");
                m.remove("note_position");
                m.remove("note_role");
            });
        } else {
            let set_i64 = |m: &mut serde_json::Map<String, Value>, k: &str, v: Option<i64>| {
                if let Some(v) = v {
                    m.insert(k.into(), json!(v));
                }
            };
            if let Some(m) = metadata.as_object_mut() {
                m.insert("note_prompt".into(), prompt);
                set_i64(m, "note_interval", note.get("interval").and_then(|v| v.as_i64()));
                set_i64(m, "note_depth", note.get("depth").and_then(|v| v.as_i64()));
                set_i64(m, "note_position", note.get("position").and_then(|v| v.as_i64()));
                set_i64(m, "note_role", note.get("role").and_then(|v| v.as_i64()));
            }
        }
        state.user.save_chat(&avatar, &file_name, &chat, false)?;
        return Ok(json!({"ok": true}));
    }
    Err(RpcError::BadRequest("empty chat".into()))
}

/// 绑定/解绑聊天 persona（chat_metadata.persona）。
fn chats_set_persona(state: SharedState, params: Value) -> RpcResult {
    let avatar = param_str(&params, "avatar")?;
    let file_name = param_str(&params, "file_name")?;
    let persona = params.get("persona").cloned().unwrap_or(Value::Null);
    let mut chat = state.user.read_chat(&avatar, &file_name)?;
    if let Some(header) = chat.0.first_mut() {
        let metadata = header
            .get_mut("chat_metadata")
            .ok_or_else(|| RpcError::BadRequest("missing chat_metadata".into()))?;
        if let Some(m) = metadata.as_object_mut() {
            if persona.is_null() || persona.as_str() == Some("") {
                m.remove("persona");
            } else {
                m.insert("persona".into(), persona);
            }
        }
        state.user.save_chat(&avatar, &file_name, &chat, false)?;
        return Ok(json!({"ok": true}));
    }
    Err(RpcError::BadRequest("empty chat".into()))
}

/// 绑定/解绑聊天世界书（chat_metadata.world）。
fn chats_set_world(state: SharedState, params: Value) -> RpcResult {
    let avatar = param_str(&params, "avatar")?;
    let file_name = param_str(&params, "file_name")?;
    let world = params.get("world").cloned().unwrap_or(Value::Null);
    let mut chat = state.user.read_chat(&avatar, &file_name)?;
    if let Some(header) = chat.0.first_mut() {
        let metadata = header
            .get_mut("chat_metadata")
            .ok_or_else(|| RpcError::BadRequest("missing chat_metadata".into()))?;
        if let Some(m) = metadata.as_object_mut() {
            if world.is_null() || world.as_str() == Some("") {
                m.remove("world");
            } else {
                m.insert("world".into(), world);
            }
        }
        state.user.save_chat(&avatar, &file_name, &chat, false)?;
        return Ok(json!({"ok": true}));
    }
    Err(RpcError::BadRequest("empty chat".into()))
}

/// 编辑指定消息（script.js updateMessage 8080-8136 语义）：
/// - 按 index 定位（不按内容匹配）；只更新当前 swipe（mes + swipes[swipe_id]）
/// - 编辑后重跑正则（runOnEdit 开关生效，USER_INPUT/AI_OUTPUT placement）
/// - chat_metadata.tainted = true
fn chats_update_message(state: SharedState, params: Value) -> RpcResult {
    let avatar = param_str(&params, "avatar")?.to_string();
    let file_name = param_str(&params, "file_name")?.to_string();
    let index = params
        .get("index")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RpcError::BadRequest("missing index".into()))? as usize;
    let text = param_str(&params, "text")?.to_string();
    let reasoning = params.get("reasoning").and_then(|v| v.as_str()).map(String::from);

    let mut chat = state.user.read_chat(&avatar, &file_name)?;
    let pos = index + 1;
    if pos >= chat.0.len() {
        return Err(RpcError::BadRequest("index out of range".into()));
    }
    let mut msg: nast_model::chat::ChatMessage = serde_json::from_value(chat.0[pos].clone())
        .map_err(|e| RpcError::BadRequest(format!("invalid message: {e}")))?;

    // 正则 runOnEdit pass
    let character = read_character(&state, &avatar)?;
    let metadata = chat.metadata();
    let settings_snapshot = state.settings.try_read().map(|s| s.clone()).unwrap_or_default();
    let scripts = crate::generate::collect_regex_scripts_for(&settings_snapshot, &character, &metadata);
    let placement = if msg.is_user {
        nast_model::regex_script::RP_USER_INPUT
    } else {
        nast_model::regex_script::RP_AI_OUTPUT
    };
    let macro_fn = move |s: &str| {
        crate::prompt_bridge::substitute_basic(s, "User", &character.name)
    };
    let edited = nast_engine::regex_engine::get_regexed_string(
        &text,
        placement,
        &scripts,
        &nast_engine::regex_engine::RegexParams { is_edit: true, ..Default::default() },
        &macro_fn,
    );

    msg.mes = edited.clone();
    if let Some(swipe_id) = msg.swipe_id {
        if let Some(swipes) = msg.swipes.as_mut() {
            let sid = (swipe_id.max(0) as usize).min(swipes.len().saturating_sub(1));
            if sid < swipes.len() {
                swipes[sid] = edited.clone();
            }
        }
    }
    if let Some(r) = reasoning {
        msg.extra.reasoning = if r.is_empty() { None } else { Some(r) };
    }
    // display_text 过期：编辑后重算（display pass）
    {
        let params = nast_engine::regex_engine::RegexParams {
            is_markdown: true,
            ..Default::default()
        };
        let display = nast_engine::regex_engine::get_regexed_string(
            &edited,
            nast_model::regex_script::RP_AI_OUTPUT,
            &scripts,
            &params,
            &macro_fn,
        );
        if display != edited {
            msg.extra.display_text = Some(display);
        } else {
            msg.extra.display_text = None;
        }
    }
    chat.0[pos] = serde_json::to_value(&msg).map_err(|e| RpcError::Internal(e.to_string()))?;
    // tainted 标记
    if let Some(header) = chat.0.first_mut() {
        if let Some(m) = header
            .get_mut("chat_metadata")
            .and_then(|m| m.as_object_mut())
        {
            m.insert("tainted".into(), json!(true));
        }
    }
    state.user.save_chat(&avatar, &file_name, &chat, false)?;
    Ok(json!({"ok": true, "mes": edited}))
}

fn plugins_list(state: SharedState) -> RpcResult {
    let host = state.plugins.lock().unwrap();
    Ok(json!({ "plugins": host.list(), "lua": host.lua_names() }))
}

fn plugins_reload(state: SharedState) -> RpcResult {
    let mut host = state.plugins.lock().unwrap();
    let loaded = host.load_dir().map_err(|e| RpcError::Internal(e.to_string()))?;
    Ok(json!({ "loaded": loaded }))
}

/// 创建新聊天：写入首行 header + 随机（或指定）开场白消息。
/// greeting_index: -1 = 随机；0 = first_mes；1.. = alternate_greetings。
fn chats_new(state: SharedState, params: Value) -> RpcResult {
    let avatar = param_str(&params, "avatar")?;
    let character = read_character(&state, avatar)?;
    let greeting_index = params
        .get("greeting_index")
        .and_then(|v| v.as_i64())
        .unwrap_or(-1);

    let mut greetings: Vec<String> = Vec::new();
    greetings.push(character.first_mes.clone());
    greetings.extend(character.data.alternate_greetings.clone());
    let greeting = if greeting_index < 0 || greeting_index as usize >= greetings.len() {
        use rand::Rng;
        let idx = rand::thread_rng().gen_range(0..greetings.len().max(1));
        greetings.get(idx).cloned().unwrap_or_default()
    } else {
        greetings[greeting_index as usize].clone()
    };

    let metadata = nast_model::chat::ChatMetadata {
        integrity: Some(uuid::Uuid::new_v4().to_string()),
        ..Default::default()
    };
    let header = ChatHeader {
        user_name: "unused".into(),
        character_name: "unused".into(),
        chat_metadata: metadata,
    };
    let greeting_msg = json!({
        "name": character.name,
        "is_user": false,
        "is_system": false,
        "send_date": nast_storage::message_time_stamp(),
        "mes": greeting,
        "swipes": [greeting],
        "swipe_id": 0,
        "swipe_info": [{"send_date": nast_storage::message_time_stamp()}],
    });
    let chat = ChatFile(vec![
        serde_json::to_value(&header).map_err(|e| RpcError::Internal(e.to_string()))?,
        greeting_msg,
    ]);

    // 文件名：humanizedDateTime（ST 语义）
    let file_name = format!("{} - {}.jsonl", character.name, nast_storage::humanized_date_time());
    state.user.save_chat(avatar, &file_name, &chat, false)?;
    Ok(json!({"file_name": file_name}))
}

/// 切换到指定 swipe（左/右箭头）：更新 swipe_id、mes 镜像当前 swipe。
fn chats_swipe(state: SharedState, params: Value) -> RpcResult {
    let avatar = param_str(&params, "avatar")?;
    let file_name = param_str(&params, "file_name")?;
    let mut chat = state.user.read_chat(&avatar, &file_name)?;
    let last = chat
        .0
        .last_mut()
        .ok_or_else(|| RpcError::BadRequest("empty chat".into()))?;

    let direction = params.get("direction").and_then(|v| v.as_str()).unwrap_or("right");
    let cur = last.get("swipe_id").and_then(|v| v.as_i64()).unwrap_or(0);
    let total = last.get("swipes").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(1);

    let new_id = match direction {
        "left" => (cur - 1).max(0),
        _ => (cur + 1).min(total as i64 - 1),
    };
    if new_id != cur {
        last["swipe_id"] = json!(new_id);
        if let Some(swipes) = last.get("swipes").and_then(|v| v.as_array()) {
            if let Some(text) = swipes.get(new_id as usize).and_then(|v| v.as_str()) {
                last["mes"] = json!(text);
            }
        }
        // 同步 swipe_info 的 send_date 语义（当前 swipe 的展示时间）
        let mes_text = last
            .get("mes")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        state.user.save_chat(&avatar, &file_name, &chat, false)?;
        return Ok(json!({"swipe_id": new_id, "mes": mes_text}));
    }
    let mes_text = last
        .get("mes")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Ok(json!({"swipe_id": new_id, "mes": mes_text}))
}

fn worlds_list(state: SharedState) -> RpcResult {
    Ok(json!(state.user.list_worlds()?))
}

fn worlds_get(state: SharedState, params: Value) -> RpcResult {
    let name = param_str(&params, "name")?;
    let book: WorldInfoBook = state.user.read_world(&name)?;
    Ok(serde_json::to_value(book).map_err(|e| RpcError::Internal(e.to_string()))?)
}

fn worlds_save(state: SharedState, params: Value) -> RpcResult {
    let name = param_str(&params, "name")?;
    let book: WorldInfoBook = serde_json::from_value(
        params.get("book").cloned().unwrap_or(Value::Null),
    )
    .map_err(|e| RpcError::BadRequest(format!("invalid book: {e}")))?;
    state.user.save_world(&name, &book)?;
    Ok(json!({"ok": true}))
}

fn worlds_delete(state: SharedState, params: Value) -> RpcResult {
    let name = param_str(&params, "name")?;
    state.user.delete_world(&name)?;
    Ok(json!({"ok": true}))
}

fn groups_all(state: SharedState) -> RpcResult {
    let groups = state.user.list_groups()?;
    Ok(serde_json::to_value(groups).map_err(|e| RpcError::Internal(e.to_string()))?)
}

fn param_str<'a>(params: &'a Value, key: &str) -> Result<&'a str, RpcError> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| RpcError::BadRequest(format!("missing param {key}")))
}

// ---------- generate ----------

async fn generate_run(state: SharedState, params: Value) -> RpcResult {
    use crate::generate::{GenerateParams, GenerateSession};
    use nast_model::chat::GenerationType;

    let avatar = param_str(&params, "avatar")?.to_string();
    let chat_file = param_str(&params, "chat_file")?.to_string();
    let type_str = params
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("normal");
    let generation_type = match type_str {
        "normal" => GenerationType::Normal,
        "continue" => GenerationType::Continue,
        "impersonate" => GenerationType::Impersonate,
        "swipe" => GenerationType::Swipe,
        "regenerate" => GenerationType::Regenerate,
        "quiet" => GenerationType::Quiet,
        other => return Err(RpcError::BadRequest(format!("unknown type {other}"))),
    };
    let character = read_character(&state, &avatar)?;

    // 同一时刻一个生成：已有生成在跑则拒绝（ST is_send_press 语义）
    {
        let guard = state.generation.read().await;
        if guard.abort.is_some() && !guard.abort.as_ref().unwrap().is_cancelled() {
            return Err(RpcError::BadRequest("generation already in progress".into()));
        }
    }
    let abort = tokio_util::sync::CancellationToken::new();
    {
        let mut guard = state.generation.write().await;
        guard.abort = Some(abort.clone());
    }

    let oai: nast_model::preset::OaiSettings =
        serde_json::from_value(state.settings.read().await.get("oai_settings").cloned().unwrap_or(json!({})))
            .unwrap_or_default();
    let secrets_snapshot = state.secrets.read().await.clone();
    let provider = crate::connection::provider(&oai, &secrets_snapshot);

    let settings_snapshot = state.settings.read().await.clone();
    let session = GenerateSession {
        user: &state.user,
        hub: &state.hub,
        oai,
        settings_json: &settings_snapshot,
        provider,
        abort: abort.clone(),
        plugins: &state.plugins,
    };
    let p = GenerateParams {
        generation_type,
        avatar: avatar.clone(),
        chat_file: chat_file.clone(),
        user_message: params
            .get("user_message")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        character,
        is_group: false,
    };

    let result = session.run(p).await;
    // 清理生成状态
    {
        let mut guard = state.generation.write().await;
        guard.abort = None;
    }
    result
        .map(|r| json!({"text": r.text, "saved": r.saved}))
        .map_err(RpcError::Internal)
}

async fn generate_stop(state: SharedState) -> RpcResult {
    let guard = state.generation.read().await;
    if let Some(abort) = &guard.abort {
        abort.cancel();
    }
    state.hub.emit(events::GENERATION_STOPPED, json!({}));
    Ok(json!({"ok": true}))
}

#[allow(dead_code)]
fn unused(_: CardSpec, _: Uuid) {}
