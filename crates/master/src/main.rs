//! relay-master v1 控制面守护进程（M2 MVP）
//!
//! CLI：
//!   relay-master daemon                  启动 gRPC 服务
//!   relay-master node-add --name X       生成 enrollment token 并打印
//!   relay-master ca-show                 打印 CA bundle（base64）用于分发给 node
//!
//! 相关 env：
//!   RELAY_MASTER_CA_DIR    默认 /etc/relay-master
//!   RELAY_MASTER_LISTEN    默认 0.0.0.0:9443
//!   RELAY_MASTER_HOSTNAME  server cert SAN 列表（逗号分隔），默认 127.0.0.1,localhost

mod admin;
mod ca;
mod db;
mod panel;
mod reconciler;
mod service;
mod session;
mod token;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::fs;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use tonic::transport::{Certificate, Identity, Server, ServerTlsConfig};

use crate::ca::Ca;
use crate::service::ControlService;
use crate::token::TokenStore;

#[derive(Parser)]
#[command(name = "relay-master", about = "relay-rs v1 控制面", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 启动控制面守护进程
    Daemon,
    /// 生成一次性 enrollment token（授权新 node 注册）
    NodeAdd {
        /// node 可读名（仅用于标识）
        #[arg(long)]
        name: String,
        /// token TTL（秒），默认 86400
        #[arg(long)]
        ttl: Option<u64>,
    },
    /// 打印 CA bundle（便于 install-node.sh 分发）
    CaShow {
        /// base64 输出（systemd EnvironmentFile 友好）
        #[arg(long)]
        base64: bool,
    },
    /// 列出已注册节点
    NodeList,
    /// 删除节点（含其所有 segments，会级联）
    NodeRm {
        /// node_id
        #[arg(long)]
        id: String,
    },
    /// 新建一条转发 segment（M3 MVP: tcp + upstream 单口）
    SegAdd {
        /// 归属的节点 id
        #[arg(long)]
        node: String,
        /// 监听端口（目前仅单端口）
        #[arg(long)]
        listen: String,
        /// upstream "host:port"
        #[arg(long)]
        upstream: String,
        /// 可选 chain 名（默认随机）
        #[arg(long)]
        chain: Option<String>,
        /// 协议，默认 tcp；M3 只支持 tcp
        #[arg(long, default_value = "tcp")]
        proto: String,
        /// 强制 ipv6
        #[arg(long, default_value_t = false)]
        ipv6: bool,
        /// 说明
        #[arg(long)]
        comment: Option<String>,
    },
    /// 列出所有 segment
    SegList {
        /// 仅列出某节点的 segments
        #[arg(long)]
        node: Option<String>,
    },
    /// 删除 segment
    SegRm {
        /// segment id
        #[arg(long)]
        id: String,
    },
    /// 配置 Discourse SSO（写入 v1_settings.discourse）
    ///
    /// 出于安全考虑 secret 必须从 stdin 读取，避免 argv 泄露到 /proc/<pid>/cmdline。
    DiscourseSet {
        /// Discourse 站点 URL（如 https://forum.example.com）
        #[arg(long)]
        url: String,
        /// 从 stdin 读取 SSO secret（去掉首尾空白）
        #[arg(long, default_value_t = true)]
        secret_stdin: bool,
    },
    /// 清除 Discourse SSO 配置
    DiscourseUnset,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let _ = rustls::crypto::ring::default_provider().install_default();

    let cli = Cli::parse();
    let ca_dir = ca::default_ca_dir();
    let token_dir = token::default_token_dir();

    match cli.cmd {
        Command::Daemon => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            rt.block_on(run_daemon(&ca_dir, &token_dir))
        }
        Command::NodeAdd { name, ttl } => {
            let tokens = TokenStore::new(token_dir)?;
            let t = tokens.create(&name, ttl)?;
            println!("enrollment token: {}", t);
            let hours = ttl.unwrap_or(86400) / 3600;
            println!("(有效期 {} 小时，过期或消费后失效)", hours);
            Ok(())
        }
        Command::CaShow { base64 } => {
            let ca = Ca::load_or_create(&ca_dir)?;
            if base64 {
                use base64::Engine;
                let b64 =
                    base64::engine::general_purpose::STANDARD.encode(ca.cert_pem.as_bytes());
                println!("{}", b64);
            } else {
                print!("{}", ca.cert_pem);
            }
            Ok(())
        }
        Command::NodeList => admin_rt().block_on(admin::node_list()),
        Command::NodeRm { id } => admin_rt().block_on(admin::node_rm(&id)),
        Command::SegAdd {
            node,
            listen,
            upstream,
            chain,
            proto,
            ipv6,
            comment,
        } => admin_rt().block_on(admin::seg_add(
            &node, &listen, &upstream, chain, &proto, ipv6, comment,
        )),
        Command::SegList { node } => admin_rt().block_on(admin::seg_list(node)),
        Command::SegRm { id } => admin_rt().block_on(admin::seg_rm(&id)),
        Command::DiscourseSet { url, secret_stdin: _ } => {
            // 始终从 stdin 读 secret —— argv/--secret 一律不接，防止 /proc/cmdline 泄露
            use std::io::Read;
            let mut secret = String::new();
            std::io::stdin()
                .read_to_string(&mut secret)
                .context("从 stdin 读取 Discourse SSO secret 失败")?;
            let secret = secret.trim().to_string();
            if secret.is_empty() {
                anyhow::bail!("从 stdin 读到空 secret，已中止");
            }
            if url.trim().is_empty() {
                anyhow::bail!("--url 不能为空");
            }
            admin_rt().block_on(admin::discourse_set(&url, &secret))
        }
        Command::DiscourseUnset => admin_rt().block_on(admin::discourse_unset()),
    }
}

fn admin_rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio rt")
}

async fn run_daemon(ca_dir: &Path, token_dir: &Path) -> Result<()> {
    let ca = Arc::new(Ca::load_or_create(ca_dir)?);
    let tokens = Arc::new(TokenStore::new(token_dir.to_path_buf())?);

    let (server_cert, server_key) = ensure_server_cert(&ca, ca_dir)?;

    let database_url = std::env::var("DATABASE_URL")
        .context("必须设置 DATABASE_URL 环境变量指向 PostgreSQL")?;
    let pool = Arc::new(db::connect(&database_url).await?);
    db::migrate(&pool).await?;

    // reconciler：LISTEN v1_node_desired_changed → kick 对应 NodeSession 重推
    let registry = reconciler::new_registry();
    reconciler::spawn_listener(pool.clone(), registry.clone());

    let listen: SocketAddr = std::env::var("RELAY_MASTER_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:9443".to_string())
        .parse()
        .context("RELAY_MASTER_LISTEN 地址无效")?;

    log::info!("relay-master 启动，gRPC 监听 {}", listen);
    log::info!(
        "CA bundle v{}（{} bytes）",
        ca.bundle_version,
        ca.cert_pem.len()
    );

    // mTLS：对 Sync 要求 client cert，对 Register 不要求 → client_auth_optional
    // Sync handler 自己在 `req.peer_certs()` 为空时返回 Unauthenticated。
    let tls_config = ServerTlsConfig::new()
        .identity(Identity::from_pem(
            server_cert.as_bytes(),
            server_key.as_bytes(),
        ))
        .client_ca_root(Certificate::from_pem(ca.cert_pem.as_bytes()))
        .client_auth_optional(true);

    let svc =
        ControlService::new(ca.clone(), tokens.clone(), pool.clone(), registry.clone())
            .into_server();

    let panel_listen: SocketAddr = std::env::var("RELAY_PANEL_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:9090".to_string())
        .parse()
        .context("RELAY_PANEL_LISTEN 地址无效")?;

    // Arc<PgPool> → PgPool（PgPool 内部已是 Arc，clone 即克隆引用）
    let panel_pool = (*pool).clone();
    let panel_fut = panel::run(panel_listen, panel_pool, tokens.clone(), ca.clone());
    let grpc_fut = async {
        Server::builder()
            .tls_config(tls_config)?
            .add_service(svc)
            .serve_with_shutdown(listen, shutdown_signal())
            .await
            .context("gRPC 服务器异常退出")
    };

    tokio::try_join!(panel_fut, grpc_fut)?;

    Ok(())
}

fn ensure_server_cert(ca: &Ca, dir: &Path) -> Result<(String, String)> {
    let cert_path = dir.join("server.pem");
    let key_path = dir.join("server.key");
    if cert_path.exists() && key_path.exists() {
        return Ok((
            fs::read_to_string(&cert_path)?,
            fs::read_to_string(&key_path)?,
        ));
    }

    let sans_env = std::env::var("RELAY_MASTER_HOSTNAME")
        .unwrap_or_else(|_| "127.0.0.1,localhost".to_string());
    let sans: Vec<String> = sans_env
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    log::warn!("server cert 不存在，基于 CA 签发一张（SAN: {:?}）", sans);
    let (cert, key) = ca.issue_server_cert(&sans)?;
    fs::write(&cert_path, &cert)?;
    fs::write(&key_path, &key)?;
    Ok((cert, key))
}

pub(crate) async fn shutdown_signal() {
    let ctrl_c = async { let _ = tokio::signal::ctrl_c().await; };
    let term = async {
        #[cfg(unix)]
        {
            let mut s =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();
            s.recv().await;
        }
        #[cfg(not(unix))]
        std::future::pending::<()>().await
    };
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
    log::info!("收到关闭信号，优雅退出");
}
