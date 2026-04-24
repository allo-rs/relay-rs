//! node 模式 HTTP API
//!
//! master 通过 Ed25519 JWT 签名调用本节点，修改 `/var/lib/relay-rs/state.json`
//! 中的 forward/block 规则，并翻转共享的 reload 原子量通知 proxy 代际切换。

use axum::{
    Json, Router,
    extract::{Path, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use serde_json::{json, Value};
use tower_http::cors::CorsLayer;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::config;
use crate::node_state;

/// node 路由共享状态
#[derive(Clone)]
pub struct NodeHandlerState {
    state_path: String,
    reload: Arc<AtomicBool>,
    write_lock: Arc<tokio::sync::Mutex<()>>,
}

impl NodeHandlerState {
    pub fn new(state_path: String, reload: Arc<AtomicBool>) -> Self {
        Self {
            state_path,
            reload,
            write_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}

async fn auth_middleware(
    State(master_pubkey): State<String>,
    req: Request,
    next: Next,
) -> Response {
    let token = super::auth::extract_bearer(req.headers());
    match token {
        Some(t) => match super::auth::verify_node_jwt(&t, &master_pubkey) {
            Ok(_) => next.run(req).await,
            Err(_) => (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "JWT 验证失败" })),
            )
                .into_response(),
        },
        None => (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "缺少 Authorization 头" })),
        )
            .into_response(),
    }
}

pub fn router(master_pubkey: String, state_path: String, reload: Arc<AtomicBool>) -> Router {
    let state = NodeHandlerState::new(state_path, reload);

    let api = Router::new()
        .route("/api/status", get(handle_status))
        .route("/api/rules", get(handle_get_rules))
        .route("/api/rules", put(handle_put_rules))
        .route("/api/rules/forward", post(handle_add_forward))
        .route("/api/rules/forward/:idx", delete(handle_del_forward))
        .route("/api/rules/block", post(handle_add_block))
        .route("/api/rules/block/:idx", delete(handle_del_block))
        .route("/api/stats", get(handle_stats))
        .route("/api/reload", post(handle_reload))
        .with_state(state)
        .layer(middleware::from_fn_with_state(master_pubkey, auth_middleware));

    Router::new()
        .merge(api)
        .layer(CorsLayer::permissive())
}

fn notify_reload(state: &NodeHandlerState) {
    state.reload.store(true, Ordering::Relaxed);
}

fn err_response(status: StatusCode, msg: String) -> Response {
    (status, Json(json!({ "error": msg }))).into_response()
}

async fn handle_status() -> impl IntoResponse {
    Json(json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "mode": "node"
    }))
}

async fn handle_get_rules(State(state): State<NodeHandlerState>) -> Response {
    match node_state::load(&state.state_path) {
        Ok(s) => Json(json!({
            "ok": true,
            "forward": s.forward,
            "block": s.block,
            "revision": s.revision,
        }))
        .into_response(),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn mutate<F>(state: &NodeHandlerState, f: F) -> Response
where
    F: FnOnce(&mut node_state::NodeState) -> Result<(), (StatusCode, String)>,
{
    let _guard = state.write_lock.lock().await;
    let mut s = match node_state::load(&state.state_path) {
        Ok(s) => s,
        Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    if let Err((code, msg)) = f(&mut s) {
        return err_response(code, msg);
    }

    s.revision = s.revision.wrapping_add(1);

    match node_state::save(&state.state_path, &s) {
        Ok(_) => {
            notify_reload(state);
            Json(json!({ "ok": true, "revision": s.revision })).into_response()
        }
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn handle_put_rules(
    State(state): State<NodeHandlerState>,
    Json(body): Json<Value>,
) -> Response {
    mutate(&state, |s| {
        if let Some(fwd) = body.get("forward") {
            let rules: Vec<config::ForwardRule> = serde_json::from_value(fwd.clone())
                .map_err(|e| (StatusCode::BAD_REQUEST, format!("forward 解析失败: {}", e)))?;
            s.forward = rules;
        }
        if let Some(blk) = body.get("block") {
            let rules: Vec<config::BlockRule> = serde_json::from_value(blk.clone())
                .map_err(|e| (StatusCode::BAD_REQUEST, format!("block 解析失败: {}", e)))?;
            s.block = rules;
        }
        Ok(())
    })
    .await
}

async fn handle_add_forward(
    State(state): State<NodeHandlerState>,
    Json(rule): Json<config::ForwardRule>,
) -> Response {
    mutate(&state, |s| { s.forward.push(rule); Ok(()) }).await
}

async fn handle_del_forward(
    State(state): State<NodeHandlerState>,
    Path(idx): Path<usize>,
) -> Response {
    mutate(&state, |s| {
        if idx >= s.forward.len() {
            return Err((StatusCode::BAD_REQUEST, format!("索引 {} 超出范围", idx)));
        }
        s.forward.remove(idx);
        Ok(())
    })
    .await
}

async fn handle_add_block(
    State(state): State<NodeHandlerState>,
    Json(rule): Json<config::BlockRule>,
) -> Response {
    mutate(&state, |s| { s.block.push(rule); Ok(()) }).await
}

async fn handle_del_block(
    State(state): State<NodeHandlerState>,
    Path(idx): Path<usize>,
) -> Response {
    mutate(&state, |s| {
        if idx >= s.block.len() {
            return Err((StatusCode::BAD_REQUEST, format!("索引 {} 超出范围", idx)));
        }
        s.block.remove(idx);
        Ok(())
    })
    .await
}

async fn handle_stats() -> impl IntoResponse {
    match std::fs::read_to_string("/tmp/relay-rs.stats") {
        Ok(content) => match serde_json::from_str::<Value>(&content) {
            Ok(v) => Json(v).into_response(),
            Err(_) => Json(json!({})).into_response(),
        },
        Err(_) => Json(json!({})).into_response(),
    }
}

async fn handle_reload(State(state): State<NodeHandlerState>) -> impl IntoResponse {
    notify_reload(&state);
    Json(json!({ "ok": true }))
}
