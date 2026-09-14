//! data/<handle>/ 存储层：目录布局、聊天 jsonl、settings、世界书、群组、备份。
//!
//! 行为契约（对齐 refrence/SillyTavern/src/endpoints/chats.js、settings.js、
//! worldinfo.js、groups.js、users.js USER_DIRECTORY_TEMPLATE）：
//! - chats/<charWithoutPng>/<name>.jsonl：首行 header{user_name:'unused',character_name:'unused',chat_metadata}
//! - group chats/<chatId>.jsonl（字面空格）
//! - integrity slug 校验，mismatch 拒绝写（除非 force）
//! - 写入原子化（tmp + rename）；备份节流 10s，chat_<name>_<ts>.jsonl

use nast_model::chat::{ChatFile, ChatHeader};
use nast_model::group::Group;
use nast_model::world::WorldInfoBook;
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub const BACKUP_THROTTLE: Duration = Duration::from_secs(10);

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("integrity mismatch (use force to overwrite)")]
    IntegrityMismatch,
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid name: {0}")]
    InvalidName(String),
}

pub type StorageResult<T> = Result<T, StorageError>;

/// 单用户数据目录（默认 default-user）。
#[derive(Debug)]
pub struct UserData {
    pub root: PathBuf,
    last_chat_backup: Mutex<Option<Instant>>,
    last_settings_backup: Mutex<Option<Instant>>,
}

/// humanizedDateTime（RossAscends-mods.js:169）：
/// "2026-09-14@10h44m30s123ms" — 无冒号（Windows 文件名安全）。
pub fn humanized_date_time() -> String {
    let now = chrono::Local::now();
    format!(
        "{}-{}-{}@{}h{}m{}s{}ms",
        now.format("%Y"),
        now.format("%m"),
        now.format("%d"),
        now.format("%H"),
        now.format("%M"),
        now.format("%S"),
        now.format("%3f")
    )
}

/// getMessageTimeStamp：ISO8601 UTC（RossAscends-mods.js）。
pub fn message_time_stamp() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

impl UserData {
    pub fn new(data_root: &Path, handle: &str) -> StorageResult<Self> {
        let root = data_root.join(handle);
        for dir in [
            "",
            "characters",
            "chats",
            "group chats",
            "groups",
            "worlds",
            "backups",
            "settings",
            "user",
        ] {
            fs::create_dir_all(root.join(dir))?;
        }
        Ok(Self {
            root,
            last_chat_backup: Mutex::new(None),
            last_settings_backup: Mutex::new(None),
        })
    }

    // ---------- characters ----------

    pub fn character_dir(&self) -> PathBuf {
        self.root.join("characters")
    }

    /// 列出全部角色 PNG 文件名。
    pub fn list_characters(&self) -> StorageResult<Vec<String>> {
        let mut out = Vec::new();
        for entry in fs::read_dir(self.character_dir())? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.to_ascii_lowercase().ends_with(".png") {
                out.push(name);
            }
        }
        out.sort();
        Ok(out)
    }

    /// 角色头像文件名 → 聊天目录名（去 .png）。
    pub fn chat_dir_for_character(&self, avatar: &str) -> PathBuf {
        let dir_name = avatar.strip_suffix(".png").unwrap_or(avatar);
        self.root.join("chats").join(dir_name)
    }

    /// 名字冲突时找 " N" 后缀（characters.js）。
    pub fn unique_character_file(&self, name: &str) -> String {
        let base = format!("{}.png", nast_cards::sanitize_filename(name));
        let mut candidate = base.clone();
        let mut i = 0;
        while self.character_dir().join(&candidate).exists() {
            i += 1;
            candidate = format!("{} {}.png", base.trim_end_matches(".png"), i);
        }
        candidate
    }

    // ---------- chats ----------

    pub fn list_chats(&self, avatar: &str) -> StorageResult<Vec<String>> {
        let dir = self.chat_dir_for_character(avatar);
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut out = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".jsonl") {
                out.push(name);
            }
        }
        out.sort();
        Ok(out)
    }

    pub fn read_chat(&self, avatar: &str, file_name: &str) -> StorageResult<ChatFile> {
        let path = self.chat_dir_for_character(avatar).join(file_name);
        let raw = fs::read_to_string(&path)
            .map_err(|_| StorageError::NotFound(path.display().to_string()))?;
        let mut values = Vec::new();
        for line in raw.lines().filter(|l| !l.trim().is_empty()) {
            values.push(serde_json::from_str::<Value>(line)?);
        }
        Ok(ChatFile(values))
    }

    /// 保存聊天：integrity 校验 + 备份节流 + 原子写。
    pub fn save_chat(
        &self,
        avatar: &str,
        file_name: &str,
        chat: &ChatFile,
        force: bool,
    ) -> StorageResult<()> {
        if chat.header().is_none() {
            return Err(StorageError::InvalidName("missing header line".into()));
        }
        let dir = self.chat_dir_for_character(avatar);
        fs::create_dir_all(&dir)?;
        let path = dir.join(file_name);
        // integrity：现有文件的 metadata.integrity ≠ 新的 → 拒绝
        if !force && path.exists() {
            let existing = self.read_chat(avatar, file_name)?;
            let existing_slug = existing.metadata().integrity;
            let new_slug = chat.metadata().integrity;
            if existing_slug.is_some() && existing_slug != new_slug {
                return Err(StorageError::IntegrityMismatch);
            }
        }
        // 备份（节流）
        if path.exists() {
            self.backup_chat_throttled(avatar, file_name, &path)?;
        }
        let mut serialized = String::new();
        for v in &chat.0 {
            serialized.push_str(&serde_json::to_string(v)?);
            serialized.push('\n');
        }
        atomic_write(&path, serialized.as_bytes())?;
        Ok(())
    }

    fn backup_chat_throttled(&self, avatar: &str, file_name: &str, path: &Path) -> StorageResult<()> {
        {
            let mut last = self.last_chat_backup.lock().unwrap();
            if let Some(t) = *last {
                if t.elapsed() < BACKUP_THROTTLE {
                    return Ok(());
                }
            }
            *last = Some(Instant::now());
        }
        let slug: String = format!("{}_{}", avatar, file_name)
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect::<String>()
            .to_lowercase();
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let dest = self.root.join("backups").join(format!("chat_{slug}_{ts}.jsonl"));
        fs::copy(path, dest)?;
        Ok(())
    }

    pub fn delete_chat(&self, avatar: &str, file_name: &str) -> StorageResult<()> {
        let path = self.chat_dir_for_character(avatar).join(file_name);
        fs::remove_file(path).map_err(|_| StorageError::NotFound(file_name.into()))
    }

    pub fn rename_chat(&self, avatar: &str, original: &str, renamed: &str) -> StorageResult<String> {
        let dir = self.chat_dir_for_character(avatar);
        let sanitized = sanitize_chat_name(renamed);
        let dest = dir.join(format!("{sanitized}.jsonl"));
        fs::rename(dir.join(original), &dest)
            .map_err(|_| StorageError::NotFound(original.into()))?;
        Ok(sanitized)
    }

    // ---------- settings ----------

    pub fn settings_path(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    pub fn read_settings(&self) -> StorageResult<Value> {
        let p = self.settings_path();
        if !p.exists() {
            return Ok(serde_json::json!({}));
        }
        Ok(serde_json::from_str(&fs::read_to_string(&p)?)?)
    }

    pub fn save_settings(&self, value: &Value) -> StorageResult<()> {
        if self.settings_path().exists() {
            let mut last = self.last_settings_backup.lock().unwrap();
            let should = match *last {
                Some(t) => t.elapsed() >= Duration::from_secs(600),
                None => true,
            };
            if should {
                *last = Some(Instant::now());
                drop(last);
                let ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let dest = self.root.join("backups").join(format!("settings_{ts}.json"));
                fs::copy(self.settings_path(), dest)?;
            }
        }
        let pretty = serde_json::to_string_pretty(value)?;
        atomic_write(&self.settings_path(), pretty.as_bytes())?;
        Ok(())
    }

    // ---------- worlds ----------

    pub fn list_worlds(&self) -> StorageResult<Vec<String>> {
        let dir = self.root.join("worlds");
        let mut out = Vec::new();
        for entry in fs::read_dir(dir)? {
            let name = entry?.file_name().to_string_lossy().to_string();
            if name.ends_with(".json") {
                out.push(name.trim_end_matches(".json").to_string());
            }
        }
        out.sort();
        Ok(out)
    }

    pub fn read_world(&self, name: &str) -> StorageResult<WorldInfoBook> {
        let path = self.root.join("worlds").join(format!("{name}.json"));
        let raw = fs::read_to_string(&path)
            .map_err(|_| StorageError::NotFound(format!("world {name}")))?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn save_world(&self, name: &str, book: &WorldInfoBook) -> StorageResult<()> {
        let path = self
            .root
            .join("worlds")
            .join(format!("{}.json", sanitize_world_name(name)));
        let pretty = serde_json::to_string_pretty(book)?;
        atomic_write(&path, pretty.as_bytes())?;
        Ok(())
    }

    pub fn delete_world(&self, name: &str) -> StorageResult<()> {
        fs::remove_file(self.root.join("worlds").join(format!("{name}.json")))
            .map_err(|_| StorageError::NotFound(format!("world {name}")))
    }

    // ---------- groups ----------

    pub fn list_groups(&self) -> StorageResult<Vec<Group>> {
        let dir = self.root.join("groups");
        let mut out: Vec<Group> = Vec::new();
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let raw = fs::read_to_string(&path)?;
                out.push(serde_json::from_str(&raw)?);
            }
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    pub fn save_group(&self, group: &Group) -> StorageResult<()> {
        let path = self.root.join("groups").join(format!("{}.json", group.id));
        let pretty = serde_json::to_string_pretty(group)?;
        atomic_write(&path, pretty.as_bytes())
    }

    pub fn delete_group(&self, id: &str) -> StorageResult<()> {
        fs::remove_file(self.root.join("groups").join(format!("{id}.json")))
            .map_err(|_| StorageError::NotFound(format!("group {id}")))
    }

    /// 群聊天文件在 "group chats/" 平铺。
    pub fn group_chat_path(&self, chat_id: &str) -> PathBuf {
        self.root.join("group chats").join(format!("{chat_id}.jsonl"))
    }

    pub fn save_group_chat(&self, chat_id: &str, chat: &ChatFile, force: bool) -> StorageResult<()> {
        let path = self.group_chat_path(chat_id);
        if !force && path.exists() {
            let raw = fs::read_to_string(&path)?;
            if let Ok(existing) = serde_json::from_str::<ChatHeader>(raw.lines().next().unwrap_or(""))
            {
                if existing.chat_metadata.integrity.is_some()
                    && existing.chat_metadata.integrity != chat.metadata().integrity
                {
                    return Err(StorageError::IntegrityMismatch);
                }
            }
        }
        fs::create_dir_all(path.parent().unwrap())?;
        let mut serialized = String::new();
        for v in &chat.0 {
            serialized.push_str(&serde_json::to_string(v)?);
            serialized.push('\n');
        }
        atomic_write(&path, serialized.as_bytes())
    }

    pub fn read_group_chat(&self, chat_id: &str) -> StorageResult<ChatFile> {
        let path = self.group_chat_path(chat_id);
        let raw = fs::read_to_string(&path)
            .map_err(|_| StorageError::NotFound(path.display().to_string()))?;
        let mut values = Vec::new();
        for line in raw.lines().filter(|l| !l.trim().is_empty()) {
            values.push(serde_json::from_str::<Value>(line)?);
        }
        Ok(ChatFile(values))
    }
}

fn sanitize_chat_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || " ._-()".contains(c) { c } else { '_' })
        .collect();
    cleaned.trim().to_string()
}

fn sanitize_world_name(name: &str) -> String {
    name.trim().to_string()
}

/// 原子写：同目录 tmp 文件 + rename。
pub fn atomic_write(path: &Path, data: &[u8]) -> StorageResult<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_user() -> (tempfile::TempDir, UserData) {
        let dir = tempfile::tempdir().unwrap();
        let ud = UserData::new(dir.path(), "default-user").unwrap();
        (dir, ud)
    }

    fn sample_chat() -> ChatFile {
        let header = serde_json::json!({
            "user_name": "unused",
            "character_name": "unused",
            "chat_metadata": {"integrity": "abc123", "world": "Eldoria"}
        });
        let msg = serde_json::json!({
            "name": "Seraphina", "is_user": false, "is_system": false,
            "send_date": "2026-09-14T08:00:00.000Z", "mes": "hello"
        });
        ChatFile(vec![header, msg])
    }

    #[test]
    fn chat_roundtrip_and_integrity() {
        let (_d, ud) = temp_user();
        ud.save_chat("Seraphina.png", "chat1.jsonl", &sample_chat(), false).unwrap();
        let read = ud.read_chat("Seraphina.png", "chat1.jsonl").unwrap();
        assert_eq!(read.metadata().integrity.as_deref(), Some("abc123"));
        assert_eq!(read.messages().count(), 1);
        // integrity 不匹配 → 拒绝
        let mut tampered = sample_chat();
        tampered.0[0]["chat_metadata"]["integrity"] = serde_json::json!("DIFFERENT");
        assert!(matches!(
            ud.save_chat("Seraphina.png", "chat1.jsonl", &tampered, false),
            Err(StorageError::IntegrityMismatch)
        ));
        // force 覆盖成功
        ud.save_chat("Seraphina.png", "chat1.jsonl", &tampered, true).unwrap();
    }

    #[test]
    fn list_chats_sorted() {
        let (_d, ud) = temp_user();
        ud.save_chat("Seraphina.png", "b.jsonl", &sample_chat(), false).unwrap();
        ud.save_chat("Seraphina.png", "a.jsonl", &sample_chat(), false).unwrap();
        let chats = ud.list_chats("Seraphina.png").unwrap();
        assert_eq!(chats, vec!["a.jsonl", "b.jsonl"]);
    }

    #[test]
    fn settings_roundtrip() {
        let (_d, ud) = temp_user();
        ud.save_settings(&serde_json::json!({"v": 2})).unwrap();
        assert_eq!(ud.read_settings().unwrap()["v"], 2);
    }

    #[test]
    fn world_roundtrip() {
        let (_d, ud) = temp_user();
        let book = WorldInfoBook::default();
        ud.save_world("Eldoria", &book).unwrap();
        assert_eq!(ud.list_worlds().unwrap(), vec!["Eldoria"]);
        ud.read_world("Eldoria").unwrap();
        ud.delete_world("Eldoria").unwrap();
    }

    #[test]
    fn group_roundtrip() {
        let (_d, ud) = temp_user();
        let mut g = Group::default();
        g.name = "Test Group".into();
        ud.save_group(&g).unwrap();
        assert_eq!(ud.list_groups().unwrap()[0].name, "Test Group");
        ud.delete_group(&g.id).unwrap();
    }

    #[test]
    fn humanized_format() {
        let s = humanized_date_time();
        // 2026-09-14@10h44m30s123ms 形态（毫秒尾零可被 %3f 裁剪），无冒号
        assert!(s.starts_with("20") && s.contains('@'));
        assert!(s.contains('h') && s.contains('m') && s.contains('s') && s.ends_with("ms"));
        assert!(!s.contains(':'));
    }

    #[test]
    fn unique_character_file_collides() {
        let (_d, ud) = temp_user();
        // 名字未占用 → 直接 base 名
        assert_eq!(ud.unique_character_file("Seraphina"), "Seraphina.png");
        // 预占 base → 顺延后缀
        std::fs::write(ud.character_dir().join("Seraphina.png"), b"x").unwrap();
        assert_eq!(ud.unique_character_file("Seraphina"), "Seraphina 1.png");
        std::fs::write(ud.character_dir().join("Seraphina 1.png"), b"x").unwrap();
        assert_eq!(ud.unique_character_file("Seraphina"), "Seraphina 2.png");
    }
}
