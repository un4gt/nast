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
        "characters.all" => characters_all(state),
        "characters.get" => characters_get(state, params),
        "characters.import" => characters_import(state, params),
        "characters.delete" => characters_delete(state, params),
        "characters.chats" => characters_chats(state, params),
        "chats.get" => chats_get(state, params),
        "chats.save" => chats_save(state, params),
        "chats.delete" => chats_delete(state, params),
        "chats.rename" => chats_rename(state, params),
        "generate.run" => generate_run(state, params).await,
        "generate.stop" => generate_stop(state).await,
        "groups.create" => groups_create(state, params),
        "groups.edit" => groups_edit(state, params),
        "groups.delete" => groups_delete(state, params),
        "groups.get" => groups_get(state),
        "groups.chats" => groups_chats(state, params),
        "generate.group" => crate::group_gen::generate_group(state, params).await,
        "chats.swipe" => chats_swipe(state, params),
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
    let out = nast_cards::write_card(&png, &v2_json, None)
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

fn characters_chats(state: SharedState, params: Value) -> RpcResult {
    let avatar = param_str(&params, "avatar")?;
    let chats = state.user.list_chats(&avatar)?;
    Ok(json!(chats))
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
    let provider = nast_providers::Provider::new(match oai.chat_completion_source.as_str() {
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
    });

    let settings_snapshot = state.settings.read().await.clone();
    let session = GenerateSession {
        user: &state.user,
        hub: &state.hub,
        oai,
        settings_json: &settings_snapshot,
        provider,
        abort: abort.clone(),
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
        persona_description: params
            .get("persona_description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        persona_position_in_prompt: params
            .get("persona_position_in_prompt")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
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
