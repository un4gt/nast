//! nast 单端口服务入口：静态资源 + /ws + /upload。

mod connection;
mod auth;
mod model_catalog;
mod routing;
mod events;
mod generate;
mod group_gen;
mod group_sessions;
mod prompt_bridge;
mod rpc;
mod library;
mod state;
mod tts;
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

/// 角色头像缩略图：GET /thumbnail?file=<avatar>.png
/// ST 前端 <img src> 直接可用；v1 原样返回 PNG（浏览器端缩放）。
async fn thumbnail(
    req: actix_web::HttpRequest,
    state: web::Data<crate::state::SharedState>,
) -> impl Responder {
    use actix_web::http::header;
    let file = req
        .uri()
        .query()
        .and_then(|q| {
            q.split('&').find_map(|kv| {
                let (k, v) = kv.split_once('=')?;
                (k == "file").then(|| v.to_string())
            })
        })
        .unwrap_or_default()
        .trim()
        .to_string();
    // 百分号解码（encodeURIComponent 的中文文件名）
    let file = percent_decode(&file);
    // 防目录穿越：只允许文件名字符
    if file.is_empty() || file.contains("..") || file.contains('/') || file.contains('\\') {
        return HttpResponse::BadRequest().finish();
    }
    let path = state.user.character_dir().join(&file);
    match std::fs::read(&path) {
        Ok(bytes) => HttpResponse::Ok()
            .insert_header((header::CONTENT_TYPE, "image/png"))
            .insert_header((header::CACHE_CONTROL, "max-age=3600"))
            .body(bytes),
        Err(_) => HttpResponse::NotFound().finish(),
    }
}

/// Imported card resources only; settings, secrets, and chat files are never served.
async fn card_asset(
    path: web::Path<String>,
    state: web::Data<SharedState>,
) -> actix_web::Result<actix_files::NamedFile> {
    let relative = path.into_inner();
    let parts: Vec<_> = relative.split('/').collect();
    if parts.iter().any(|part| library::leaf(part).is_err()) {
        return Err(actix_web::error::ErrorBadRequest("invalid resource path"));
    }
    let base = if parts.first() == Some(&"characters") && parts.len() >= 3 {
        state.user.character_dir()
    } else if parts.starts_with(&["user", "images"]) && parts.len() >= 4 {
        state.user.root.join("user/images")
    } else {
        return Err(actix_web::error::ErrorNotFound("resource not found"));
    };
    let base = base.canonicalize().map_err(actix_web::error::ErrorNotFound)?;
    let target = state.user.root.join(relative).canonicalize().map_err(actix_web::error::ErrorNotFound)?;
    if !target.starts_with(&base) || !target.is_file() {
        return Err(actix_web::error::ErrorNotFound("resource not found"));
    }
    let extension = target.extension().and_then(|v| v.to_str()).unwrap_or("").to_ascii_lowercase();
    if !["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif", "mp3", "wav", "ogg", "mp4", "webm"].contains(&extension.as_str()) {
        return Err(actix_web::error::ErrorNotFound("unsupported resource"));
    }
    Ok(actix_files::NamedFile::open(target)?.use_last_modified(true))
}

/// %XX 百分号解码（UTF-8 字节序列重组，兼容 '+' 不处理——encodeURIComponent 不产生 '+'）。
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(hex) = std::str::from_utf8(&b[i + 1..i + 3]) {
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
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
    // 容器内需绑定 0.0.0.0；本机默认 127.0.0.1
    let bind_host: String = std::env::var("NAST_BIND").unwrap_or_else(|_| "127.0.0.1".into());
    let data_root: PathBuf = std::env::var("NAST_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./data"));
    let web_dist: PathBuf = std::env::var("NAST_WEB")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./web/dist"));

    let auth = web::Data::new(auth::Auth::from_env().map_err(std::io::Error::other)?);
    let user =
        nast_storage::UserData::new(&data_root, "default-user").expect("init user data");
    let mut settings = user
        .read_settings()
        .map_err(std::io::Error::other)?;
    nast_model::settings::normalize_settings(&mut settings);
    let secrets = user
        .read_secrets()
        .map_err(std::io::Error::other)?;
    let state: SharedState = std::sync::Arc::new(AppState::new(user, settings, secrets));

    // 预热 tokenizer（首次构建 o200k/cl100k BPE 约需数秒，避免拖慢首个请求）
    std::thread::spawn(|| {
        for model in ["gpt-4o", "gpt-4"] {
            let _ = nast_engine::tokens::count_tokens(
                "warmup",
                nast_engine::tokens::resolve_tokenizer(model),
            );
        }
    });

    tracing::info!("nast listening on http://{bind_host}:{port}");

    HttpServer::new(move || {
        let web_dist = web_dist.clone();
        App::new()
            .app_data(auth.clone())
            .app_data(web::Data::new(state.clone()))
            .wrap(actix_web::middleware::from_fn(auth::guard))
            .route("/healthz", web::get().to(|| async { HttpResponse::Ok().body("ok") }))
            .service(web::scope("/api/auth")
                .app_data(web::JsonConfig::default().limit(4096))
                .route("/session", web::get().to(auth::status))
                .route("/login", web::post().to(auth::login))
                .route("/logout", web::post().to(auth::logout)))
            .route("/ws", web::get().to(ws::ws_route))
            .route("/upload", web::post().to(upload))
            .route("/thumbnail", web::get().to(thumbnail))
            .route("/assets/{path:.*}", web::get().to(card_asset))
            .service(web::scope("/api/tts")
                .app_data(web::JsonConfig::default().limit(12 * 1024 * 1024))
                .route("/voices", web::post().to(tts::voices))
                .route("/models", web::post().to(tts::models))
                .route("/synthesize", web::post().to(tts::synthesize))
                .route("/voices/add", web::post().to(tts::add_voice))
                .route("/manage", web::post().to(tts::manage)))
            .service(Files::new("/", &web_dist).index_file("index.html"))
    })
    .bind((bind_host.as_str(), port))?
    .run()
    .await
}

#[cfg(test)]
mod tests {
    use super::percent_decode;

    #[test]
    fn percent_decode_utf8_filenames() {
        // encodeURIComponent("你的女仆妈妈由美.png")
        assert_eq!(
            percent_decode("%E4%BD%A0%E7%9A%84%E5%A5%B3%E4%BB%86%E5%A6%88%E5%A6%88%E7%94%B1%E7%BE%8E.png"),
            "你的女仆妈妈由美.png"
        );
        assert_eq!(percent_decode("plain.png"), "plain.png");
        assert_eq!(percent_decode("a%2Gb"), "a%2Gb"); // 非法十六进制保持原样
        assert_eq!(percent_decode("%20space%20"), " space ");
    }
}
