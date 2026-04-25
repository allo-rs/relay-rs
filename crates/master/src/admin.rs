//! 运维用 CLI 子命令：segment / node 的增删查，直接读写 v1 DB。
//!
//! 这些命令**不走 gRPC**，直接连 DATABASE_URL：
//!   - 允许 master daemon 不在时也能做数据维护
//!   - 修改后发 `NOTIFY v1_node_desired_changed, '<node_id>'`，
//!     运行中的 master daemon 会触发 reconciler 立刻重推 FullSync

#![allow(clippy::type_complexity)]

use anyhow::{Context, Result, anyhow};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::db;

/// 节点列表行（CLI 与 panel API 共用）。
#[derive(Debug, Clone, Serialize)]
pub struct NodeRow {
    pub id: String,
    pub name: String,
    pub status: String,
    pub session_epoch: i64,
    pub desired_revision: i64,
    pub applied_revision: i64,
    pub last_seen: Option<chrono::DateTime<chrono::Utc>>,
}

/// Segment 列表行（CLI 与 panel API 共用）。
#[derive(Debug, Clone, Serialize)]
pub struct SegmentRow {
    pub id: String,
    pub chain_id: String,
    pub node_id: String,
    pub listen: String,
    pub proto: String,
    pub next_kind: String,
    pub next_segment_id: Option<String>,
    pub upstream_host: Option<String>,
    pub upstream_port_start: Option<i32>,
    pub upstream_port_end: Option<i32>,
    pub comment: Option<String>,
}

/// 直接查询 v1_nodes，供 panel API 与 CLI 共用。
pub async fn node_list_rows(pool: &PgPool) -> Result<Vec<NodeRow>> {
    let rows: Vec<(
        String,
        String,
        String,
        i64,
        i64,
        i64,
        Option<chrono::DateTime<chrono::Utc>>,
    )> = sqlx::query_as(
        "SELECT id, name, status::text, session_epoch, desired_revision, applied_revision, last_seen
           FROM v1_nodes ORDER BY enrolled_at",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name, status, session_epoch, desired_revision, applied_revision, last_seen)| {
            NodeRow {
                id,
                name,
                status,
                session_epoch,
                desired_revision,
                applied_revision,
                last_seen,
            }
        })
        .collect())
}

/// 删除一个节点（含其所有 segments）；返回是否删除成功。
pub async fn node_rm_inner(pool: &PgPool, id: &str) -> Result<bool> {
    let r = sqlx::query("DELETE FROM v1_nodes WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn node_list() -> Result<()> {
    let pool = connect().await?;
    let rows = node_list_rows(&pool).await?;
    if rows.is_empty() {
        println!("(没有已注册节点)");
        return Ok(());
    }
    println!(
        "{:<44} {:<20} {:<10} {:>6} {:>6} last_seen",
        "id", "name", "status", "desired", "applied"
    );
    for r in rows {
        let seen = r
            .last_seen
            .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "-".to_string());
        println!(
            "{:<44} {:<20} {:<10} {:>6} {:>6} {}",
            r.id, r.name, r.status, r.desired_revision, r.applied_revision, seen
        );
    }
    Ok(())
}

pub async fn node_rm(id: &str) -> Result<()> {
    let pool = connect().await?;
    if !node_rm_inner(&pool, id).await? {
        return Err(anyhow!("节点 {} 不存在", id));
    }
    println!("✓ 已删除 {}（及其所有 segments）", id);
    Ok(())
}

pub async fn seg_list_rows(pool: &PgPool, node: Option<&str>) -> Result<Vec<SegmentRow>> {
    let rows: Vec<(
        String,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<i32>,
        Option<i32>,
        Option<String>,
    )> = if let Some(n) = node {
        sqlx::query_as(
            "SELECT id, chain_id, listen_node_id, listen, proto::text, next_kind::text,
                    next_segment_id, upstream_host, upstream_port_start, upstream_port_end,
                    comment
               FROM v1_segments WHERE listen_node_id = $1 ORDER BY chain_id, id",
        )
        .bind(n)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT id, chain_id, listen_node_id, listen, proto::text, next_kind::text,
                    next_segment_id, upstream_host, upstream_port_start, upstream_port_end,
                    comment
               FROM v1_segments ORDER BY chain_id, id",
        )
        .fetch_all(pool)
        .await?
    };

    Ok(rows
        .into_iter()
        .map(
            |(id, chain_id, node_id, listen, proto, next_kind, next_segment_id,
              upstream_host, upstream_port_start, upstream_port_end, comment)| SegmentRow {
                id,
                chain_id,
                node_id,
                listen,
                proto,
                next_kind,
                next_segment_id,
                upstream_host,
                upstream_port_start,
                upstream_port_end,
                comment,
            },
        )
        .collect())
}

pub async fn seg_list(node: Option<String>) -> Result<()> {
    let pool = connect().await?;
    let rows = seg_list_rows(&pool, node.as_deref()).await?;
    if rows.is_empty() {
        println!("(无 segment)");
        return Ok(());
    }
    println!(
        "{:<40} {:<14} {:<44} {:<10} {:<5} next",
        "id", "chain", "node", "listen", "proto"
    );
    for r in rows {
        let next = match r.next_kind.as_str() {
            "node" => format!("node:{}", r.next_segment_id.unwrap_or_default()),
            "upstream" => format!(
                "{}:{}",
                r.upstream_host.unwrap_or_default(),
                r.upstream_port_start.unwrap_or(0)
            ),
            _ => "?".to_string(),
        };
        println!(
            "{:<40} {:<14} {:<44} {:<10} {:<5} {}",
            r.id, r.chain_id, r.node_id, r.listen, r.proto, next
        );
    }
    Ok(())
}

/// `seg_add_inner` 的输入；用结构体而非长参数列表避免 `too_many_arguments`。
pub struct SegAddSpec<'a> {
    pub node_id: &'a str,
    pub listen: &'a str,
    pub upstream: &'a str,
    pub chain: Option<String>,
    pub proto: &'a str,
    pub ipv6: bool,
    pub comment: Option<String>,
}

/// 真正的 segment 插入逻辑；返回新 segment 的 id 与 chain_id。
pub async fn seg_add_inner(pool: &PgPool, spec: SegAddSpec<'_>) -> Result<(String, String)> {
    let (host, port_str) = spec
        .upstream
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("upstream 需 host:port 形式"))?;
    let port: i32 = port_str
        .parse()
        .with_context(|| format!("端口 {} 无法解析", port_str))?;
    if !matches!(spec.proto, "tcp" | "udp" | "all") {
        return Err(anyhow!("proto 必须是 tcp/udp/all"));
    }

    let listen_port: &str = if let Some((_, p)) = spec.listen.rsplit_once(':') {
        p
    } else {
        spec.listen
    };
    listen_port
        .parse::<u16>()
        .with_context(|| format!("listen 端口 {} 无法解析", listen_port))?;

    if !db::node_exists(pool, spec.node_id).await? {
        return Err(anyhow!("节点 {} 不存在", spec.node_id));
    }

    let seg_id = format!("seg-{}", Uuid::new_v4());
    let chain_id = spec
        .chain
        .clone()
        .unwrap_or_else(|| format!("chain-{}", &seg_id[4..12]));

    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO v1_segments (id, chain_id, listen_node_id, listen, proto, ipv6,
                                  next_kind, upstream_host, upstream_port_start, upstream_port_end,
                                  balance, comment)
         VALUES ($1, $2, $3, $4, $5::v1_proto, $6, 'upstream', $7, $8, $8, 'round_robin', $9)",
    )
    .bind(&seg_id)
    .bind(&chain_id)
    .bind(spec.node_id)
    .bind(listen_port)
    .bind(spec.proto)
    .bind(spec.ipv6)
    .bind(host)
    .bind(port)
    .bind(spec.comment)
    .execute(&mut *tx)
    .await
    .context("INSERT v1_segments 失败")?;

    sqlx::query(
        "UPDATE v1_nodes SET desired_revision = desired_revision + 1, updated_at = NOW()
          WHERE id = $1",
    )
    .bind(spec.node_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    sqlx::query("SELECT pg_notify('v1_node_desired_changed', $1)")
        .bind(spec.node_id)
        .execute(pool)
        .await?;

    Ok((seg_id, chain_id))
}

pub async fn seg_add(
    node_id: &str,
    listen: &str,
    upstream: &str,
    chain: Option<String>,
    proto: &str,
    ipv6: bool,
    comment: Option<String>,
) -> Result<()> {
    let pool = connect().await?;
    let (seg_id, chain_id) = seg_add_inner(
        &pool,
        SegAddSpec {
            node_id,
            listen,
            upstream,
            chain,
            proto,
            ipv6,
            comment,
        },
    )
    .await?;
    println!("✓ 新增 segment {}", seg_id);
    println!(
        "  chain={} node={} listen={} → {}",
        chain_id, node_id, listen, upstream
    );
    println!("  （已触发 reconciler 推送）");
    Ok(())
}

/// 删除 segment；返回归属的 node_id（用于通知）；不存在时返回 None。
pub async fn seg_rm_inner(pool: &PgPool, id: &str) -> Result<Option<String>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT listen_node_id FROM v1_segments WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    let node_id = match row {
        Some((n,)) => n,
        None => return Ok(None),
    };
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM v1_segments WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE v1_nodes SET desired_revision = desired_revision + 1, updated_at = NOW()
          WHERE id = $1",
    )
    .bind(&node_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    sqlx::query("SELECT pg_notify('v1_node_desired_changed', $1)")
        .bind(&node_id)
        .execute(pool)
        .await?;
    Ok(Some(node_id))
}

pub async fn seg_rm(id: &str) -> Result<()> {
    let pool = connect().await?;
    if seg_rm_inner(&pool, id).await?.is_none() {
        return Err(anyhow!("segment {} 不存在", id));
    }
    println!("✓ 已删除 segment {}", id);
    Ok(())
}

async fn connect() -> Result<PgPool> {
    let url = std::env::var("DATABASE_URL")
        .context("需要 DATABASE_URL 环境变量")?;
    db::connect(&url).await
}

pub async fn discourse_set(url: &str, secret: &str) -> Result<()> {
    let pool = connect().await?;
    let value = serde_json::json!({ "url": url, "secret": secret });
    crate::panel::settings::put_setting(&pool, "discourse", &value).await?;
    println!("✓ Discourse SSO 配置已写入 v1_settings.discourse (url={})", url);
    println!("  master daemon 会在 30s 内自动重载，或重启 relay-master 立即生效");
    Ok(())
}

pub async fn discourse_unset() -> Result<()> {
    let pool = connect().await?;
    sqlx::query("DELETE FROM v1_settings WHERE key = 'discourse'")
        .execute(&pool)
        .await
        .context("删除 v1_settings.discourse 失败")?;
    println!("✓ Discourse SSO 配置已清除");
    Ok(())
}
