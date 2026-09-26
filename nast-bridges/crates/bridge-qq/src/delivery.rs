//! Durable pending text: only an acknowledged QQ send advances the queue.
use std::{collections::{BTreeMap, VecDeque}, io::Write, path::PathBuf};

pub const MORE_HINT: &str = "\n（还有后续内容，发送 /more 继续查看；不会重新生成。）";

pub fn split_text(text: &str, max_chars: usize) -> VecDeque<String> {
    let limit = max_chars.clamp(128, 1500).saturating_sub(MORE_HINT.chars().count());
    let mut rest = text;
    let mut chunks = VecDeque::new();
    while !rest.is_empty() {
        let end = rest.char_indices().nth(limit).map(|(i,_)|i).unwrap_or(rest.len());
        let candidate = &rest[..end];
        let split = if end < rest.len() {
            candidate.rfind('\n').filter(|i| *i >= end/2).map(|i|i+1).unwrap_or(end)
        } else { end };
        chunks.push_back(rest[..split].to_string());
        rest = &rest[split..];
    }
    chunks
}

pub struct Outbox {
    path: PathBuf,
    pending: BTreeMap<String, VecDeque<String>>,
}
impl Outbox {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let pending = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_|"QQ 待发送记录损坏，请备份并检查 outbox 文件".to_string())?,
            Err(e) if e.kind()==std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(format!("无法读取 QQ 待发送记录：{e}")),
        };
        Ok(Self { path, pending })
    }
    fn save(&self, pending: &BTreeMap<String,VecDeque<String>>) -> Result<(),String> {
        let parent = self.path.parent().filter(|p|!p.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
        std::fs::create_dir_all(parent).map_err(|e|e.to_string())?;
        let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e|e.to_string())?;
        file.write_all(&serde_json::to_vec(pending).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        file.as_file().sync_all().map_err(|e|e.to_string())?;
        file.persist(&self.path).map_err(|e|e.to_string())?;
        Ok(())
    }
    pub fn enqueue(&mut self, source: &str, text: &str, limit: usize) -> Result<(),String> {
        let mut next = self.pending.clone();
        next.entry(source.into()).or_default().extend(split_text(text,limit));
        self.save(&next)?;
        self.pending=next;
        Ok(())
    }
    pub fn front(&self, source: &str, last_allowed: bool) -> Option<String> {
        let queue = self.pending.get(source)?;
        let mut text = queue.front()?.clone();
        if last_allowed && queue.len()>1 { text.push_str(MORE_HINT); }
        Some(text)
    }
    pub fn acknowledge(&mut self, source: &str) -> Result<(),String> {
        let mut next = self.pending.clone();
        if let Some(queue) = next.get_mut(source) { queue.pop_front(); if queue.is_empty() { next.remove(source); } }
        self.save(&next)?;
        self.pending=next;
        Ok(())
    }
    pub fn len(&self, source: &str) -> usize { self.pending.get(source).map_or(0,VecDeque::len) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn split_preserves_all_unicode_and_newlines() {
        let text = "中文🙂👩‍💻 paragraph\n".repeat(1000);
        for limit in [0,128,1500,10000] {
            let chunks=split_text(&text,limit);
            assert_eq!(chunks.iter().cloned().collect::<String>(), text);
            assert!(chunks.iter().all(|s|s.chars().count()+MORE_HINT.chars().count()<=limit.clamp(128,1500)));
        }
    }
    #[test]
    fn delivery_failure_and_restart_preserve_unsent_tail() {
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("outbox.json");
        let text="完整回复🙂\n".repeat(1600);
        let mut box_=Outbox::open(path.clone()).unwrap();box_.enqueue("source",&text,1500).unwrap();
        let mut sent=String::new();
        // Four C2C replies, with the last one inviting a manual continuation.
        for seq in 1..=4 {
            let preview=box_.front("source",seq==4).unwrap();
            if seq==4 { assert!(preview.ends_with(MORE_HINT)); }
            sent.push_str(preview.strip_suffix(MORE_HINT).unwrap_or(&preview));
            box_.acknowledge("source").unwrap();
        }
        let failed=box_.front("source",false).unwrap();
        drop(box_);
        let mut box_=Outbox::open(path.clone()).unwrap();
        assert_eq!(box_.front("source",false).unwrap(),failed);
        while let Some(part)=box_.front("source",false) { sent.push_str(&part);box_.acknowledge("source").unwrap(); }
        assert_eq!(sent,text);assert_eq!(box_.len("source"),0);
        std::fs::write(path,b"invalid").unwrap();assert!(Outbox::open(dir.path().join("outbox.json")).is_err());
    }
}
