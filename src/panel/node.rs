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

use crate::config;

/// node 模式共享状态
#[derive(Clone)]
struct NodeState {
    config_path: String,
}

/// Ed25519 JWT 认证中间件（验证主控签发的短效 token）
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

/// 构建 node 模式路由
pub fn router(master_pubkey: String, config_path: String) -> Router {
    let state = NodeState { config_path };

    let api = Router::new()
        .route("/api/status", get(handle_status))
        .route("/api/rules", get(handle_get_rules))
        .route("/api/rules", put(handle_put_rules))
        .route("/api/rules/forward", post(handle_add_forward))
        .route("/api/rules/forward/{idx}", delete(handle_del_forward))
        .route("/api/rules/block", post(handle_add_block))
        .route("/api/rules/block/{idx}", delete(handle_del_block))
        .route("/api/stats", get(handle_stats))
        .route("/api/reload", post(handle_reload))
        .with_state(state)
        .layer(middleware::from_fn_with_state(master_pubkey, auth_middleware));

    Router::new()
        .merge(api)
        .layer(CorsLayer::permissive())
}

/// GET /api/status
async fn handle_status() -> impl IntoResponse {
    Json(json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "mode": "node"
    }))
}

/// GET /api/rules
async fn handle_get_rules(State(state): State<NodeState>) -> Response {
    match config::load(&state.config_path) {
        Ok(cfg) => Json(json!({
            "ok": true,
            "forward": cfg.forward,
            "block": cfg.block
        }))
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// PUT /api/rules — body: { forward, block }
async fn handle_put_rules(
    State(state): State<NodeState>,
    Json(body): Json<Value>,
) -> Response {
    let mut cfg = match config::load(&state.config_path) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    if let Some(fwd) = body.get("forward") {
        match serde_json::from_value::<Vec<config::ForwardRule>>(fwd.clone()) {
            Ok(rules) => cfg.forward = rules,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("forward 解析失败: {}", e) })),
                )
                    .into_response();
            }
        }
    }

    if let Some(blk) = body.get("block") {
        match serde_json::from_value::<Vec<config::BlockRule>>(blk.clone()) {
            Ok(rules) => cfg.block = rules,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("block 解析失败: {}", e) })),
                )
                    .into_response();
            }
        }
    }

    match config::save(&cfg, &state.config_path) {
        Ok(_) => {
            trigger_reload();
            Json(json!({ "ok": true })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /api/rules/forward — 追加一条转发规则
async fn handle_add_forward(
    State(state): State<NodeState>,
    Json(rule): Json<config::ForwardRule>,
) -> Response {
    let mut cfg = match config::load(&state.config_path) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    cfg.forward.push(rule);

    match config::save(&cfg, &state.config_path) {
        Ok(_) => {
            trigger_reload();
            Json(json!({ "ok": true })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// DELETE /api/rules/forward/:idx — 删除第 idx 条（0-based）
async fn handle_del_forward(
    State(state): State<NodeState>,
    Path(idx): Path<usize>,
) -> Response {
    let mut cfg = match config::load(&state.config_path) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    if idx >= cfg.forward.len() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("索引 {} 超出范围", idx) })),
        )
            .into_response();
    }

    cfg.forward.remove(idx);

    match config::save(&cfg, &state.config_path) {
        Ok(_) => {
            trigger_reload();
            Json(json!({ "ok": true })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /api/rules/block — 追加一条封锁规则
async fn handle_add_block(
    State(state): State<NodeState>,
    Json(rule): Json<config::BlockRule>,
) -> Response {
    let mut cfg = match config::load(&state.config_path) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    cfg.block.push(rule);

    match config::save(&cfg, &state.config_path) {
        Ok(_) => {
            trigger_reload();
            Json(json!({ "ok": true })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// DELETE /api/rules/block/:idx — 删除第 idx 条封锁规则（0-based）
async fn handle_del_block(
    State(state): State<NodeState>,
    Path(idx): Path<usize>,
) -> Response {
    let mut cfg = match config::load(&state.config_path) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    if idx >= cfg.block.len() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("索引 {} 超出范围", idx) })),
        )
            .into_response();
    }

    cfg.block.remove(idx);

    match config::save(&cfg, &state.config_path) {
        Ok(_) => {
            trigger_reload();
            Json(json!({ "ok": true })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /api/stats — 读取 /tmp/relay-rs.stats 的 JSON
async fn handle_stats() -> impl IntoResponse {
    match std::fs::read_to_string("/tmp/relay-rs.stats") {
        Ok(content) => match serde_json::from_str::<Value>(&content) {
            Ok(v) => Json(v).into_response(),
            Err(_) => Json(json!({})).into_response(),
        },
        Err(_) => Json(json!({})).into_response(),
    }
}

/// POST /api/reload — 向 relay-rs systemd 服务发送 SIGHUP
async fn handle_reload() -> impl IntoResponse {
    trigger_reload();
    Json(json!({ "ok": true }))
}

/// 向 relay-rs systemd 服务发送 SIGHUP 触发热重载
fn trigger_reload() {
    let _ = std::process::Command::new("systemctl")
        .args(["kill", "-s", "SIGHUP", "relay-rs"])
        .status();
}
