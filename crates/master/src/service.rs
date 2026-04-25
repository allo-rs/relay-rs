//! ControlPlane gRPC service 实现。
//!
//! v1.0 M2 范围：Register 完整实现；Renew、Sync 返回 Unimplemented。
//! 后续里程碑：
//! - M3: Sync 实现 + stream fencing
//! - 2.1: Renew 实现（需 mTLS 认证）

use crate::ca::Ca;
use crate::reconciler::Registry;
use crate::session::{self, NodeSession};
use crate::token::TokenStore;
use relay_proto::v1::{
    RegisterReq, RegisterResp, RenewReq, RenewResp,
    control_plane_server::{ControlPlane, ControlPlaneServer},
    MasterToNode, NodeToMaster,
};
use sqlx::PgPool;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::{Stream, wrappers::ReceiverStream};
use tonic::{Request, Response, Status};
use uuid::Uuid;

pub type SyncResponseStream =
    Pin<Box<dyn Stream<Item = Result<MasterToNode, Status>> + Send + 'static>>;

pub struct ControlService {
    ca: Arc<Ca>,
    tokens: Arc<TokenStore>,
    pool: Arc<PgPool>,
    registry: Registry,
}

impl ControlService {
    pub fn new(
        ca: Arc<Ca>,
        tokens: Arc<TokenStore>,
        pool: Arc<PgPool>,
        registry: Registry,
    ) -> Self {
        Self {
            ca,
            tokens,
            pool,
            registry,
        }
    }

    pub fn into_server(self) -> ControlPlaneServer<Self> {
        ControlPlaneServer::new(self)
    }
}

#[tonic::async_trait]
impl ControlPlane for ControlService {
    async fn register(
        &self,
        req: Request<RegisterReq>,
    ) -> Result<Response<RegisterResp>, Status> {
        let r = req.into_inner();
        log::info!(
            "Register 请求 node_name={} csr_len={}",
            r.node_name,
            r.csr_pem.len()
        );

        // 1. 消费 enrollment token（一次性）
        let record = self
            .tokens
            .consume(&r.enrollment_token)
            .map_err(|e| Status::unauthenticated(format!("enrollment token 无效: {}", e)))?;

        // 2. 校验 token 绑定的 node_name 与请求一致
        if record.node_name != r.node_name {
            return Err(Status::permission_denied(format!(
                "token 绑定的 node_name 为 {}, 请求的是 {}",
                record.node_name, r.node_name
            )));
        }

        // 3. 校验 CSR 格式
        let csr_pem = String::from_utf8(r.csr_pem)
            .map_err(|e| Status::invalid_argument(format!("CSR 不是 UTF-8: {}", e)))?;
        if !csr_pem.contains("CERTIFICATE REQUEST") {
            return Err(Status::invalid_argument("CSR PEM 格式异常"));
        }

        // 4. 分配 node_id
        let node_id = format!("node-{}", Uuid::new_v4());

        // 5. 用 CA 签发
        let cert_pem = self
            .ca
            .sign_node_csr(&csr_pem, &node_id, &r.node_name)
            .map_err(|e| {
                log::error!("签发 CSR 失败: {:#}", e);
                Status::internal("签发证书失败")
            })?;

        log::info!("✓ 已为 {} ({}) 签发证书", r.node_name, node_id);

        // 在 v1_nodes 登记此节点（M3 Sync 会据此 UPDATE）
        if let Err(e) =
            crate::db::upsert_node(&self.pool, &node_id, &r.node_name, self.ca.bundle_version as i32)
                .await
        {
            log::error!("upsert_node 失败: {:#}", e);
            return Err(Status::internal("登记节点失败"));
        }

        Ok(Response::new(RegisterResp {
            node_id,
            cert_pem: cert_pem.into_bytes(),
            ca_bundle_pem: vec![self.ca.cert_pem.clone().into_bytes()],
            ca_bundle_version: self.ca.bundle_version,
        }))
    }

    async fn renew(&self, _req: Request<RenewReq>) -> Result<Response<RenewResp>, Status> {
        // TODO(M2.1)：mTLS 认证 + 签发新 cert
        Err(Status::unimplemented("Renew 将在 M2.1 实现"))
    }

    type SyncStream = SyncResponseStream;

    async fn sync(
        &self,
        req: Request<tonic::Streaming<NodeToMaster>>,
    ) -> Result<Response<Self::SyncStream>, Status> {
        // 从 mTLS peer cert 抽 node_id —— 身份唯一真相源
        let peer_certs = req
            .peer_certs()
            .ok_or_else(|| Status::unauthenticated("Sync 需要 mTLS client cert"))?;
        let first_cert = peer_certs
            .first()
            .ok_or_else(|| Status::unauthenticated("peer cert 列表为空"))?;
        let node_id = session::node_id_from_peer_cert(first_cert.as_ref())
            .map_err(|e| Status::unauthenticated(format!("peer cert 不合法: {}", e)))?;

        // 校验 node 已注册
        match crate::db::node_exists(&self.pool, &node_id).await {
            Ok(true) => {}
            Ok(false) => return Err(Status::not_found(format!("节点 {} 未注册", node_id))),
            Err(e) => {
                log::error!("node_exists 查询失败: {:#}", e);
                return Err(Status::internal("DB 错误"));
            }
        }

        let inbound = req.into_inner();
        let (tx, rx) = mpsc::channel::<Result<MasterToNode, Status>>(32);

        let pool = self.pool.clone();
        let registry = self.registry.clone();
        let node_id_for_task = node_id.clone();
        tokio::spawn(async move {
            if let Err(e) =
                NodeSession::run(pool, registry, node_id_for_task.clone(), inbound, tx.clone()).await
            {
                log::warn!("{}: NodeSession 结束 {:?}", node_id_for_task, e);
                let _ = tx.send(Err(e)).await;
            }
        });

        let stream: Self::SyncStream = Box::pin(ReceiverStream::new(rx));
        Ok(Response::new(stream))
    }
}
