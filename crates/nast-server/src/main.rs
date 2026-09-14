//! nast 单端口服务入口：静态资源 + /ws + /upload。

mod events;
mod generate;
mod group_gen;
mod prompt_bridge;
mod rpc;
mod state;
mod ws;

use actix_files::Files;
use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use base64::Engine;
use state::{AppState, SharedState};
use std::path::PathBuf;

/// 文件上传（卡片导入等）：multipart 走 HTTP，返回 base64 列表，
/// 前端再经 WS 调 characters.import 完成落盘。
async fn upload(mut payload: actix_multipart::Multipart) -> impl Responder {
    use futures_util::StreamExt;
    let mut out: Vec<serde_json::Value> = Vec::new();
    while let Some(f) = payload.next().await {
        let Ok(mut f) = f else { continue };
        let mut bytes = Vec::new();
        while let Some(chunk) = f.next().await {
            match chunk {
                Ok(b) => bytes.extend_from_slice(&b),
                Err(_) => break,
            }
        }
        let name = f
            .content_disposition()
            .and_then(|cd| cd.get_filename().map(|s| s.to_string()))
            .unwrap_or_else(|| "file".into());
        out.push(serde_json::json!({
            "name": name,
            "data_base64": base64::engine::general_purpose::STANDARD.encode(&bytes),
        }));
    }
    HttpResponse::Ok().json(out)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let port: u16 = std::env::var("NAST_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8000);
    let data_root: PathBuf = std::env::var("NAST_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./data"));
    let web_dist: PathBuf = std::env::var("NAST_WEB")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./web/dist"));

    let user =
        nast_storage::UserData::new(&data_root, "default-user").expect("init user data");
    let settings = user
        .read_settings()
        .unwrap_or_else(|_| serde_json::json!({}));
    let state: SharedState = std::sync::Arc::new(AppState::new(user, settings));

    tracing::info!("nast listening on http://127.0.0.1:{port}");

    HttpServer::new(move || {
        let web_dist = web_dist.clone();
        App::new()
            .app_data(web::Data::new(state.clone()))
            .route("/ws", web::get().to(ws::ws_route))
            .route("/upload", web::post().to(upload))
            .service(Files::new("/", &web_dist).index_file("index.html"))
    })
    .bind(("127.0.0.1", port))?
    .run()
    .await
}
