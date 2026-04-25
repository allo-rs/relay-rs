//! Web 面板 HTTP 服务（axum）。
//!
//! 与 gRPC 服务并行运行（同一 `run_daemon`，通过 `tokio::try_join!` 协同）。
//!
//! 认证模型：
//! - 公共：`/api/auth/discourse/login`、`/api/auth/discourse/callback`
//! - 用户：`/api/auth/me`、`/api/auth/logout`（自身校验）
//! - `/api/v1/*` 与 `/api/v1/settings/*` 都强制 admin（先要求 user JWT，再要求 `claims.admin`）
//!
//! 与 v0 panel 关键差异：
//! - 端口由 `RELAY_PANEL_LISTEN` 控制（默认 `0.0.0.0:9090`），与 v0 同名但生命周期独立。
//! - 配置统一存到 v1 专属表 `v1_settings`，不复用 v0 `settings`。
//! - 不内置 TLS（建议外置 nginx/cloudflare 终结）；Cookie `Secure` 由
//!   `RELAY_PANEL_EXTERNAL_URL` 是否 https 决定。

use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{Path, Query, Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{delete, get, post},
};
use rand::RngCore;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgPool;
// (CORS removed: panel UI and API serve from the same origin)

use crate::admin;
use crate::ca::Ca;
use crate::panel::auth::{AUTH_COOKIE, UserClaims, now_secs};
use crate::panel::discourse::{self, NonceStore};
use crate::panel::settings as kv;
use crate::token::TokenStore;

// ── 状态 ──────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct DiscourseRuntime {
    pub url: String,
    pub secret: String,
}

pub struct AppState {
    pub db: PgPool,
    pub jwt_secret: String,
    pub discourse: Arc<RwLock<Option<DiscourseRuntime>>>,
    pub nonce_store: Arc<NonceStore>,
    pub external_base_url: String,
    pub tokens: Arc<TokenStore>,
    pub ca: Arc<Ca>,
    pub dev_bypass: bool,
}

impl AppState {
    fn discourse(&self) -> Option<DiscourseRuntime> {
        self.discourse.read().ok().and_then(|g| g.clone())
    }

    async fn reload_discourse(&self) {
        match kv::get_setting(&self.db, "discourse").await {
            Ok(Some(v)) => {
                let url = v
                    .get("url")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let secret = v
                    .get("secret")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let rt = if !url.is_empty() && !secret.is_empty() {
                    Some(DiscourseRuntime { url, secret })
                } else {
                    None
                };
                if let Ok(mut g) = self.discourse.write() {
                    let changed = g.as_ref().map(|x| (&x.url, &x.secret))
                        != rt.as_ref().map(|x| (&x.url, &x.secret));
                    *g = rt.clone();
                    if changed {
                        log::info!(
                            "discourse 配置已重载（configured={}）",
                            rt.is_some()
                        );
                    }
                }
            }
            Ok(None) => {
                if let Ok(mut g) = self.discourse.write() {
                    *g = None;
                }
            }
            Err(e) => log::warn!("读取 v1_settings.discourse 失败: {}", e),
        }
    }

    fn cookie_secure(&self) -> bool {
        self.external_base_url.starts_with("https://")
    }
}

// ── 入口 ──────────────────────────────────────────────────────────

/// 启动面板 HTTP 服务。**与 gRPC 同时运行**，二者由 `try_join!` 同步退出。
pub async fn run(
    listen: SocketAddr,
    pool: PgPool,
    tokens: Arc<TokenStore>,
    ca: Arc<Ca>,
) -> Result<()> {
    let jwt_secret = match std::env::var("RELAY_PANEL_JWT_SECRET").ok() {
        Some(s) if !s.trim().is_empty() => s,
        _ => {
            let mut buf = [0u8; 32];
            rand::thread_rng().fill_bytes(&mut buf);
            let s = hex::encode(buf);
            log::warn!(
                "未设置 RELAY_PANEL_JWT_SECRET，本次启动随机生成（重启后失效，所有用户需重新登录）"
            );
            s
        }
    };

    let external_base_url = std::env::var("RELAY_PANEL_EXTERNAL_URL")
        .unwrap_or_default()
        .trim_end_matches('/')
        .to_string();

    let dev_bypass = matches!(
        std::env::var("RELAY_PANEL_DEV").as_deref(),
        Ok("1") | Ok("true")
    );
    if dev_bypass {
        log::warn!(
            "[安全] RELAY_PANEL_DEV=1：登录端点支持 ?devbypass=1 直发 admin 凭证（仅限开发！）"
        );
    }

    let state = Arc::new(AppState {
        db: pool,
        jwt_secret,
        discourse: Arc::new(RwLock::new(None)),
        nonce_store: Arc::new(NonceStore::new()),
        external_base_url,
        tokens,
        ca,
        dev_bypass,
    });

    // 启动 + 每 30s 刷新 discourse 配置
    {
        let s = state.clone();
        tokio::spawn(async move {
            let mut first = true;
            loop {
                s.reload_discourse().await;
                if first {
                    first = false;
                    if s.discourse().is_none() {
                        log::warn!(
                            "[安全] Discourse 未配置，受保护 API 仍要求登录，请尽快通过 PUT /api/v1/settings/discourse 完成配置"
                        );
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            }
        });
    }

    let app = router(state);

    log::info!("relay-master 面板 HTTP 监听 {}", listen);
    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .with_context(|| format!("绑定 panel 端口 {} 失败", listen))?;
    axum::serve(listener, app)
        .with_graceful_shutdown(crate::shutdown_signal())
        .await
        .context("panel HTTP 服务异常退出")?;
    Ok(())
}

fn router(state: Arc<AppState>) -> Router {
    let public = Router::new()
        .route("/api/auth/discourse/login", get(handle_discourse_login))
        .route("/api/auth/discourse/callback", get(handle_discourse_callback))
        .route("/api/auth/logout", post(handle_logout))
        .route("/api/auth/me", get(handle_me))
        .with_state(state.clone());

    let admin_api = Router::new()
        .route("/api/v1/nodes", get(handle_list_nodes))
        .route("/api/v1/nodes/:id", delete(handle_delete_node))
        .route(
            "/api/v1/segments",
            get(handle_list_segments).post(handle_add_segment),
        )
        .route("/api/v1/segments/:id", delete(handle_delete_segment))
        .route("/api/v1/enrollment-tokens", post(handle_create_enrollment))
        .route(
            "/api/v1/settings/discourse",
            get(handle_get_discourse_setting)
                .put(handle_put_discourse_setting)
                .delete(handle_delete_discourse_setting),
        )
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            admin_guard,
        ));

    Router::new()
        .merge(public)
        .merge(admin_api)
        .fallback(handle_static)
}

async fn handle_static(req: Request) -> Response {
    if req.method() != axum::http::Method::GET {
        return (StatusCode::METHOD_NOT_ALLOWED, "method not allowed").into_response();
    }
    super::assets::serve_asset(req.uri().clone()).await.into_response()
}

// ── 中间件 ────────────────────────────────────────────────────────

fn unauthorized(msg: &str) -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({ "error": msg }))).into_response()
}

fn forbidden(msg: &str) -> Response {
    (StatusCode::FORBIDDEN, Json(json!({ "error": msg }))).into_response()
}

fn server_error(msg: impl std::fmt::Display) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": msg.to_string() })),
    )
        .into_response()
}

/// 认证错误（小型枚举，避免把 ~128B 的 `Response` 放进 `Result::Err`，触发
/// `clippy::result_large_err`）。
enum AuthErr {
    Missing,
    Invalid,
}

impl AuthErr {
    fn into_response(self) -> Response {
        match self {
            AuthErr::Missing => unauthorized("未登录"),
            AuthErr::Invalid => unauthorized("登录已过期，请重新登录"),
        }
    }
}

fn extract_user_claims(state: &AppState, req: &Request) -> Result<UserClaims, AuthErr> {
    let token = super::auth::extract_cookie(req.headers(), AUTH_COOKIE)
        .or_else(|| super::auth::extract_bearer(req.headers()))
        .ok_or(AuthErr::Missing)?;
    super::auth::verify_user_token(&token, &state.jwt_secret).map_err(|_| AuthErr::Invalid)
}

async fn admin_guard(
    State(state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Response {
    let claims = match extract_user_claims(&state, &req) {
        Ok(c) => c,
        Err(e) => return e.into_response(),
    };
    if !claims.admin {
        return forbidden("需要管理员权限");
    }
    req.extensions_mut().insert(claims);
    next.run(req).await
}

// ── Discourse SSO ─────────────────────────────────────────────────

#[derive(Deserialize)]
struct LoginQuery {
    return_to: Option<String>,
    next: Option<String>,
    devbypass: Option<String>,
}

fn safe_relative(path: Option<String>) -> String {
    match path {
        Some(p) if p.starts_with('/') && !p.starts_with("//") => p,
        _ => "/".to_string(),
    }
}

fn build_callback_url(state: &AppState, req: &Request) -> String {
    if !state.external_base_url.is_empty() {
        return format!("{}/api/auth/discourse/callback", state.external_base_url);
    }
    let scheme = req
        .headers()
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    format!("{}://{}/api/auth/discourse/callback", scheme, host)
}

/// 签发登录 cookie；签名失败时直接返回错误字符串，调用方用 `server_error` 包装。
fn issue_login_cookie(state: &AppState, claims: &UserClaims) -> Result<String, String> {
    let token = super::auth::create_user_token(claims, &state.jwt_secret)
        .map_err(|e| format!("签发 token 失败: {}", e))?;
    let secure = if state.cookie_secure() { "; Secure" } else { "" };
    Ok(format!(
        "{}={}; HttpOnly; Path=/; Max-Age={}; SameSite=Lax{}",
        AUTH_COOKIE,
        token,
        7 * 24 * 3600,
        secure
    ))
}

async fn handle_discourse_login(
    State(state): State<Arc<AppState>>,
    Query(q): Query<LoginQuery>,
    req: Request,
) -> Response {
    let return_to = safe_relative(q.return_to.or(q.next));

    // 开发态：?devbypass=1 直接发 admin cookie，不走 discourse
    if state.dev_bypass && matches!(q.devbypass.as_deref(), Some("1") | Some("true")) {
        let claims = UserClaims {
            sub: "dev".into(),
            username: "dev".into(),
            name: Some("Dev Admin".into()),
            email: None,
            avatar: None,
            admin: true,
            exp: now_secs() + 7 * 24 * 3600,
        };
        let cookie = match issue_login_cookie(&state, &claims) {
            Ok(c) => c,
            Err(e) => return server_error(e),
        };
        let mut resp = Redirect::temporary(&return_to).into_response();
        if let Ok(v) = HeaderValue::from_str(&cookie) {
            resp.headers_mut().append(header::SET_COOKIE, v);
        }
        return resp;
    }

    let disc = match state.discourse() {
        Some(d) => d,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "未配置 Discourse SSO" })),
            )
                .into_response();
        }
    };

    let nonce = state.nonce_store.issue(return_to);
    let callback = build_callback_url(&state, &req);
    let redirect = discourse::build_login_redirect(&disc.url, &disc.secret, &callback, &nonce);
    Redirect::temporary(&redirect).into_response()
}

#[derive(Deserialize)]
struct CallbackQuery {
    sso: String,
    sig: String,
}

async fn handle_discourse_callback(
    State(state): State<Arc<AppState>>,
    Query(q): Query<CallbackQuery>,
) -> Response {
    let disc = match state.discourse() {
        Some(d) => d,
        None => return login_failed("未配置 Discourse"),
    };

    let mut return_to: Option<String> = None;
    let parsed = discourse::verify_and_parse(&q.sso, &q.sig, &disc.secret, |nonce| {
        if let Some(rt) = state.nonce_store.consume(nonce) {
            return_to = Some(rt);
            true
        } else {
            false
        }
    });

    let claims = match parsed {
        Ok(c) => c,
        Err(e) => {
            log::warn!("Discourse 回调校验失败: {}", e);
            return login_failed(&e);
        }
    };

    let cookie = match issue_login_cookie(&state, &claims) {
        Ok(c) => c,
        Err(e) => return server_error(e),
    };
    let next = safe_relative(return_to);

    let mut resp = Redirect::temporary(&next).into_response();
    if let Ok(v) = HeaderValue::from_str(&cookie) {
        resp.headers_mut().append(header::SET_COOKIE, v);
    }
    resp
}

fn login_failed(msg: &str) -> Response {
    let body = format!(
        "<!doctype html><html><body><h3>登录失败</h3><p>{}</p><a href=\"/login\">返回</a></body></html>",
        html_escape(msg)
    );
    let mut resp = (StatusCode::UNAUTHORIZED, body).into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    resp
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

async fn handle_me(State(state): State<Arc<AppState>>, req: Request) -> Response {
    match extract_user_claims(&state, &req) {
        Ok(c) => Json(json!({
            "ok": true,
            "configured": state.discourse().is_some(),
            "user": {
                "id": c.sub,
                "username": c.username,
                "name": c.name,
                "email": c.email,
                "avatar": c.avatar,
                "admin": c.admin,
            }
        }))
        .into_response(),
        Err(e) => e.into_response(),
    }
}

async fn handle_logout(State(state): State<Arc<AppState>>) -> Response {
    let secure = if state.cookie_secure() { "; Secure" } else { "" };
    let expired = format!(
        "{}=; HttpOnly; Path=/; Max-Age=0; SameSite=Lax{}",
        AUTH_COOKIE, secure
    );
    let mut resp = (StatusCode::NO_CONTENT, ()).into_response();
    if let Ok(v) = HeaderValue::from_str(&expired) {
        resp.headers_mut().append(header::SET_COOKIE, v);
    }
    resp
}

// ── /api/v1/nodes ─────────────────────────────────────────────────

async fn handle_list_nodes(State(state): State<Arc<AppState>>) -> Response {
    match admin::node_list_rows(&state.db).await {
        Ok(rows) => Json(json!({ "items": rows })).into_response(),
        Err(e) => server_error(e),
    }
}

async fn handle_delete_node(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    match admin::node_rm_inner(&state.db, &id).await {
        Ok(true) => (StatusCode::NO_CONTENT, ()).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("节点 {} 不存在", id) })),
        )
            .into_response(),
        Err(e) => server_error(e),
    }
}

// ── /api/v1/segments ──────────────────────────────────────────────

#[derive(Deserialize)]
struct SegListQuery {
    node: Option<String>,
}

async fn handle_list_segments(
    State(state): State<Arc<AppState>>,
    Query(q): Query<SegListQuery>,
) -> Response {
    match admin::seg_list_rows(&state.db, q.node.as_deref()).await {
        Ok(rows) => Json(json!({ "items": rows })).into_response(),
        Err(e) => server_error(e),
    }
}

#[derive(Deserialize)]
struct SegAddBody {
    node: String,
    listen: String,
    upstream: String,
    chain: Option<String>,
    proto: Option<String>,
    #[serde(default)]
    ipv6: bool,
    comment: Option<String>,
}

async fn handle_add_segment(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SegAddBody>,
) -> Response {
    let proto = body.proto.unwrap_or_else(|| "tcp".to_string());
    match admin::seg_add_inner(
        &state.db,
        admin::SegAddSpec {
            node_id: &body.node,
            listen: &body.listen,
            upstream: &body.upstream,
            chain: body.chain,
            proto: &proto,
            ipv6: body.ipv6,
            comment: body.comment,
        },
    )
    .await
    {
        Ok((id, chain_id)) => (
            StatusCode::CREATED,
            Json(json!({ "id": id, "chain_id": chain_id })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn handle_delete_segment(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    match admin::seg_rm_inner(&state.db, &id).await {
        Ok(Some(_)) => (StatusCode::NO_CONTENT, ()).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("segment {} 不存在", id) })),
        )
            .into_response(),
        Err(e) => server_error(e),
    }
}

// ── /api/v1/enrollment-tokens ─────────────────────────────────────

#[derive(Deserialize)]
struct EnrollBody {
    name: String,
    ttl: Option<u64>,
}

async fn handle_create_enrollment(
    State(state): State<Arc<AppState>>,
    Json(body): Json<EnrollBody>,
) -> Response {
    let name = body.name.trim();
    if name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "name 不能为空" })),
        )
            .into_response();
    }
    // 严格字符集：node name 只允许字母/数字/`_`/`-`/`.`，避免 install_cmd 拼接出 shell 注入。
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        || name.len() > 64
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "name 仅允许字母/数字/_-. 且长度<=64" })),
        )
            .into_response();
    }
    let token = match state.tokens.create(name, body.ttl) {
        Ok(t) => t,
        Err(e) => return server_error(e),
    };

    // master URL（让 install 命令可以直接 paste）：优先用 RELAY_MASTER_PUBLIC_URL，
    // 退化到 external_base_url 把 https/http 部分留下。
    let master_url = std::env::var("RELAY_MASTER_PUBLIC_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "https://<master-host>:9443".to_string());

    use base64::Engine;
    let ca_b64 =
        base64::engine::general_purpose::STANDARD.encode(state.ca.cert_pem.as_bytes());

    let install_cmd = format!(
        "bash <(curl -fsSL https://raw.githubusercontent.com/allo-rs/relay-rs/main/scripts/install-node.sh) \
--master {master_url} --ca-b64 \"{ca}\" --enrollment-token {token} --node-name {name}",
        master_url = master_url,
        ca = ca_b64,
        token = token,
        name = name,
    );

    Json(json!({
        "token": token,
        "node_name": name,
        "install_cmd": install_cmd,
    }))
    .into_response()
}

// ── /api/v1/settings/discourse ────────────────────────────────────

async fn handle_get_discourse_setting(State(state): State<Arc<AppState>>) -> Response {
    let d = state.discourse();
    Json(json!({
        "configured": d.is_some(),
        "url": d.as_ref().map(|x| x.url.clone()).unwrap_or_default(),
        "secret_set": d.is_some(),
    }))
    .into_response()
}

#[derive(Deserialize)]
struct DiscourseSettingBody {
    url: String,
    /// 留空表示沿用旧 secret。
    secret: Option<String>,
}

async fn handle_put_discourse_setting(
    State(state): State<Arc<AppState>>,
    Json(body): Json<DiscourseSettingBody>,
) -> Response {
    let url = body.url.trim().trim_end_matches('/').to_string();
    if url.is_empty() || !(url.starts_with("http://") || url.starts_with("https://")) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "url 需为 http(s):// 开头" })),
        )
            .into_response();
    }

    let new_secret = match body
        .secret
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(s) => s.to_string(),
        None => match state.discourse() {
            Some(d) => d.secret,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "首次配置必须提供 secret" })),
                )
                    .into_response();
            }
        },
    };
    if new_secret.len() < 10 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "secret 至少 10 字符" })),
        )
            .into_response();
    }

    let value: Value = json!({ "url": url, "secret": new_secret });
    if let Err(e) = kv::put_setting(&state.db, "discourse", &value).await {
        return server_error(e);
    }
    state.reload_discourse().await;
    Json(json!({ "configured": true, "url": url, "secret_set": true })).into_response()
}

async fn handle_delete_discourse_setting(State(state): State<Arc<AppState>>) -> Response {
    if let Err(e) = kv::delete_setting(&state.db, "discourse").await {
        return server_error(e);
    }
    state.reload_discourse().await;
    (StatusCode::NO_CONTENT, ()).into_response()
}
