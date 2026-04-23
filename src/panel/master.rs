use axum::{
    Json, Router,
    extract::{Path, Query, Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{delete, get, post, put},
};
use base64::Engine as _;
use rcgen;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::sync::{Arc, RwLock};
use tower_http::cors::CorsLayer;

use crate::config::PanelConfig;
use crate::panel::auth::AUTH_COOKIE;
use crate::panel::discourse::{self as discourse, NonceStore};

/// 运行时 Discourse 配置（来自 DB，可热更新）
#[derive(Clone, Debug)]
pub struct DiscourseRuntime {
    pub url: String,
    pub secret: String,
}

#[derive(Clone)]
struct MasterState {
    panel_cfg: PanelConfig,
    http_client: reqwest::Client,
    db: PgPool,
    nonces: Arc<NonceStore>,
    /// 运行时 Discourse 配置缓存；None 表示「未配置」，此时 panel 开放访问
    discourse: Arc<RwLock<Option<DiscourseRuntime>>>,
}

impl MasterState {
    fn discourse(&self) -> Option<DiscourseRuntime> {
        self.discourse.read().ok().and_then(|g| g.clone())
    }

    async fn reload_discourse(&self) {
        match crate::db::get_setting(&self.db, "discourse").await {
            Ok(Some(v)) => {
                let url = v.get("url").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
                let secret = v.get("secret").and_then(|x| x.as_str()).unwrap_or("").to_string();
                let rt = if !url.is_empty() && !secret.is_empty() {
                    Some(DiscourseRuntime { url, secret })
                } else {
                    None
                };
                if let Ok(mut g) = self.discourse.write() { *g = rt; }
            }
            Ok(None) => {
                if let Ok(mut g) = self.discourse.write() { *g = None; }
            }
            Err(e) => log::warn!("读取 settings.discourse 失败: {}", e),
        }
    }
}

fn is_public_path(path: &str) -> bool {
    matches!(
        path,
        "/api/auth/discourse/login" | "/api/auth/discourse/callback"
    )
}

/// JWT 认证中间件：从 HttpOnly cookie 读取；未配置 Discourse 时放行
async fn jwt_middleware(State(state): State<MasterState>, req: Request, next: Next) -> Response {
    if is_public_path(req.uri().path()) {
        return next.run(req).await;
    }
    // 未配置 Discourse → 开放访问
    if state.discourse().is_none() {
        return next.run(req).await;
    }
    let token = super::auth::extract_cookie(req.headers(), AUTH_COOKIE);
    match token {
        Some(t) => match super::auth::verify_user_token(&t, &state.panel_cfg.secret) {
            Ok(_) => next.run(req).await,
            Err(_) => unauthorized("登录已过期，请重新登录"),
        },
        None => unauthorized("未登录"),
    }
}

fn unauthorized(msg: &str) -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({ "error": msg }))).into_response()
}

pub fn router(panel_cfg: PanelConfig, _config_path: String, db: PgPool) -> Router {
    let http_client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(std::time::Duration::from_secs(3))
        .connect_timeout(std::time::Duration::from_secs(2))
        .build()
        .unwrap();

    let state = MasterState {
        panel_cfg,
        http_client,
        db,
        nonces: Arc::new(NonceStore::new()),
        discourse: Arc::new(RwLock::new(None)),
    };

    // 启动时 + 每 30 秒异步刷新 Discourse 配置缓存（支持 CLI reset-auth 生效）
    {
        let s = state.clone();
        tokio::spawn(async move {
            loop {
                s.reload_discourse().await;
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            }
        });
    }

    let api = Router::new()
        // 认证
        .route("/api/auth/discourse/login", get(handle_discourse_login))
        .route("/api/auth/discourse/callback", get(handle_discourse_callback))
        .route("/api/auth/me", get(handle_me))
        .route("/api/auth/logout", post(handle_logout))
        // 设置
        .route(
            "/api/settings/discourse",
            get(handle_get_discourse_setting)
                .put(handle_put_discourse_setting)
                .delete(handle_delete_discourse_setting),
        )
        // 节点
        .route("/api/nodes", get(handle_list_nodes).post(handle_add_node))
        .route("/api/nodes/:id", delete(handle_del_node))
        .route("/api/nodes/:id/status", get(handle_node_status))
        .route("/api/nodes/:id/rules", get(handle_node_get_rules))
        .route("/api/nodes/:id/rules", put(handle_node_put_rules))
        .route("/api/nodes/:id/rules/forward", post(handle_node_add_forward))
        .route("/api/nodes/:id/rules/forward/:idx", delete(handle_node_del_forward))
        .route("/api/nodes/:id/rules/block", post(handle_node_add_block))
        .route("/api/nodes/:id/rules/block/:idx", delete(handle_node_del_block))
        .route("/api/nodes/:id/stats", get(handle_node_stats))
        .route("/api/nodes/:id/reload", post(handle_node_reload))
        // 跨节点聚合
        .route("/api/forwards", get(handle_list_all_forwards))
        .route("/api/pubkey", get(handle_pubkey))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state.clone(), jwt_middleware));

    Router::new()
        .merge(api)
        .fallback(handle_asset)
        .layer(CorsLayer::permissive())
}

async fn handle_asset(req: Request) -> Response {
    super::assets::serve_asset(req.uri().clone()).await.into_response()
}

// ── Discourse Connect 登录 ────────────────────────────────────────

#[derive(Deserialize)]
struct LoginQuery {
    next: Option<String>,
}

async fn handle_discourse_login(
    State(state): State<MasterState>,
    Query(q): Query<LoginQuery>,
    req: Request,
) -> Response {
    let disc = match state.discourse() {
        Some(d) => d,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "未配置 Discourse" })),
            )
                .into_response();
        }
    };

    let next = q.next.unwrap_or_else(|| "/".to_string());
    let nonce = state.nonces.issue(next);
    let callback = build_callback_url(&req, &state.panel_cfg);
    let redirect = discourse::build_login_redirect(&disc.url, &disc.secret, &callback, &nonce);

    Redirect::temporary(&redirect).into_response()
}

#[derive(Deserialize)]
struct CallbackQuery {
    sso: String,
    sig: String,
}

async fn handle_discourse_callback(
    State(state): State<MasterState>,
    Query(q): Query<CallbackQuery>,
) -> Response {
    let disc = match state.discourse() {
        Some(d) => d,
        None => return login_failed("未配置 Discourse"),
    };

    let mut return_to: Option<String> = None;
    let claims_result = discourse::verify_and_parse(&q.sso, &q.sig, &disc.secret, |nonce| {
        if let Some(rt) = state.nonces.consume(nonce) {
            return_to = Some(rt);
            true
        } else {
            false
        }
    });

    let claims = match claims_result {
        Ok(c) => c,
        Err(e) => {
            log::warn!("Discourse 回调校验失败: {}", e);
            return login_failed(&e);
        }
    };

    let token = match super::auth::create_user_token(&claims, &state.panel_cfg.secret) {
        Ok(t) => t,
        Err(e) => {
            log::error!("签发用户 JWT 失败: {}", e);
            return login_failed("签发 token 失败");
        }
    };

    let next = return_to.unwrap_or_else(|| "/".to_string());
    let cookie = format!(
        "{}={}; HttpOnly; Path=/; Max-Age={}; SameSite=Lax",
        AUTH_COOKIE, token, 7 * 24 * 3600
    );

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

fn build_callback_url(req: &Request, panel_cfg: &PanelConfig) -> String {
    let scheme = if panel_cfg.tls_cert.is_some() || panel_cfg.tls_key.is_some() {
        "https"
    } else {
        req.headers()
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("http")
    };
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or(&panel_cfg.listen);
    format!("{}://{}/api/auth/discourse/callback", scheme, host)
}

async fn handle_me(State(state): State<MasterState>, req: Request) -> Response {
    let configured = state.discourse().is_some();
    if !configured {
        // 未配置 Discourse：开放模式，返回一个占位的 setup 用户（admin=true）
        return Json(json!({
            "ok": true,
            "configured": false,
            "user": {
                "id": "setup",
                "username": "setup",
                "name": "首次部署",
                "email": null,
                "avatar": null,
                "admin": true,
            }
        }))
        .into_response();
    }
    let token = match super::auth::extract_cookie(req.headers(), AUTH_COOKIE) {
        Some(t) => t,
        None => return unauthorized("未登录"),
    };
    match super::auth::verify_user_token(&token, &state.panel_cfg.secret) {
        Ok(c) => Json(json!({
            "ok": true,
            "configured": true,
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
        Err(_) => unauthorized("登录已过期"),
    }
}

// ── 设置：Discourse 配置 ─────────────────────────────────────────

async fn handle_get_discourse_setting(State(state): State<MasterState>) -> Response {
    let d = state.discourse();
    Json(json!({
        "ok": true,
        "configured": d.is_some(),
        "url": d.as_ref().map(|x| x.url.clone()).unwrap_or_default(),
        "hasSecret": d.is_some(),
    }))
    .into_response()
}

#[derive(Deserialize)]
struct DiscourseSettingBody {
    url: String,
    /// 可选：留空表示保持原 secret 不变
    secret: Option<String>,
}

async fn handle_put_discourse_setting(
    State(state): State<MasterState>,
    Json(body): Json<DiscourseSettingBody>,
) -> Response {
    let url = body.url.trim().trim_end_matches('/').to_string();
    if url.is_empty() || !(url.starts_with("http://") || url.starts_with("https://")) {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "url 需为 http(s):// 开头" })))
            .into_response();
    }

    // 若 secret 为 None 或空，沿用旧 secret
    let new_secret = match body.secret.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
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

    let value = json!({ "url": url, "secret": new_secret });
    if let Err(e) = crate::db::set_setting(&state.db, "discourse", &value).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("写入失败: {}", e) })),
        )
            .into_response();
    }
    state.reload_discourse().await;
    Json(json!({ "ok": true, "configured": true, "url": url, "hasSecret": true })).into_response()
}

async fn handle_delete_discourse_setting(State(state): State<MasterState>) -> Response {
    if let Err(e) = crate::db::delete_setting(&state.db, "discourse").await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("删除失败: {}", e) })),
        )
            .into_response();
    }
    state.reload_discourse().await;
    Json(json!({ "ok": true, "configured": false })).into_response()
}

async fn handle_logout() -> Response {
    let expired = format!("{}=; HttpOnly; Path=/; Max-Age=0; SameSite=Lax", AUTH_COOKIE);
    let mut resp = Json(json!({ "ok": true })).into_response();
    if let Ok(v) = HeaderValue::from_str(&expired) {
        resp.headers_mut().append(header::SET_COOKIE, v);
    }
    resp
}

// ── 跨节点聚合 ────────────────────────────────────────────────────

/// GET /api/forwards — 并发拉取所有节点的转发规则并打平
///
/// 节点不可达时该节点 online=false 但不影响其他节点结果。
async fn handle_list_all_forwards(State(state): State<MasterState>) -> Response {
    let nodes = match crate::db::list_nodes(&state.db).await {
        Ok(n) => n,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    };

    let pk = state.panel_cfg.private_key.clone().unwrap_or_default();
    let mut set = tokio::task::JoinSet::new();
    for node in nodes {
        let client = state.http_client.clone();
        let pk = pk.clone();
        set.spawn(async move {
            let entry = mk_entry(&node.name, &node.url);
            let res = proxy_to_node(&client, &entry, reqwest::Method::GET, "/api/rules", None, &pk).await;
            (node, res)
        });
    }

    let mut items: Vec<Value> = Vec::new();
    let mut node_summaries: Vec<Value> = Vec::new();
    while let Some(joined) = set.join_next().await {
        let (node, res) = match joined {
            Ok(v) => v,
            Err(e) => { log::warn!("聚合任务 panic: {}", e); continue; }
        };
        match res {
            Ok(value) => {
                let arr = value.get("forward").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                node_summaries.push(json!({
                    "id": node.id,
                    "name": node.name,
                    "online": true,
                    "rule_count": arr.len(),
                }));
                for (idx, rule) in arr.into_iter().enumerate() {
                    items.push(json!({
                        "node_id": node.id,
                        "node_name": node.name,
                        "node_online": true,
                        "idx": idx,
                        "rule": rule,
                    }));
                }
            }
            Err(_) => {
                node_summaries.push(json!({
                    "id": node.id,
                    "name": node.name,
                    "online": false,
                    "rule_count": 0,
                }));
            }
        }
    }

    items.sort_by(|a, b| {
        let ai = a.get("node_id").and_then(|v| v.as_i64()).unwrap_or(0);
        let bi = b.get("node_id").and_then(|v| v.as_i64()).unwrap_or(0);
        ai.cmp(&bi).then_with(|| {
            let ax = a.get("idx").and_then(|v| v.as_i64()).unwrap_or(-1);
            let bx = b.get("idx").and_then(|v| v.as_i64()).unwrap_or(-1);
            ax.cmp(&bx)
        })
    });
    node_summaries.sort_by_key(|n| n.get("id").and_then(|v| v.as_i64()).unwrap_or(0));

    Json(json!({ "ok": true, "nodes": node_summaries, "items": items })).into_response()
}

// ── 主控公钥 ──────────────────────────────────────────────────────

async fn handle_pubkey(State(state): State<MasterState>) -> Response {
    let pubkey = derive_pubkey_pem(state.panel_cfg.private_key.as_deref().unwrap_or(""));
    Json(json!({ "pubkey": pubkey })).into_response()
}

// ── 节点 CRUD ─────────────────────────────────────────────────────

/// GET /api/nodes — 仅从 DB 读节点列表（不探活，状态由前端按需调用 /api/nodes/:id/status）
async fn handle_list_nodes(State(state): State<MasterState>) -> Response {
    let nodes = match crate::db::list_nodes(&state.db).await {
        Ok(n) => n,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    };
    let list: Vec<Value> = nodes
        .into_iter()
        .map(|n| json!({ "id": n.id, "name": n.name, "url": n.url, "status": null }))
        .collect();
    Json(json!({ "ok": true, "nodes": list })).into_response()
}

#[derive(Deserialize)]
struct AddNodeBody { name: String, url: String }

/// POST /api/nodes — 写入 DB，返回主控公钥供节点安装脚本使用
async fn handle_add_node(State(state): State<MasterState>, Json(body): Json<AddNodeBody>) -> Response {
    let pubkey = derive_pubkey_pem(state.panel_cfg.private_key.as_deref().unwrap_or(""));
    match crate::db::add_node(&state.db, &body.name, &body.url).await {
        Ok(id) => Json(json!({ "ok": true, "id": id, "pubkey": pubkey })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

/// DELETE /api/nodes/:id — 从 DB 删除节点
async fn handle_del_node(State(state): State<MasterState>, Path(id): Path<i32>) -> Response {
    match crate::db::delete_node(&state.db, id).await {
        Ok(true) => Json(json!({ "ok": true })).into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, Json(json!({ "error": format!("节点 {} 不存在", id) }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

// ── 节点代理操作 ──────────────────────────────────────────────────

async fn handle_node_status(State(state): State<MasterState>, Path(id): Path<i32>) -> Response {
    with_node(&state, id, |c, n, pk| async move {
        forward_get(c, n, "/api/status", pk).await
    }).await
}

async fn handle_node_get_rules(State(state): State<MasterState>, Path(id): Path<i32>) -> Response {
    with_node(&state, id, |c, n, pk| async move {
        forward_get(c, n, "/api/rules", pk).await
    }).await
}

async fn handle_node_put_rules(
    State(state): State<MasterState>,
    Path(id): Path<i32>,
    Json(body): Json<Value>,
) -> Response {
    with_node(&state, id, |c, n, pk| async move {
        forward_body(c, n, reqwest::Method::PUT, "/api/rules", body, pk).await
    }).await
}

async fn handle_node_add_forward(
    State(state): State<MasterState>,
    Path(id): Path<i32>,
    Json(body): Json<Value>,
) -> Response {
    with_node(&state, id, |c, n, pk| async move {
        forward_body(c, n, reqwest::Method::POST, "/api/rules/forward", body, pk).await
    }).await
}

async fn handle_node_del_forward(
    State(state): State<MasterState>,
    Path((id, idx)): Path<(i32, usize)>,
) -> Response {
    with_node(&state, id, |c, n, pk| async move {
        forward_method(c, n, reqwest::Method::DELETE, &format!("/api/rules/forward/:idx"), None, pk).await
    }).await
}

async fn handle_node_add_block(
    State(state): State<MasterState>,
    Path(id): Path<i32>,
    Json(body): Json<Value>,
) -> Response {
    with_node(&state, id, |c, n, pk| async move {
        forward_body(c, n, reqwest::Method::POST, "/api/rules/block", body, pk).await
    }).await
}

async fn handle_node_del_block(
    State(state): State<MasterState>,
    Path((id, idx)): Path<(i32, usize)>,
) -> Response {
    with_node(&state, id, |c, n, pk| async move {
        forward_method(c, n, reqwest::Method::DELETE, &format!("/api/rules/block/:idx"), None, pk).await
    }).await
}

async fn handle_node_stats(State(state): State<MasterState>, Path(id): Path<i32>) -> Response {
    with_node(&state, id, |c, n, pk| async move {
        forward_get(c, n, "/api/stats", pk).await
    }).await
}

async fn handle_node_reload(State(state): State<MasterState>, Path(id): Path<i32>) -> Response {
    with_node(&state, id, |c, n, pk| async move {
        forward_method(c, n, reqwest::Method::POST, "/api/reload", None, pk).await
    }).await
}

// ── 辅助 ──────────────────────────────────────────────────────────

/// 从 DB 取节点后执行闭包，节点不存在时返回 404
async fn with_node<F, Fut>(state: &MasterState, id: i32, f: F) -> Response
where
    F: FnOnce(reqwest::Client, crate::config::NodeEntry, String) -> Fut,
    Fut: std::future::Future<Output = Response>,
{
    match crate::db::get_node(&state.db, id).await {
        Ok(Some(node)) => {
            let entry = mk_entry(&node.name, &node.url);
            let pk = state.panel_cfg.private_key.clone().unwrap_or_default();
            f(state.http_client.clone(), entry, pk).await
        }
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({ "error": format!("节点 {} 不存在", id) }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

fn mk_entry(name: &str, url: &str) -> crate::config::NodeEntry {
    crate::config::NodeEntry { name: name.to_string(), url: url.to_string() }
}

async fn forward_get(client: reqwest::Client, node: crate::config::NodeEntry, path: &str, pk: String) -> Response {
    match proxy_to_node(&client, &node, reqwest::Method::GET, path, None, &pk).await {
        Ok(v) => Json(v).into_response(),
        Err(code) => (code, Json(json!({ "error": "转发请求失败" }))).into_response(),
    }
}

async fn forward_body(
    client: reqwest::Client,
    node: crate::config::NodeEntry,
    method: reqwest::Method,
    path: &str,
    body: Value,
    pk: String,
) -> Response {
    match proxy_to_node(&client, &node, method, path, Some(body), &pk).await {
        Ok(v) => Json(v).into_response(),
        Err(code) => (code, Json(json!({ "error": "转发请求失败" }))).into_response(),
    }
}

async fn forward_method(
    client: reqwest::Client,
    node: crate::config::NodeEntry,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
    pk: String,
) -> Response {
    match proxy_to_node(&client, &node, method, path, body, &pk).await {
        Ok(v) => Json(v).into_response(),
        Err(code) => (code, Json(json!({ "error": "转发请求失败" }))).into_response(),
    }
}

async fn proxy_to_node(
    client: &reqwest::Client,
    node: &crate::config::NodeEntry,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
    private_key: &str,
) -> Result<Value, StatusCode> {
    let token = match super::auth::sign_node_jwt(private_key) {
        Ok(t) => t,
        Err(e) => { log::error!("签署节点 JWT 失败: {}", e); return Err(StatusCode::INTERNAL_SERVER_ERROR); }
    };

    let url = format!("{}{}", node.url.trim_end_matches('/'), path);
    let mut req = client.request(method, &url).header("Authorization", format!("Bearer {}", token));
    if let Some(b) = body { req = req.json(&b); }

    let resp = req.send().await.map_err(|e| {
        log::warn!("转发到 node {} 失败: {}", node.name, e);
        StatusCode::BAD_GATEWAY
    })?;

    let status = resp.status();
    if !status.is_success() {
        log::warn!("node {} 返回错误状态: {}", node.name, status);
        return Err(StatusCode::BAD_GATEWAY);
    }
    let json: Value = resp.json().await.map_err(|e| {
        log::warn!("node {} 返回非 JSON 响应: {}", node.name, e);
        StatusCode::BAD_GATEWAY
    })?;
    Ok(json)
}

fn derive_pubkey_pem(private_key_pem: &str) -> String {
    if private_key_pem.is_empty() { return String::new(); }
    let kp = match rcgen::KeyPair::from_pem(private_key_pem) {
        Ok(k) => k,
        Err(_) => return String::new(),
    };
    let der = kp.public_key_der();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&der);
    let lines: Vec<&str> = b64.as_bytes().chunks(64)
        .map(|c| std::str::from_utf8(c).unwrap_or("")).collect();
    format!("-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n", lines.join("\n"))
}
