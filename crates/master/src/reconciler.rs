//! desired state reconciler：监听 PG NOTIFY 并 kick 对应 NodeSession 重推 FullSync。
//!
//! 设计：
//!   - 全局共享 `SessionRegistry`：`node_id → mpsc::Sender<()>` ("kick" 通道)
//!   - `NodeSession::run` 启动时 `register(node_id, tx)`，结束时 `deregister`
//!   - `spawn_listener(pool)` 启一个长驻 task：
//!       - 建立独立的 PgListener，LISTEN `v1_node_desired_changed`
//!       - 收到 notify → payload 就是 node_id → 在 registry 里查 tx → send(())
//!   - NodeSession 的 event_loop select! 上 kick_rx，一收到就 push 新 FullSync
//!
//! PG LISTEN/NOTIFY 保证：
//!   - NOTIFY 必须在事务 COMMIT 之后执行（admin.rs 已经这样做）
//!   - LISTEN 端不会错过已 COMMIT 的 notify（PG 内部队列化）
//!   - payload 限制 8000 字节 —— 我们只发 node_id（<256 字节，远低于限制）

use anyhow::{Context, Result};
use sqlx::postgres::{PgListener, PgPool};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

pub type KickSender = mpsc::Sender<()>;
pub type Registry = Arc<RwLock<HashMap<String, KickSender>>>;

pub fn new_registry() -> Registry {
    Arc::new(RwLock::new(HashMap::new()))
}

pub async fn register(reg: &Registry, node_id: &str, tx: KickSender) {
    let mut g = reg.write().await;
    // 如有旧流的 sender，直接覆盖 —— 旧 session 的 kick_rx 会被 drop 时自然中止
    g.insert(node_id.to_string(), tx);
}

pub async fn deregister(reg: &Registry, node_id: &str) {
    let mut g = reg.write().await;
    g.remove(node_id);
}

/// 长驻 task：LISTEN `v1_node_desired_changed`，把 payload 作为 node_id 去 registry 找 sender kick。
pub fn spawn_listener(pool: Arc<PgPool>, reg: Registry) {
    tokio::spawn(async move {
        let mut backoff = std::time::Duration::from_secs(1);
        loop {
            match listen_loop(&pool, &reg).await {
                Ok(()) => {
                    log::info!("reconciler listener 正常退出，1s 后重启");
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    backoff = std::time::Duration::from_secs(1);
                }
                Err(e) => {
                    log::warn!("reconciler listener 异常: {:#}；{:?} 后重试", e, backoff);
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(std::time::Duration::from_secs(30));
                }
            }
        }
    });
}

async fn listen_loop(pool: &PgPool, reg: &Registry) -> Result<()> {
    let mut listener = PgListener::connect_with(pool)
        .await
        .context("PgListener::connect_with 失败")?;
    listener
        .listen("v1_node_desired_changed")
        .await
        .context("LISTEN v1_node_desired_changed 失败")?;
    log::info!("reconciler 已订阅 v1_node_desired_changed");

    loop {
        let n = listener.recv().await.context("PgListener::recv")?;
        let node_id = n.payload().to_string();
        if node_id.is_empty() {
            log::warn!("收到空 payload 的 notify，忽略");
            continue;
        }
        let maybe_tx = { reg.read().await.get(&node_id).cloned() };
        match maybe_tx {
            Some(tx) => match tx.try_send(()) {
                Ok(()) => log::info!("reconciler: kick {} 成功", node_id),
                Err(e) => log::info!("reconciler: {} kick 队列满或关闭 ({})", node_id, e),
            },
            None => log::info!("reconciler: {} 暂无活动 session，忽略", node_id),
        }
    }
}
