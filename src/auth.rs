//! Deployment authentication: browser sessions and a separate bridge bearer token.
use actix_web::{
    Error, HttpMessage, HttpRequest, HttpResponse,
    body::EitherBody,
    cookie::{Cookie, SameSite, time::Duration as CookieDuration},
    dev::{ServiceRequest, ServiceResponse},
    http::{Method, header},
    middleware::Next,
    web,
};
use rand::RngCore;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use tokio_util::sync::CancellationToken;

const COOKIE: &str = "nast_session";
const SESSION_TTL: Duration = Duration::from_secs(12 * 60 * 60);

pub struct Auth {
    username: Option<String>,
    password: [u8; 32],
    bridge_token: Option<[u8; 32]>,
    public_origin: Option<String>,
    secure_cookie: bool,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    attempts: Mutex<HashMap<String, (Instant, u32)>>,
}
pub struct Session {
    expires: Instant,
    cancelled: CancellationToken,
}
#[derive(Clone)]
pub enum Grant {
    Anonymous,
    Bridge,
    Browser(Arc<Session>),
}
impl Grant {
    pub fn valid(&self) -> bool {
        match self {
            Self::Browser(s) => s.expires > Instant::now() && !s.cancelled.is_cancelled(),
            _ => true,
        }
    }
    pub async fn revoked(&self) {
        if let Self::Browser(s) = self {
            tokio::select! {
                _ = s.cancelled.cancelled() => {},
                _ = tokio::time::sleep_until(s.expires.into()) => {},
            }
        } else {
            std::future::pending::<()>().await;
        }
    }
}
fn digest(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}
fn matches(value: &str, expected: &[u8; 32]) -> bool {
    bool::from(digest(value).ct_eq(expected))
}
impl Auth {
    pub fn from_env() -> Result<Self, String> {
        Self::configure(|key| std::env::var(key).unwrap_or_default())
    }
    fn configure(get: impl Fn(&str) -> String) -> Result<Self, String> {
        let username = get("NAST_USERNAME");
        let password = get("NAST_PASSWORD");
        let bridge = get("NAST_BRIDGE_TOKEN");
        let anonymous = get("NAST_ALLOW_ANONYMOUS") == "true";
        if username.is_empty() != password.is_empty() {
            return Err("NAST_USERNAME 和 NAST_PASSWORD 必须同时设置".into());
        }
        if (!username.is_empty() && username.trim().is_empty())
            || username.len() > 256
            || password.len() > 1024
        {
            return Err("用户名须为 1–256 字节非空白文本，密码最多 1024 字节".into());
        }
        if username.is_empty() && !anonymous {
            return Err("请设置 NAST_USERNAME / NAST_PASSWORD；仅本机开发可显式设置 NAST_ALLOW_ANONYMOUS=true".into());
        }
        if !bridge.is_empty()
            && (bridge.len() < 32
                || !bridge
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.~".contains(&b)))
        {
            return Err("NAST_BRIDGE_TOKEN 须为至少 32 字符的字母、数字或 -_.~".into());
        }
        let origin = get("NAST_PUBLIC_ORIGIN");
        let public_origin = if origin.is_empty() {
            None
        } else {
            let url = reqwest::Url::parse(&origin).map_err(|_| "无效 NAST_PUBLIC_ORIGIN")?;
            if !["http", "https"].contains(&url.scheme())
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || url.path() != "/"
            {
                return Err(
                    "NAST_PUBLIC_ORIGIN 须为完整站点源，例如 https://chat.example.com".into(),
                );
            }
            Some(url.origin().ascii_serialization())
        };
        Ok(Self {
            username: (!username.is_empty()).then_some(username),
            password: digest(&password),
            bridge_token: (!bridge.is_empty()).then(|| digest(&bridge)),
            public_origin,
            secure_cookie: get("NAST_COOKIE_SECURE") == "true",
            sessions: Mutex::new(HashMap::new()),
            attempts: Mutex::new(HashMap::new()),
        })
    }
    fn origin_allowed(&self, req: &HttpRequest) -> bool {
        let Some(origin) = req.headers().get(header::ORIGIN) else {
            return true;
        };
        let Ok(origin) = origin.to_str() else {
            return false;
        };
        let Ok(url) = reqwest::Url::parse(origin) else {
            return false;
        };
        if let Some(expected) = &self.public_origin {
            return url.origin().ascii_serialization() == *expected;
        }
        let host = req
            .headers()
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        ["http", "https"].contains(&url.scheme())
            && url.origin().ascii_serialization() == format!("{}://{host}", url.scheme())
    }
    fn grant(&self, req: &HttpRequest) -> Option<Grant> {
        if self.username.is_none() {
            return Some(Grant::Anonymous);
        }
        if let Some(value) = req.headers().get(header::AUTHORIZATION) {
            let token = value.to_str().ok()?.strip_prefix("Bearer ")?;
            return self
                .bridge_token
                .as_ref()
                .filter(|expected| matches(token, expected))
                .map(|_| Grant::Bridge);
        }
        let cookie = req.cookie(COOKIE)?;
        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|_, s| s.expires > Instant::now() && !s.cancelled.is_cancelled());
        sessions.get(cookie.value()).cloned().map(Grant::Browser)
    }
    fn secure(&self, req: &HttpRequest) -> bool {
        self.secure_cookie
            || self
                .public_origin
                .as_ref()
                .is_some_and(|v| v.starts_with("https://"))
            || req
                .headers()
                .get(header::ORIGIN)
                .is_some_and(|v| v.as_bytes().starts_with(b"https://"))
    }
    fn cookie(&self, value: String, req: &HttpRequest, seconds: i64) -> Cookie<'static> {
        Cookie::build(COOKIE, value)
            .path("/")
            .http_only(true)
            .same_site(SameSite::Strict)
            .secure(self.secure(req))
            .max_age(CookieDuration::seconds(seconds))
            .finish()
    }
    fn allow_attempt(&self, req: &HttpRequest) -> bool {
        // Use the actual peer, never client-controlled forwarded headers.
        let peer = req
            .peer_addr()
            .map(|a| a.ip().to_string())
            .unwrap_or_default();
        let mut attempts = self.attempts.lock().unwrap();
        attempts.retain(|_, (start, _)| start.elapsed() < Duration::from_secs(300));
        if attempts.len() >= 1024 && !attempts.contains_key(&peer) {
            return false;
        }
        let entry = attempts.entry(peer).or_insert((Instant::now(), 0));
        entry.1 += 1;
        entry.1 <= 20
    }
}
fn public(req: &HttpRequest) -> bool {
    let path = req.path();
    (matches!(*req.method(), Method::GET | Method::HEAD)
        && (matches!(
            path,
            "/" | "/index.html"
                | "/healthz"
                | "/api/auth/session"
                | "/favicon.svg"
                | "/favicon.ico"
                | "/favicon-32.png"
                | "/apple-touch-icon.png"
        ) || path.starts_with("/static/")))
        || (req.method() == Method::POST && path == "/api/auth/login")
}
pub async fn guard<B: actix_web::body::MessageBody + 'static>(
    req: ServiceRequest,
    next: Next<B>,
) -> Result<ServiceResponse<EitherBody<B>>, Error> {
    let auth = req.app_data::<web::Data<Auth>>().expect("auth state");
    let is_public = public(req.request());
    let grant = auth.grant(req.request());
    let browser_ws = req.path() == "/ws" && matches!(grant, Some(Grant::Browser(_)));
    if !auth.origin_allowed(req.request())
        || (browser_ws && !req.headers().contains_key(header::ORIGIN))
    {
        return Ok(req
            .into_response(HttpResponse::Forbidden().json(json!({"error":"不允许跨站访问"})))
            .map_into_right_body());
    }
    if !is_public && grant.is_none() {
        return Ok(req
            .into_response(
                HttpResponse::Unauthorized()
                    .insert_header((header::CACHE_CONTROL, "no-store"))
                    .json(json!({"error":"请先登录"})),
            )
            .map_into_right_body());
    }
    if let Some(grant) = grant {
        req.extensions_mut().insert(grant);
    }
    let mut response = next.call(req).await?.map_into_left_body();
    if !is_public || response.request().path().starts_with("/api/auth/") {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-store"),
        );
    }
    Ok(response)
}
pub async fn status(auth: web::Data<Auth>, req: HttpRequest) -> HttpResponse {
    let logged_in = matches!(auth.grant(&req), Some(Grant::Browser(_) | Grant::Anonymous));
    HttpResponse::Ok().json(json!({"enabled":auth.username.is_some(),"authenticated":logged_in,"username":if logged_in {auth.username.as_deref()} else {None}}))
}
#[derive(Deserialize)]
pub struct Login {
    username: String,
    password: String,
}
fn requested(req: &HttpRequest) -> bool {
    req.headers()
        .get("x-nast-request")
        .is_some_and(|v| v == "1")
}
pub async fn login(
    auth: web::Data<Auth>,
    req: HttpRequest,
    input: web::Json<Login>,
) -> HttpResponse {
    if !requested(&req) {
        return HttpResponse::Forbidden().finish();
    }
    if !auth.allow_attempt(&req) {
        return HttpResponse::TooManyRequests()
            .insert_header((header::RETRY_AFTER, "300"))
            .json(json!({"error":"尝试过于频繁，请五分钟后重试"}));
    }
    let user_ok = auth
        .username
        .as_ref()
        .is_some_and(|u| matches(&input.username, &digest(u)));
    let password_ok = matches(&input.password, &auth.password);
    if !(user_ok & password_ok) {
        return HttpResponse::Unauthorized().json(json!({"error":"用户名或密码不正确"}));
    }
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    use base64::Engine;
    let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let mut sessions = auth.sessions.lock().unwrap();
    sessions.retain(|_, s| s.expires > Instant::now() && !s.cancelled.is_cancelled());
    if let Some(old) = req.cookie(COOKIE).and_then(|c| sessions.remove(c.value())) {
        old.cancelled.cancel();
    }
    if sessions.len() >= 256 {
        return HttpResponse::TooManyRequests()
            .json(json!({"error":"登录会话过多，请退出其他会话后重试"}));
    }
    sessions.insert(
        token.clone(),
        Arc::new(Session {
            expires: Instant::now() + SESSION_TTL,
            cancelled: CancellationToken::new(),
        }),
    );
    HttpResponse::Ok()
        .cookie(auth.cookie(token, &req, SESSION_TTL.as_secs() as i64))
        .json(json!({"ok":true}))
}
pub async fn logout(auth: web::Data<Auth>, req: HttpRequest) -> HttpResponse {
    if !requested(&req) {
        return HttpResponse::Forbidden().finish();
    }
    if let Some(cookie) = req.cookie(COOKIE) {
        if let Some(s) = auth.sessions.lock().unwrap().remove(cookie.value()) {
            s.cancelled.cancel();
        }
    }
    HttpResponse::Ok()
        .cookie(auth.cookie(String::new(), &req, 0))
        .json(json!({"ok":true}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{App, test};
    fn config() -> Auth {
        Auth::configure(|k| {
            match k {
                "NAST_USERNAME" => "admin",
                "NAST_PASSWORD" => "test-password",
                "NAST_BRIDGE_TOKEN" => "0123456789abcdef0123456789abcdef",
                _ => "",
            }
            .into()
        })
        .unwrap()
    }
    #[actix_web::test]
    async fn protects_http_and_ws_and_revokes_cookie() {
        let auth = web::Data::new(config());
        let app = test::init_service(
            App::new()
                .app_data(auth.clone())
                .wrap(actix_web::middleware::from_fn(guard))
                .route("/api/auth/login", web::post().to(login))
                .route("/api/auth/logout", web::post().to(logout))
                .default_service(web::to(|| async { HttpResponse::Ok().finish() })),
        )
        .await;
        for path in [
            "/ws",
            "/upload",
            "/thumbnail",
            "/assets/characters/a/a.png",
            "/api/tts/synthesize",
            "/future-api",
        ] {
            let res =
                test::call_service(&app, test::TestRequest::get().uri(path).to_request()).await;
            assert_eq!(res.status(), 401, "{path}");
        }
        let res = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/api/auth/login")
                .insert_header(("x-nast-request", "1"))
                .set_json(json!({"username":"admin","password":"test-password"}))
                .to_request(),
        )
        .await;
        assert_eq!(res.status(), 200);
        let cookie = res.response().cookies().next().unwrap().into_owned();
        assert_eq!(cookie.http_only(), Some(true));
        assert_eq!(cookie.same_site(), Some(SameSite::Strict));
        let request = test::TestRequest::get()
            .cookie(cookie.clone())
            .to_http_request();
        let grant = auth.grant(&request).unwrap();
        assert!(grant.valid());
        let res = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/thumbnail")
                .cookie(cookie.clone())
                .to_request(),
        )
        .await;
        assert_eq!(res.status(), 200);
        let res = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/api/auth/logout")
                .insert_header(("x-nast-request", "1"))
                .cookie(cookie.clone())
                .to_request(),
        )
        .await;
        assert_eq!(res.status(), 200);
        assert!(!grant.valid());
        let res = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/thumbnail")
                .cookie(cookie)
                .to_request(),
        )
        .await;
        assert_eq!(res.status(), 401);
    }
    #[actix_web::test]
    async fn rejects_cross_origin_bad_password_and_bad_bridge_token() {
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(config()))
                .wrap(actix_web::middleware::from_fn(guard))
                .route("/api/auth/login", web::post().to(login))
                .default_service(web::to(|| async { HttpResponse::Ok().finish() })),
        )
        .await;
        for (token, code) in [("bad", 401), ("0123456789abcdef0123456789abcdef", 200)] {
            let res = test::call_service(
                &app,
                test::TestRequest::get()
                    .uri("/ws")
                    .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                    .to_request(),
            )
            .await;
            assert_eq!(res.status(), code);
        }
        for origin in ["https://evil.example", "null"] {
            let res = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/api/auth/login")
                    .insert_header((header::ORIGIN, origin))
                    .insert_header(("x-nast-request", "1"))
                    .set_json(json!({"username":"admin","password":"test-password"}))
                    .to_request(),
            )
            .await;
            assert_eq!(res.status(), 403);
        }
        for n in 0..21 {
            let res = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/api/auth/login")
                    .insert_header(("x-nast-request", "1"))
                    .set_json(json!({"username":"admin","password":"wrong"}))
                    .to_request(),
            )
            .await;
            assert_eq!(res.status(), if n < 20 { 401 } else { 429 });
        }
    }
    #[actix_web::test]
    async fn configuration_fails_closed_and_expired_sessions_are_rejected() {
        assert!(Auth::configure(|_| String::new()).is_err());
        assert!(
            Auth::configure(|k| if k == "NAST_USERNAME" {
                "admin".into()
            } else {
                String::new()
            })
            .is_err()
        );
        assert!(
            Auth::configure(|k| if k == "NAST_ALLOW_ANONYMOUS" {
                "true".into()
            } else {
                String::new()
            })
            .is_ok()
        );
        let auth = config();
        auth.sessions.lock().unwrap().insert(
            "expired".into(),
            Arc::new(Session {
                expires: Instant::now() - Duration::from_secs(1),
                cancelled: CancellationToken::new(),
            }),
        );
        assert!(
            auth.grant(
                &test::TestRequest::get()
                    .cookie(Cookie::new(COOKIE, "expired"))
                    .to_http_request()
            )
            .is_none()
        );
    }
}
