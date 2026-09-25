//! 凭据持久化（对应 Go 参考实现中的 FileStore）。

use std::fs;
use std::path::{Path, PathBuf};

use crate::connect::{ConnectOptions, Credentials, connect};
use crate::error::Result;

/// 基于本地 JSON 文件的凭据存储，文件格式与 [qqbot-go](https://github.com/libaibaia/qqbot-go) 兼容：
///
/// ```json
/// { "app_id": "...", "app_secret": "..." }
/// ```
#[derive(Clone, Debug)]
pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    /// 创建指向 `path` 的文件存储。
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// 文件路径。
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 读取凭据。文件不存在、JSON 中字段为空时返回 `Ok(None)`；其余错误透传。
    pub fn load(&self) -> Result<Option<Credentials>> {
        let data = match fs::read_to_string(&self.path) {
            Ok(data) => data,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        let creds: Credentials = serde_json::from_str(&data)?;
        if creds.app_id.is_empty() || creds.app_secret.is_empty() {
            return Ok(None);
        }
        Ok(Some(creds))
    }

    /// 保存凭据（Unix 下文件权限 0600）。
    pub fn save(&self, creds: &Credentials) -> Result<()> {
        let data = serde_json::to_string_pretty(creds)?;
        write_private(&self.path, data.as_bytes())?;
        Ok(())
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)?;
        // mode 仅影响新文件；先收紧已有文件权限，成功后再覆盖凭据。
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.set_len(0)?;
        file.write_all(bytes)
    }
    #[cfg(not(unix))]
    {
        fs::write(path, bytes)
    }
}

/// 优先从 `store` 读取凭据；读不到时执行 [`connect`] 扫码绑定并保存。
///
/// 注意：`options.display_qr_code_to_console` 按原样生效。
pub fn load_or_connect(store: &FileStore, options: &ConnectOptions) -> Result<Credentials> {
    if let Some(creds) = store.load()? {
        return Ok(creds);
    }
    let creds = connect(options)?;
    store.save(&creds)?;
    Ok(creds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("qqbot-connector-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("creds.json");

        let store = FileStore::new(&path);
        // 不存在 → None。
        assert!(store.load().unwrap().is_none());

        let creds = Credentials {
            app_id: "10001".into(),
            app_secret: "s3cret".into(),
            user_openid: None,
        };
        store.save(&creds).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        let loaded = store.load().unwrap().unwrap();
        assert_eq!(loaded, creds);

        // 字段为空 → 视为无凭据。
        fs::write(&path, r#"{"app_id":"","app_secret":""}"#).unwrap();
        assert!(store.load().unwrap().is_none());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn save_tightens_existing_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!(
            "qqbot-connector-permissions-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("creds.json");
        // 旧文件比新凭据长，同时检查覆盖时会清除尾部内容。
        fs::write(&path, vec![b' '; 1024]).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        let store = FileStore::new(&path);
        let creds = Credentials {
            app_id: "10001".into(),
            app_secret: "replacement-secret".into(),
            user_openid: None,
        };
        store.save(&creds).unwrap();

        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            serde_json::to_string_pretty(&creds).unwrap()
        );
        assert_eq!(store.load().unwrap(), Some(creds));
        fs::remove_dir_all(&dir).unwrap();
    }
}
