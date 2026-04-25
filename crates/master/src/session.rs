//! 单个 Sync 双向流的会话处理。
//!
//! 生命周期：
//!   1. `NodeSession::serve` 被 `ControlService::sync` 调用，拿到 peer cert + inbound stream
//!   2. 从 peer cert 提取 node_id（mTLS 身份真相源）
//!   3. `begin_session` 原子分配 conn_gen
//!   4. 等首条 Hello，记录 applied_revision/session_epoch
//!   5. 推一次 FullSync（M3 第一版 always FullSync）
//!   6. 循环：收 Ack → CAS 写回；收 Heartbeat → touch last_seen；每 30s 主动 Ping
//!   7. 流结束 / 错误 → CAS 标 offline
//!
//! rubber-duck 关键约束：
//!   - 所有 DB 写回带 `WHERE conn_gen = ?` 防旧流污染
//!   - 识别信任 **peer cert**，绝不信 Hello.node_id
//!   - M3 不做 DeltaApply

use anyhow::{Context, Result, anyhow};
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tonic::{Status, Streaming};

use relay_proto::v1::{
    master_to_node, node_to_master, FullSync, MasterToNode, NodeToMaster, Ping, Segment,
    Shutdown,
};

use crate::db::{self, NodeStatus};
use crate::reconciler::{self, Registry};
use relay_proto::envelope_hash;

/// 从 peer cert 的 CN 抽 node_id。tonic/tokio-rustls 的 peer cert 在请求 extensions 里。
pub fn node_id_from_peer_cert(cert_der: &[u8]) -> Result<String> {
    use x509_parser::prelude::*;
    let (_, cert) =
        X509Certificate::from_der(cert_der).context("解析 peer cert DER 失败")?;
    let cn = cert
        .subject()
        .iter_common_name()
        .next()
        .ok_or_else(|| anyhow!("peer cert subject 缺 CN"))?
        .as_str()
        .context("CN 不是 UTF-8")?
        .to_string();
    if !cn.starts_with("node-") {
        return Err(anyhow!("peer cert CN 不是合法 node_id: {}", cn));
    }
    Ok(cn)
}

pub struct NodeSession {
    pool: Arc<PgPool>,
    node_id: String,
    conn_gen: i64,
    ca_bundle_version: u32,
}

impl NodeSession {
    /// 进入会话主循环。`inbound` 是 node 发来的流，`outbound_tx` 是回 node 的 sender
    /// （由 `ControlService::sync` 包成 `Response<ReceiverStream>` 返回）。
    pub async fn run(
        pool: Arc<PgPool>,
        registry: Registry,
        node_id: String,
        mut inbound: Streaming<NodeToMaster>,
        outbound_tx: mpsc::Sender<Result<MasterToNode, Status>>,
    ) -> Result<(), Status> {
        // 等待 Hello（3 秒超时）
        let first = tokio::time::timeout(Duration::from_secs(3), inbound.next())
            .await
            .map_err(|_| Status::deadline_exceeded("未在 3s 内收到 Hello"))?
            .ok_or_else(|| Status::cancelled("流在 Hello 前关闭"))?
            .map_err(|e| Status::from_error(Box::new(e)))?;

        let hello = match first.msg {
            Some(node_to_master::Msg::Hello(h)) => h,
            _ => return Err(Status::failed_precondition("首条消息必须是 Hello")),
        };
        log::info!(
            "node {} Hello: session_epoch={}, applied_revision={}, stream_id={}",
            node_id, hello.session_epoch, hello.applied_revision, hello.stream_id
        );

        // begin_session CAS
        let (conn_gen, desired_revision, _desired_hash) = db::begin_session(
            &pool,
            &node_id,
            hello.session_epoch as i64,
            &hello.version,
            hello.ca_bundle_version as i32,
        )
        .await
        .map_err(|e| Status::internal(format!("begin_session: {}", e)))?;

        log::info!(
            "node {} 会话建立：conn_gen={}, desired_revision={}",
            node_id, conn_gen, desired_revision
        );

        // 注册 kick 通道（reconciler 会通过它触发重推）
        let (kick_tx, mut kick_rx) = mpsc::channel::<()>(4);
        reconciler::register(&registry, &node_id, kick_tx).await;

        let session = NodeSession {
            pool: pool.clone(),
            node_id: node_id.clone(),
            conn_gen,
            ca_bundle_version: hello.ca_bundle_version,
        };

        // 推首次 FullSync
        if let Err(e) = session.push_full_sync(&outbound_tx, desired_revision).await {
            log::warn!("{}: 推 FullSync 失败 {}", node_id, e);
            reconciler::deregister(&registry, &node_id).await;
            return Err(Status::internal("FullSync 发送失败"));
        }

        // 启 heartbeat task（30s 发一次 Ping）
        let (hb_outbound_tx, hb_node_id) = (outbound_tx.clone(), node_id.clone());
        let heartbeat = tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(30));
            tick.tick().await;
            loop {
                tick.tick().await;
                let ping = MasterToNode {
                    msg: Some(master_to_node::Msg::Ping(Ping {
                        server_time_ms: chrono::Utc::now().timestamp_millis() as u64,
                    })),
                };
                if hb_outbound_tx.send(Ok(ping)).await.is_err() {
                    log::debug!("{}: heartbeat 通道关闭，停止", hb_node_id);
                    break;
                }
            }
        });

        // 主循环
        let loop_result = session.event_loop(&mut inbound, &outbound_tx, &mut kick_rx).await;

        heartbeat.abort();
        reconciler::deregister(&registry, &node_id).await;

        // 标 offline（CAS 栅栏）
        match db::mark_offline(&pool, &node_id, conn_gen).await {
            Ok(1) => log::info!("{}: 会话结束，已标 offline", node_id),
            Ok(_) => log::info!("{}: 新流已接管（conn_gen fenced），跳过 offline 标记", node_id),
            Err(e) => log::warn!("{}: mark_offline 失败 {}", node_id, e),
        }

        loop_result
    }

    async fn event_loop(
        &self,
        inbound: &mut Streaming<NodeToMaster>,
        outbound_tx: &mpsc::Sender<Result<MasterToNode, Status>>,
        kick_rx: &mut mpsc::Receiver<()>,
    ) -> Result<(), Status> {
        loop {
            tokio::select! {
                // reconciler 通知：desired state 变更，重推 FullSync
                maybe_kick = kick_rx.recv() => {
                    if maybe_kick.is_none() {
                        // sender 被 deregister drop 或被新会话覆盖 → 本会话应结束
                        log::info!("{}: kick 通道关闭，会话退出", self.node_id);
                        return Ok(());
                    }
                    // 读当前 desired_revision 再推
                    match sqlx::query_as::<_, (i64,)>(
                        "SELECT desired_revision FROM v1_nodes WHERE id = $1 AND conn_gen = $2",
                    )
                    .bind(&self.node_id).bind(self.conn_gen)
                    .fetch_optional(&*self.pool).await
                    {
                        Ok(Some((rev,))) => {
                            log::info!("{}: reconciler kick → 重推 FullSync rev={}", self.node_id, rev);
                            if let Err(e) = self.push_full_sync(outbound_tx, rev).await {
                                log::warn!("{}: 重推失败 {}", self.node_id, e);
                            }
                        }
                        Ok(None) => {
                            log::info!("{}: 已被 fenced，kick 忽略", self.node_id);
                        }
                        Err(e) => log::warn!("{}: 读 desired_revision 失败 {}", self.node_id, e),
                    }
                }
                // 入站消息
                item = inbound.next() => {
                    let msg = match item {
                        Some(Ok(m)) => m,
                        Some(Err(e)) => {
                            log::info!("{}: 入站流错误 {}", self.node_id, e);
                            return Ok(());
                        }
                        None => return Ok(()),
                    };
                    let inner = match msg.msg { Some(x) => x, None => continue };
                    match inner {
                        node_to_master::Msg::Ack(ack) => self.handle_ack(ack).await?,
                        node_to_master::Msg::RequestFullSync(r) => {
                            log::info!("{}: node 请求 FullSync: {}", self.node_id, r.reason);
                            let rev = sqlx::query_as::<_, (i64,)>(
                                "SELECT desired_revision FROM v1_nodes WHERE id = $1 AND conn_gen = $2",
                            )
                            .bind(&self.node_id).bind(self.conn_gen)
                            .fetch_optional(&*self.pool).await
                            .ok().flatten().map(|(r,)| r).unwrap_or(0);
                            let _ = self.push_full_sync(outbound_tx, rev).await;
                        }
                        node_to_master::Msg::Metrics(_) | node_to_master::Msg::Log(_) => {}
                        node_to_master::Msg::Hello(_) => {
                            return Err(Status::failed_precondition("同一流内重复 Hello"));
                        }
                    }
                }
            }
        }
    }

    async fn handle_ack(&self, ack: relay_proto::v1::SegmentAck) -> Result<(), Status> {
        let ok_count = ack.per_segment.values().filter(|r| r.ok).count();
        let fail_count = ack.per_segment.len() - ok_count;
        let status = if fail_count == 0 {
            NodeStatus::Ok
        } else {
            NodeStatus::Degraded
        };

        match db::apply_ack(
            &self.pool,
            &self.node_id,
            self.conn_gen,
            ack.revision as i64,
            &ack.actual_hash,
            status,
        )
        .await
        {
            Ok(1) => {
                log::info!(
                    "{}: Ack rev={} ok={} fail={} actual_hash=0x{}",
                    self.node_id,
                    ack.revision,
                    ok_count,
                    fail_count,
                    hex::encode(&ack.actual_hash[..ack.actual_hash.len().min(8)])
                );
            }
            Ok(_) => {
                log::warn!(
                    "{}: Ack 被 fenced（我们 conn_gen={} 已非当前），丢弃",
                    self.node_id, self.conn_gen
                );
            }
            Err(e) => {
                log::error!("{}: apply_ack DB 错误 {}", self.node_id, e);
                return Err(Status::internal("apply_ack 失败"));
            }
        }
        Ok(())
    }

    async fn push_full_sync(
        &self,
        outbound_tx: &mpsc::Sender<Result<MasterToNode, Status>>,
        revision: i64,
    ) -> Result<()> {
        let rows = db::load_segments_for_node(&self.pool, &self.node_id).await?;
        let segments: Vec<Segment> = rows.into_iter().map(row_to_segment).collect();
        let hash = envelope_hash(&segments, self.ca_bundle_version);
        // 持久化本次 desired_hash，让运维能从 DB 看到；node Ack 的 actual_hash 会跟这个对齐
        if let Err(e) =
            db::set_desired_hash(&self.pool, &self.node_id, self.conn_gen, &hash).await
        {
            log::warn!("{}: set_desired_hash 失败 {}", self.node_id, e);
        }
        log::info!(
            "{}: FullSync revision={} segments={} hash=0x{}",
            self.node_id,
            revision,
            segments.len(),
            hex::encode(&hash[..8])
        );
        let full = FullSync {
            revision: revision as u64,
            segments,
        };
        outbound_tx
            .send(Ok(MasterToNode {
                msg: Some(master_to_node::Msg::FullSync(full)),
            }))
            .await
            .map_err(|_| anyhow!("outbound 通道关闭"))?;
        Ok(())
    }
}

fn row_to_segment(row: db::SegmentRow) -> Segment {
    use relay_proto::v1::{segment, Balance, NextSegment, Proto, Upstream};
    let proto = match row.proto.as_str() {
        "tcp" => Proto::Tcp as i32,
        "udp" => Proto::Udp as i32,
        "all" => Proto::All as i32,
        _ => Proto::Unspecified as i32,
    };
    let balance = match row.balance.as_str() {
        "round_robin" => Balance::RoundRobin as i32,
        "random" => Balance::Random as i32,
        "source_hash" => Balance::SourceHash as i32,
        _ => Balance::Unspecified as i32,
    };
    let next = match row.next_kind.as_str() {
        "node" => row.next_segment_id.map(|id| {
            segment::Next::NodeNext(NextSegment { segment_id: id })
        }),
        "upstream" => Some(segment::Next::Upstream(Upstream {
            host: row.upstream_host.unwrap_or_default(),
            port_start: row.upstream_port_start.unwrap_or(0) as u32,
            port_end: row.upstream_port_end.unwrap_or(0) as u32,
        })),
        _ => None,
    };

    Segment {
        id: row.id,
        chain_id: row.chain_id,
        listen_node_id: row.listen_node_id,
        listen: row.listen,
        proto,
        ipv6: row.ipv6,
        next,
        rate_limit_mbps: row.rate_limit_mbps.map(|v| v as u32),
        balance,
        comment: row.comment,
    }
}

/// 允许 `Shutdown` 消息清关。
#[allow(dead_code)]
pub fn make_shutdown(reason: &str) -> MasterToNode {
    MasterToNode {
        msg: Some(master_to_node::Msg::Shutdown(Shutdown {
            reason: reason.to_string(),
        })),
    }
}
