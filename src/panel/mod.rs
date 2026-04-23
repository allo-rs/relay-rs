mod assets;
mod auth;
mod master;
mod node;
mod tls;

use axum_server::tls_rustls::RustlsConfig;

use crate::config::{PanelConfig, PanelMode};

const DEFAULT_CERT_PATH: &str = "/etc/relay-rs/panel-cert.pem";
const DEFAULT_KEY_PATH: &str = "/etc/relay-rs/panel-key.pem";

pub async fn run(panel_cfg: PanelConfig, config_path: String) {
    let addr: std::net::SocketAddr = match panel_cfg.listen.parse() {
        Ok(a) => a,
        Err(e) => { log::error!("面板监听地址无效 {}: {}", panel_cfg.listen, e); return; }
    };

    let tls_cert = panel_cfg.tls_cert.clone();
    let tls_key  = panel_cfg.tls_key.clone();

    let router = match panel_cfg.mode {
        PanelMode::Node => {
            log::info!("面板启动：node 模式，监听 {}", addr);
            node::router(panel_cfg.master_pubkey.clone().unwrap_or_default(), config_path)
        }
        PanelMode::Master => {
            let db_url = match &panel_cfg.database_url {
                Some(u) => u.clone(),
                None => { log::error!("master 模式需要配置 database_url"); return; }
            };
            let pool = match crate::db::connect(&db_url).await {
                Ok(p) => p,
                Err(e) => { log::error!("数据库连接失败: {}", e); return; }
            };
            if let Err(e) = crate::db::ensure_schema(&pool).await {
                log::error!("数据库初始化失败: {}", e);
                return;
            }
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
