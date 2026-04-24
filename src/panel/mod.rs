mod assets;
pub mod auth;
mod discourse;
mod master;
mod node;
mod tls;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use axum_server::tls_rustls::RustlsConfig;

use crate::config::{PanelConfig, PanelMode};

const DEFAULT_CERT_PATH: &str = "/etc/relay-rs/panel-cert.pem";
const DEFAULT_KEY_PATH: &str = "/etc/relay-rs/panel-key.pem";

/// node 模式专用入口（v0.x 引入）：不依赖 PanelConfig / TOML，纯 env 驱动
pub async fn run_node(
    listen: SocketAddr,
    master_pubkey: String,
    state_path: String,
    reload: Arc<AtomicBool>,
) {
    if master_pubkey.trim().is_empty() {
        log::error!("node 面板需要 master_pubkey，拒绝启动");
        return;
    }
    log::info!("面板启动：node 模式，监听 {}（state: {}）", listen, state_path);
    let router = node::router(master_pubkey, state_path, reload);

    let listener = match tokio::net::TcpListener::bind(listen).await {
        Ok(l) => l,
        Err(e) => { log::error!("面板监听 {} 失败: {}", listen, e); return; }
    };
    if let Err(e) = axum::serve(listener, router).await {
        log::error!("面板服务器异常退出: {}", e);
    }
}

pub async fn run(panel_cfg: PanelConfig, config_path: String, pool: Option<sqlx::PgPool>) {
    let addr: std::net::SocketAddr = match panel_cfg.listen.parse() {
        Ok(a) => a,
        Err(e) => { log::error!("面板监听地址无效 {}: {}", panel_cfg.listen, e); return; }
    };

    let tls_cert = panel_cfg.tls_cert.clone();
    let tls_key  = panel_cfg.tls_key.clone();

    let router = match panel_cfg.mode {
        PanelMode::Node => {
            // master/CLI 场景的遗留 node 模式：仍然按 TOML 路径运行
            let pubkey = panel_cfg.master_pubkey.clone().unwrap_or_default();
            if pubkey.trim().is_empty() {
                log::error!("node 模式需要配置 master_pubkey，面板拒绝启动");
                return;
            }
            log::warn!("legacy node 模式（通过 PanelConfig）已弃用，请使用 MASTER_PUBKEY_B64 env 启动");
            // 传入一个无用的 reload 标志（该路径不再实际生效于 node 守护）
            let reload = Arc::new(AtomicBool::new(false));
            node::router(pubkey, config_path, reload)
        }
        PanelMode::Master => {
            // 优先使用调用方传入的共享连接池，避免重复建立连接
            let pool = match pool {
                Some(p) => p,
                None => {
                    let db_url = match &panel_cfg.database_url {
                        Some(u) => u.clone(),
                        None => { log::error!("master 模式需要配置 database_url"); return; }
                    };
                    match crate::db::connect(&db_url).await {
                        Ok(p) => p,
                        Err(e) => { log::error!("数据库连接失败: {}", e); return; }
                    }
                }
            };
            log::info!("面板启动：master 模式，监听 {}（PostgreSQL 就绪）", addr);
            master::router(panel_cfg, config_path, pool)
        }
    };

    // 无 TLS 配置 → HTTP
    if tls_cert.is_none() && tls_key.is_none() {
        log::info!("无 TLS 配置，以 HTTP 模式运行");
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => { log::error!("面板监听失败: {}", e); return; }
        };
        if let Err(e) = axum::serve(listener, router).await {
            log::error!("面板服务器异常退出: {}", e);
        }
        return;
    }

    // 有 TLS 配置 → HTTPS
    let cert_path = tls_cert.as_deref().unwrap_or(DEFAULT_CERT_PATH);
    let key_path  = tls_key.as_deref().unwrap_or(DEFAULT_KEY_PATH);

    let tls_files = match tls::load_or_generate(cert_path, key_path) {
        Ok(f) => f,
        Err(e) => { log::error!("TLS 证书加载失败: {}", e); return; }
    };
    let rustls_config = match RustlsConfig::from_pem(tls_files.cert_pem, tls_files.key_pem).await {
        Ok(c) => c,
        Err(e) => { log::error!("TLS 配置失败: {}", e); return; }
    };
    if let Err(e) = axum_server::bind_rustls(addr, rustls_config)
        .serve(router.into_make_service())
        .await
    {
        log::error!("面板服务器异常退出: {}", e);
    }
}
