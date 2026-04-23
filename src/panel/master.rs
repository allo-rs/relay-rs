use axum::{
    Json, Router,
    extract::{Path, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use base64::Engine as _;
use rcgen;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower_http::cors::CorsLayer;

use crate::config::PanelConfig;

#[derive(Clone)]
struct MasterState {
    panel_cfg: PanelConfig,
    http_client: reqwest::Client,
    db: PgPool,
}

/// JWT 认证中间件，跳过 /api/auth/login
async fn jwt_middleware(State(state): State<MasterState>, req: Request, next: Next) -> Response {
    if req.uri().path() == "/api/auth/login" {
        return next.run(req).await;
    }
    let token = super::auth::extract_bearer(req.headers());
    match token {
        Some(t) => match super::auth::verify_token(&t, &state.panel_cfg.secret) {
            Ok(_) => next.run(req).await,
            Err(_) => (StatusCode::UNAUTHORIZED, Json(json!({ "error": "JWT 验证失败" }))).into_response(),
        },
        None => (StatusCode::UNAUTHORIZED, Json(json!({ "error": "缺少 Authorization 头" }))).into_response(),
    }
}

pub fn router(panel_cfg: PanelConfig, _config_path: String, db: PgPool) -> Router {
    let http_client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();

    let state = MasterState { panel_cfg, http_client, db };

    let api = Router::new()
        .route("/api/auth/login", post(handle_login))
        .route("/api/nodes", get(handle_list_nodes).post(handle_add_node))
        .route("/api/nodes/{id}", delete(handle_del_node))
        .route("/api/nodes/{id}/status", get(handle_node_status))
        .route("/api/nodes/{id}/rules", get(handle_node_get_rules))
        .route("/api/nodes/{id}/rules", put(handle_node_put_rules))
        .route("/api/nodes/{id}/rules/forward", post(handle_node_add_forward))
        .route("/api/nodes/{id}/rules/forward/{idx}", delete(handle_node_del_forward))
        .route("/api/nodes/{id}/rules/block", post(handle_node_add_block))
        .route("/api/nodes/{id}/rules/block/{idx}", delete(handle_node_del_block))
        .route("/api/nodes/{id}/stats", get(handle_node_stats))
        .route("/api/nodes/{id}/reload", post(handle_node_reload))
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

// ── 登录 ──────────────────────────────────────────────────────────

async fn handle_login(State(state): State<MasterState>, Json(body): Json<Value>) -> Response {
    let auth = match &state.panel_cfg.auth {
        Some(a) => a.clone(),
        None => return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": "未配置面板认证" }))).into_response(),
    };
    let username = match body.get("username").and_then(Value::as_str) {
        Some(u) => u.to_string(),
        None => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "缺少 username 字段" }))).into_response(),
    };
    let password = match body.get("password").and_then(Value::as_str) {
        Some(p) => p.to_string(),
        None => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "缺少 password 字段" }))).into_response(),
    };
    if username != auth.username {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "用户名或密码错误" }))).into_response();
    }
    match bcrypt::verify(&password, &auth.password) {
        Ok(true) => {}
        _ => return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "用户名或密码错误" }))).into_response(),
    }
    match super::auth::create_token(&username, &state.panel_cfg.secret) {
        Ok(token) => Json(json!({ "token": token })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

// ── 节点 CRUD ─────────────────────────────────────────────────────

/// GET /api/nodes — 从 DB 读节点列表并并发探活
async fn handle_list_nodes(State(state): State<MasterState>) -> Response {
    let nodes = match crate::db::list_nodes(&state.db).await {
        Ok(n) => n,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    };

    let pk = state.panel_cfg.private_key.clone().unwrap_or_default();
    let mut tasks = Vec::new();

    for node in nodes {
        let client = state.http_client.clone();
        let pk = pk.clone();
        tasks.push(tokio::spawn(async move {
            let entry = mk_entry(&node.name, &node.url);
            let status = proxy_to_node(&client, &entry, reqwest::Method::GET, "/api/status", None, &pk)
                .await.ok();
            json!({ "id": node.id, "name": node.name, "url": node.url, "status": status })
        }));
    }

    let mut results = Vec::new();
    for t in tasks { if let Ok(v) = t.await { results.push(v); } }
    Json(json!({ "ok": true, "nodes": results })).into_response()
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
        forward_method(c, n, reqwest::Method::DELETE, &format!("/api/rules/forward/{idx}"), None, pk).await
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
        forward_method(c, n, reqwest::Method::DELETE, &format!("/api/rules/block/{idx}"), None, pk).await
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
    let json: Value = resp.json().await.unwrap_or(json!({}));

    if status.is_success() { Ok(json) } else {
        log::warn!("node {} 返回错误状态: {}", node.name, status);
        Err(StatusCode::BAD_GATEWAY)
    }
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
