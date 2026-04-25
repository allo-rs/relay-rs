//! relay-node v1 首次注册 & 证书管理
//!
//! CLI：
//!   relay-node register                  首次注册（需 ENROLLMENT_TOKEN + MASTER_ADDR + MASTER_CA_PEM_B64）
//!   relay-node daemon                    守护进程（M3 才实现 Sync，这里先 stub）
//!
//! Env：
//!   MASTER_ADDR            gRPC 地址（如 https://master.example.com:9443）
//!   MASTER_CA_PEM_B64      master CA bundle（base64 的 PEM），用于信任 server cert
//!   ENROLLMENT_TOKEN       首次注册 token（一次性）
//!   NODE_NAME              可读名（和 token 绑定的 name 必须一致）
//!   NODE_STATE_DIR         证书目录，默认 /var/lib/relay-node

mod apply;
mod cert;
mod state;
mod sync;

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use tonic::transport::{Certificate, ClientTlsConfig, Endpoint};

use relay_proto::v1::{control_plane_client::ControlPlaneClient, RegisterReq};

#[derive(Parser)]
#[command(name = "relay-node", about = "relay-rs v1 数据面节点", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 首次注册：用 ENROLLMENT_TOKEN 换 client cert
    Register,
    /// 守护进程（M3 实现 Sync）
    Daemon,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let _ = rustls::crypto::ring::default_provider().install_default();

    let cli = Cli::parse();
    let state_dir = std::env::var("NODE_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/var/lib/relay-node"));

    match cli.cmd {
        Command::Register => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            rt.block_on(do_register(&state_dir))
        }
        Command::Daemon => {
            if !cert::paths(&state_dir).cert.exists() {
                return Err(anyhow!(
                    "尚未注册。请先设置 ENROLLMENT_TOKEN/MASTER_ADDR/MASTER_CA_PEM_B64 后运行 `relay-node register`"
                ));
            }
            let master_addr =
                std::env::var("MASTER_ADDR").context("daemon 需要 MASTER_ADDR")?;
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            rt.block_on(async move {
                let cfg = sync::SyncCfg {
                    master_addr,
                    state_dir: state_dir.clone(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                };
                tokio::select! {
                    r = sync::run(cfg) => r,
                    _ = shutdown_signal() => {
                        log::info!("收到关闭信号，退出");
                        Ok(())
                    }
                }
            })
        }
    }
}

async fn shutdown_signal() {
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
}

async fn do_register(state_dir: &Path) -> Result<()> {
    let master_addr =
        std::env::var("MASTER_ADDR").context("缺少 MASTER_ADDR（如 https://master:9443）")?;
    let token = std::env::var("ENROLLMENT_TOKEN").context("缺少 ENROLLMENT_TOKEN")?;
    let node_name = std::env::var("NODE_NAME").context("缺少 NODE_NAME")?;
    let ca_b64 =
        std::env::var("MASTER_CA_PEM_B64").context("缺少 MASTER_CA_PEM_B64（master 的 CA PEM base64）")?;

    use base64::Engine;
    let ca_pem = base64::engine::general_purpose::STANDARD
        .decode(ca_b64.trim())
        .context("MASTER_CA_PEM_B64 base64 解码失败")?;

    // 若已有 cert 就不要重复注册（保护性）
    let paths = cert::paths(state_dir);
    if paths.cert.exists() {
        log::warn!("证书已存在 {:?}，跳过注册（如需重新注册请先删除）", paths.cert);
        return Ok(());
    }

    std::fs::create_dir_all(state_dir)?;
    let domain_name = Endpoint::from_shared(master_addr.clone())?
        .uri()
        .host()
        .unwrap_or("")
        .to_string();

    log::info!("为 {} 生成 CSR...", node_name);
    let (csr_pem, key_pem) = cert::generate_csr(&node_name)?;

    let tls = ClientTlsConfig::new()
        .ca_certificate(Certificate::from_pem(ca_pem))
        .domain_name(&domain_name);

    let endpoint = Endpoint::from_shared(master_addr.clone())?.tls_config(tls)?;
    let channel = endpoint.connect().await.context("连接 master 失败")?;

    let mut client = ControlPlaneClient::new(channel);
    let resp = client
        .register(RegisterReq {
            node_name: node_name.clone(),
            enrollment_token: token,
            csr_pem: csr_pem.into_bytes(),
        })
        .await
        .map_err(|e| anyhow!("Register 失败: {}", e))?
        .into_inner();

    log::info!(
        "✓ 注册成功：node_id={} (cert {} bytes, ca_bundle_v{})",
        resp.node_id,
        resp.cert_pem.len(),
        resp.ca_bundle_version
    );

    // 持久化
    cert::save(
        state_dir,
        &resp.node_id,
        &resp.cert_pem,
        key_pem.as_bytes(),
        &resp.ca_bundle_pem,
        resp.ca_bundle_version,
    )?;

    println!("node_id={}", resp.node_id);
    println!("证书已保存到 {:?}", state_dir);
    Ok(())
}
