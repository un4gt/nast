//! 高层扫码绑定流程（对应参考实现中的 qr-connect.js / Connect）。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::crypto::decrypt_secret;
use crate::error::{Error, Result};
use crate::qr::display_qr_code;
use crate::session::{
    BindStatus, BindTask, Env, POLL_INTERVAL, PollResult, build_connect_url, create_bind_task,
    poll_bind_result,
};

/// 绑定成功后获得的机器人凭据。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credentials {
    /// 机器人 AppID。
    #[serde(rename = "app_id")]
    pub app_id: String,
    /// 机器人 AppSecret。
    #[serde(rename = "app_secret")]
    pub app_secret: String,
    /// 扫码用户的 openid（若有）。
    #[serde(
        rename = "user_openid",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub user_openid: Option<String>,
}

/// 协作式取消令牌：在另一个线程调用 [`CancelToken::cancel`]
/// 即可让正在阻塞轮询的 [`connect`] 尽快以 [`Error::Cancelled`] 返回。
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// 创建一个未取消的令牌。
    pub fn new() -> Self {
        Self::default()
    }

    /// 发起取消。
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// 是否已取消。
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// 扫码链接就绪回调类型。
pub type OnQrUrl = Box<dyn Fn(&str) + Send>;
/// 扫码状态变化回调类型。
pub type OnStatus = Box<dyn Fn(BindStatus, &str) + Send>;

/// [`connect`] 的行为配置。
pub struct ConnectOptions {
    /// API 环境，默认 [`Env::Production`]（二维码 URL 始终使用生产域名）。
    pub env: Env,
    /// 接入平台标识，拼入扫码 URL；留空则扫码页面显示"第三方机器人"。
    pub source: String,
    /// 是否在终端打印二维码，默认 `true`（与 npm 包一致；需要 `qr` feature，
    /// 未启用该 feature 时退化为打印链接本身）。
    pub display_qr_code_to_console: bool,
    /// 取消令牌，默认永不取消。
    pub cancel: CancelToken,
    /// 扫码链接就绪回调，在每轮轮询开始前触发（二维码刷新时也会再次触发）。
    pub on_qr_url: Option<OnQrUrl>,
    /// 扫码状态回调。每个任务的首次状态及后续状态变化时触发，避免每 2 秒重复。
    pub on_status: Option<OnStatus>,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        Self {
            env: Env::default(),
            source: String::new(),
            display_qr_code_to_console: true,
            cancel: CancelToken::default(),
            on_qr_url: None,
            on_status: None,
        }
    }
}

impl std::fmt::Debug for ConnectOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectOptions")
            .field("env", &self.env)
            .field("source", &self.source)
            .field(
                "display_qr_code_to_console",
                &self.display_qr_code_to_console,
            )
            .field("cancelled", &self.cancel.is_cancelled())
            .finish_non_exhaustive()
    }
}

impl ConnectOptions {
    /// 使用默认配置。
    pub fn new() -> Self {
        Self::default()
    }
}

/// 执行完整的扫码绑定流程，阻塞直到成功、出错或取消。
///
/// 二维码过期时自动重新生成任务并刷新（触发 [`ConnectOptions::on_qr_url`]）；
/// 轮询期间的瞬时网络错误会被吞掉并按 [`session::POLL_INTERVAL`](crate::session::POLL_INTERVAL)
/// 重试，与参考实现一致。
///
/// # 示例
///
/// ```no_run
/// use qqbot_connector::{connect, ConnectOptions, Credentials};
///
/// let creds: Credentials = connect(&ConnectOptions::new())?;
/// println!("AppID: {}", creds.app_id);
/// # Ok::<(), qqbot_connector::Error>(())
/// ```
pub fn connect(options: &ConnectOptions) -> Result<Credentials> {
    connect_with(
        options,
        create_bind_task,
        poll_bind_result,
        std::thread::sleep,
    )
}

fn connect_with(
    options: &ConnectOptions,
    mut create_task: impl FnMut(Env) -> Result<BindTask>,
    mut poll_task: impl FnMut(&str, Env) -> Result<PollResult>,
    mut sleep: impl FnMut(std::time::Duration),
) -> Result<Credentials> {
    loop {
        if options.cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }

        let task = create_task(options.env)
            .map_err(|e| Error::Network(format!("获取绑定任务失败: {e}")))?;

        let qr_url = build_connect_url(&task.task_id, &options.source);

        // 终端打印二维码（qr feature）+ 回调通知。
        if options.display_qr_code_to_console {
            display_qr_code(&qr_url)?;
            println!("\n请使用手机 QQ 扫描上方二维码，完成机器人绑定。\n");
        }
        if let Some(on_qr_url) = &options.on_qr_url {
            on_qr_url(&qr_url);
        }

        let mut last_status = None;
        loop {
            if options.cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }

            let poll = match poll_task(&task.task_id, options.env) {
                Ok(poll) => poll,
                // 瞬时网络错误：静默重试。
                Err(Error::Network(_) | Error::HttpStatus(_)) => {
                    sleep(POLL_INTERVAL);
                    continue;
                }
                Err(err) => return Err(err),
            };

            match poll.status {
                BindStatus::Completed => {
                    let app_secret = decrypt_secret(&task.key, &poll.bot_encrypt_secret)?;
                    if let Some(on_status) = &options.on_status {
                        on_status(BindStatus::Completed, "绑定成功！");
                    }
                    return Ok(Credentials {
                        app_id: poll.bot_app_id,
                        app_secret,
                        user_openid: poll.user_openid,
                    });
                }
                BindStatus::Expired => {
                    if let Some(on_status) = &options.on_status {
                        on_status(BindStatus::Expired, "二维码已过期，正在刷新...");
                    }
                    if options.display_qr_code_to_console {
                        println!("二维码已过期，正在刷新…\n");
                    }
                    break;
                }
                status => {
                    if Some(status) != last_status {
                        let msg = if status == BindStatus::Pending {
                            "已扫码，等待确认..."
                        } else {
                            "等待扫码..."
                        };
                        if let Some(on_status) = &options.on_status {
                            on_status(status, msg);
                        }
                        last_status = Some(status);
                    }
                }
            }

            sleep(POLL_INTERVAL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_token() {
        let token = CancelToken::new();
        assert!(!token.is_cancelled());
        token.cancel();
        assert!(token.is_cancelled());
        // Clone 共享状态。
        let clone = token.clone();
        assert!(clone.is_cancelled());
    }

    #[test]
    fn connect_cancelled_before_start() {
        let options = ConnectOptions::new();
        options.cancel.cancel();
        assert!(matches!(connect(&options), Err(Error::Cancelled)));
    }

    #[test]
    fn connect_notifies_waiting_after_refresh_and_skips_repeated_statuses() {
        use std::sync::Mutex;

        use aes_gcm::aead::{Aead, KeyInit};
        use aes_gcm::{Aes256Gcm, Nonce};

        #[derive(Debug, PartialEq, Eq)]
        enum Event {
            QrUrl(String),
            Status(BindStatus, String),
        }

        let events = Arc::new(Mutex::new(Vec::new()));
        let qr_events = Arc::clone(&events);
        let status_events = Arc::clone(&events);
        let options = ConnectOptions {
            display_qr_code_to_console: false,
            on_qr_url: Some(Box::new(move |url| {
                qr_events.lock().unwrap().push(Event::QrUrl(url.into()));
            })),
            on_status: Some(Box::new(move |status, message| {
                status_events
                    .lock()
                    .unwrap()
                    .push(Event::Status(status, message.into()));
            })),
            ..ConnectOptions::new()
        };

        let key_bytes = [7u8; 32];
        let iv = [11u8; 12];
        let cipher = Aes256Gcm::new((&key_bytes).into());
        let mut encrypted = iv.to_vec();
        encrypted.extend_from_slice(
            &cipher
                .encrypt(Nonce::from_slice(&iv), b"app-secret".as_ref())
                .unwrap(),
        );
        let key = crate::base64::encode(&key_bytes);
        let encrypted_secret = crate::base64::encode(&encrypted);

        let mut tasks = ["TASK1", "TASK2"].into_iter();
        let mut polls = [
            ("TASK1", BindStatus::None),
            ("TASK1", BindStatus::None),
            ("TASK1", BindStatus::Pending),
            ("TASK1", BindStatus::Pending),
            ("TASK1", BindStatus::Expired),
            ("TASK2", BindStatus::None),
            ("TASK2", BindStatus::None),
            ("TASK2", BindStatus::Pending),
            ("TASK2", BindStatus::Completed),
        ]
        .into_iter();

        let creds = connect_with(
            &options,
            |_| {
                Ok(BindTask {
                    task_id: tasks.next().expect("unexpected task creation").into(),
                    key: key.clone(),
                })
            },
            |task_id, _| {
                let (expected_task_id, status) = polls.next().expect("unexpected poll");
                assert_eq!(task_id, expected_task_id);
                Ok(PollResult {
                    status,
                    bot_app_id: "1234567890".into(),
                    bot_encrypt_secret: encrypted_secret.clone(),
                    user_openid: Some("openid".into()),
                })
            },
            |_| {},
        )
        .unwrap();

        assert!(tasks.next().is_none());
        assert!(polls.next().is_none());
        assert_eq!(
            creds,
            Credentials {
                app_id: "1234567890".into(),
                app_secret: "app-secret".into(),
                user_openid: Some("openid".into()),
            }
        );
        assert_eq!(
            *events.lock().unwrap(),
            vec![
                Event::QrUrl(build_connect_url("TASK1", "")),
                Event::Status(BindStatus::None, "等待扫码...".into()),
                Event::Status(BindStatus::Pending, "已扫码，等待确认...".into()),
                Event::Status(BindStatus::Expired, "二维码已过期，正在刷新...".into()),
                Event::QrUrl(build_connect_url("TASK2", "")),
                Event::Status(BindStatus::None, "等待扫码...".into()),
                Event::Status(BindStatus::Pending, "已扫码，等待确认...".into()),
                Event::Status(BindStatus::Completed, "绑定成功！".into()),
            ]
        );
    }

    #[test]
    fn credentials_serde_roundtrip() {
        let creds = Credentials {
            app_id: "123".into(),
            app_secret: "abc".into(),
            user_openid: Some("o1".into()),
        };
        let json = serde_json::to_string(&creds).unwrap();
        assert_eq!(
            json,
            r#"{"app_id":"123","app_secret":"abc","user_openid":"o1"}"#
        );
        assert_eq!(serde_json::from_str::<Credentials>(&json).unwrap(), creds);

        // 无 openid 时省略字段，与 qqbot-go 的文件格式兼容。
        let creds = Credentials {
            app_id: "123".into(),
            app_secret: "abc".into(),
            user_openid: None,
        };
        let json = serde_json::to_string(&creds).unwrap();
        assert_eq!(json, r#"{"app_id":"123","app_secret":"abc"}"#);
    }
}
