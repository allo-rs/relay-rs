//! Sync 双向流客户端：维护到 master 的长连接，接收 FullSync、回 Ack、发 Hello。
//!
//! 主循环（M3 初版）：
//!   1. 建立 mTLS gRPC 连接（client cert = node.pem；server CA = ca.pem）
//!   2. open_bidirectional_stream(Sync)
//!   3. 发 Hello（session_epoch / applied_revision / applied_hash / stream_id / version）
//!   4. 循环接收 MasterToNode：
//!      - FullSync: 交给 Applier → 拿到 per_segment + actual_hash → 回 Ack
//!      - Ping: 回一个 Metrics 空消息作为 heartbeat（M4.5 再填真实指标）
//!      - Shutdown: 记日志退出，等 systemd 拉起
//!      - CaBundleUpdate: 暂忽略（M3+1）
//!   5. 连接丢失 → 指数 backoff 重连（1s, 2s, 4s … 上限 30s）
//!
//! 注意：不做本地 revision 去重 —— 相同 hash 的 FullSync 仍会走一遍 Applier（幂等）
//! 以便捕获 listener 被外力杀掉后重建的情况。Applier 内部 diff 保证无 churn。

use anyhow::{Context, Result, anyhow};
use relay_proto::{
    envelope_hash,
    v1::{
        control_plane_client::ControlPlaneClient, master_to_node, node_to_master, FullSync,
        Hello, Metrics, NodeToMaster, SegmentAck,
    },
};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use tonic::transport::{Certificate, ClientTlsConfig, Endpoint, Identity};
use uuid::Uuid;

use crate::apply::Applier;
use crate::cert;
use crate::state::{self, NodeState};

pub struct SyncCfg {
    pub master_addr: String,
    pub state_dir: std::path::PathBuf,
    pub version: String,
}

pub async fn run(cfg: SyncCfg) -> Result<()> {
    let applier = Arc::new(Applier::new());
    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(30);

    loop {
        match run_once(&cfg, applier.clone()).await {
            Ok(()) => {
                log::info!("Sync 流正常结束，1s 后重连");
                tokio::time::sleep(Duration::from_secs(1)).await;
                backoff = Duration::from_secs(1);
            }
            Err(e) => {
                log::warn!("Sync 连接失败: {:#}；{:?} 后重试", e, backoff);
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(max_backoff);
            }
        }
    }
}

async fn run_once(cfg: &SyncCfg, applier: Arc<Applier>) -> Result<()> {
    let channel = connect(&cfg.master_addr, &cfg.state_dir).await?;
    let mut client = ControlPlaneClient::new(channel);

    // 本地状态
    let mut st = state::load(&cfg.state_dir)?;
    let ca_bundle_version = read_ca_version(&cfg.state_dir).unwrap_or(1);
    let stream_id = Uuid::new_v4().to_string();

    log::info!(
        "Sync 启动：session_epoch={}, applied_revision={}, stream_id={}",
        st.session_epoch, st.applied_revision, stream_id
    );

    // 出站通道：我们发给 master 的消息
    let (tx, rx) = mpsc::channel::<NodeToMaster>(32);

    // Hello 作为首条
    tx.send(NodeToMaster {
        msg: Some(node_to_master::Msg::Hello(Hello {
            stream_id: stream_id.clone(),
            session_epoch: st.session_epoch,
            applied_revision: st.applied_revision,
            applied_hash: st.applied_hash.clone(),
            ca_bundle_version,
            version: cfg.version.clone(),
        })),
    })
    .await
    .map_err(|_| anyhow!("outbound 通道开局即关"))?;

    let outbound = ReceiverStream::new(rx);
    let resp = client.sync(outbound).await.context("打开 Sync 流失败")?;
    let mut inbound = resp.into_inner();

    while let Some(msg) = inbound.next().await {
        let m = msg.context("入站流错误")?;
        let inner = match m.msg {
            Some(x) => x,
            None => continue,
        };
        match inner {
            master_to_node::Msg::FullSync(FullSync { revision, segments }) => {
                log::info!(
                    "收到 FullSync revision={} segments={}",
                    revision,
                    segments.len()
                );
                let desired_hash = envelope_hash(&segments, ca_bundle_version);
                let outcome = applier.apply_full_sync(segments).await;

                let actual_hash = envelope_hash(&outcome.actual_segments, ca_bundle_version);
                let ok_count = outcome.per_segment.values().filter(|r| r.ok).count();
                let total = outcome.per_segment.len();
                log::info!(
                    "apply 完成 {}/{}；actual_hash=0x{} desired=0x{}",
                    ok_count,
                    total,
                    hex::encode(&actual_hash[..actual_hash.len().min(8)]),
                    hex::encode(&desired_hash[..desired_hash.len().min(8)])
                );

                // 持久化
                st.applied_revision = revision;
                st.applied_hash = desired_hash.clone();
                st.actual_hash = actual_hash.clone();
                st.segments.clear();
                for s in &outcome.actual_segments {
                    st.segments.insert(
                        s.id.clone(),
                        crate::state::SegmentSnapshot {
                            listen: s.listen.clone(),
                            proto: s.proto,
                            applied_ok: true,
                        },
                    );
                }
                st.apply_errors.clear();
                for (id, r) in &outcome.per_segment {
                    if !r.ok {
                        st.apply_errors.insert(id.clone(), r.error.clone());
                    }
                }
                if let Err(e) = state::save(&cfg.state_dir, &st) {
                    log::warn!("state.save 失败（继续运行）: {:#}", e);
                }

                tx.send(NodeToMaster {
                    msg: Some(node_to_master::Msg::Ack(SegmentAck {
                        stream_id: stream_id.clone(),
                        revision,
                        desired_hash,
                        actual_hash,
                        actual_segment_ids: outcome.actual_ids,
                        per_segment: outcome.per_segment,
                    })),
                })
                .await
                .map_err(|_| anyhow!("Ack 无法送出（出站已关）"))?;
            }
            master_to_node::Msg::Ping(_) => {
                // 回一个空 Metrics 作为心跳证据
                let _ = tx
                    .send(NodeToMaster {
                        msg: Some(node_to_master::Msg::Metrics(Metrics {
                            at_ms: chrono::Utc::now().timestamp_millis() as u64,
                            per_segment: vec![],
                            udp: None,
                        })),
                    })
                    .await;
            }
            master_to_node::Msg::Shutdown(s) => {
                log::info!("master 要求 shutdown: {}；进入重连循环", s.reason);
                break;
            }
            master_to_node::Msg::CaUpdate(u) => {
                log::info!("收到 CaBundleUpdate v{}（M3 暂不热更，需重启 node）", u.version);
            }
        }
    }
    Ok(())
}

async fn connect(
    master_addr: &str,
    state_dir: &Path,
) -> Result<tonic::transport::Channel> {
    let paths = cert::paths(state_dir);
    let cert_pem = std::fs::read(&paths.cert)
        .with_context(|| format!("读 {:?}", paths.cert))?;
    let key_pem = std::fs::read(&paths.key)
        .with_context(|| format!("读 {:?}", paths.key))?;
    let ca_pem = std::fs::read(&paths.ca)
        .with_context(|| format!("读 {:?}", paths.ca))?;

    let domain = Endpoint::from_shared(master_addr.to_string())?
        .uri()
        .host()
        .unwrap_or("")
        .to_string();

    let tls = ClientTlsConfig::new()
        .ca_certificate(Certificate::from_pem(ca_pem))
        .identity(Identity::from_pem(cert_pem, key_pem))
        .domain_name(domain);

    let ep = Endpoint::from_shared(master_addr.to_string())?
        .tls_config(tls)?
        .keep_alive_while_idle(true)
        .http2_keep_alive_interval(Duration::from_secs(20))
        .keep_alive_timeout(Duration::from_secs(10));
    let ch = ep.connect().await.context("连接 master（Sync）失败")?;
    Ok(ch)
}

fn read_ca_version(dir: &Path) -> Option<u32> {
    let p = cert::paths(dir).ca_version;
    std::fs::read_to_string(p).ok().and_then(|s| s.trim().parse().ok())
}

#[allow(dead_code)]
pub fn snapshot_state(st: &NodeState) -> String {
    format!(
        "rev={} applied_hash=0x{} actual_hash=0x{} errors={}",
        st.applied_revision,
        hex::encode(&st.applied_hash),
        hex::encode(&st.actual_hash),
        st.apply_errors.len()
    )
}
