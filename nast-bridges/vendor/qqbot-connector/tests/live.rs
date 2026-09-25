//! 真实端点冒烟测试（默认忽略，会访问 q.qq.com 并创建真实的绑定任务）。
//!
//! ```bash
//! cargo test --test live -- --ignored --nocapture
//! ```

use qqbot_connector::{
    BindStatus, Env, PollResult, build_connect_url, create_bind_task, poll_bind_result,
};

#[test]
#[ignore = "访问真实 q.qq.com 并创建绑定任务"]
fn live_create_and_poll() {
    let task = create_bind_task(Env::Production).expect("create_bind_task");
    println!("task_id = {}", task.task_id);

    // 32 字节的标准 base64 恰好是 44 字符且以单个 "=" 结尾。
    assert_eq!(task.key.len(), 44);
    assert!(task.key.ends_with('=') && !task.key.ends_with("=="));

    let url = build_connect_url(&task.task_id, "");
    println!("qr url  = {url}");
    assert!(url.starts_with("https://q.qq.com/qqbot/openclaw/connect.html?task_id="));

    // 轮询三次，仅验证响应结构与 retcode=0 路径；无人扫码，不应直接返回完成状态。
    for _ in 0..3 {
        let poll: PollResult =
            poll_bind_result(&task.task_id, Env::Production).expect("poll_bind_result");
        println!(
            "status = {:?}, appid = {:?}, has_secret = {}",
            poll.status,
            poll.bot_app_id,
            !poll.bot_encrypt_secret.is_empty()
        );
        assert_ne!(poll.status, BindStatus::Completed);
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}
