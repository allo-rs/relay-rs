//! master 的 DB 访问层（schema 表名见 `migrations/`）。
//! 迁移通过 `sqlx::migrate!` 嵌入到二进制。

use anyhow::{Context, Result};
use sqlx::postgres::{PgPool, PgPoolOptions};

/// `MIGRATOR` 在编译期把 `crates/master/migrations/` 目录里所有 SQL 嵌入二进制。
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

pub async fn connect(url: &str) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(16)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(url)
        .await
        .with_context(|| format!("连接数据库 {} 失败", mask_url(url)))?;
    Ok(pool)
}

pub async fn migrate(pool: &PgPool) -> Result<()> {
    MIGRATOR.run(pool).await.context("运行 schema 迁移失败")?;
    log::info!("schema 迁移完成");
    Ok(())
}

fn mask_url(url: &str) -> String {
    // postgresql://user:pass@host/db → postgresql://user:***@host/db
    if let Some(at_pos) = url.find('@')
        && let Some(colon_pos) = url[..at_pos].rfind(':')
    {
        return format!("{}:***{}", &url[..colon_pos], &url[at_pos..]);
    }
    url.to_string()
}

// ── 节点操作 ──────────────────────────────────────────────────

/// 接入新连接：原子地分配一个递增的 conn_gen，写入最新 stream 身份。
/// 返回 (conn_gen, desired_revision, desired_hash)。
pub async fn begin_session(
    pool: &PgPool,
    node_id: &str,
    session_epoch: i64,
    version: &str,
    ca_bundle_version: i32,
) -> Result<(i64, i64, Vec<u8>)> {
    let row: (i64, i64, Vec<u8>) = sqlx::query_as(
        "UPDATE v1_nodes
            SET conn_gen          = conn_gen + 1,
                session_epoch     = $2,
                version           = $3,
                ca_bundle_version = $4,
                status            = 'ok',
                last_seen         = NOW(),
                updated_at        = NOW()
          WHERE id = $1
         RETURNING conn_gen, desired_revision, desired_hash",
    )
    .bind(node_id)
    .bind(session_epoch)
    .bind(version)
    .bind(ca_bundle_version)
    .fetch_one(pool)
    .await
    .with_context(|| format!("begin_session: 节点 {} 不存在或 DB 错误", node_id))?;
    Ok(row)
}

/// CAS 回写 Ack：只有在当前 conn_gen 仍然等于我们持有的 gen 时才更新。
/// 返回更新成功的行数（0 表示被 fenced 了，应当丢弃该 Ack）。
pub async fn apply_ack(
    pool: &PgPool,
    node_id: &str,
    conn_gen: i64,
    applied_revision: i64,
    actual_hash: &[u8],
    status: NodeStatus,
) -> Result<u64> {
    let r = sqlx::query(
        "UPDATE v1_nodes
            SET applied_revision = $3,
                actual_hash      = $4,
                status           = $5,
                last_seen        = NOW(),
                updated_at       = NOW()
          WHERE id = $1 AND conn_gen = $2",
    )
    .bind(node_id)
    .bind(conn_gen)
    .bind(applied_revision)
    .bind(actual_hash)
    .bind(status)
    .execute(pool)
    .await
    .context("apply_ack 更新失败")?;
    Ok(r.rows_affected())
}

#[allow(dead_code)]
pub async fn touch_heartbeat(pool: &PgPool, node_id: &str, conn_gen: i64) -> Result<u64> {
    let r = sqlx::query(
        "UPDATE v1_nodes SET last_seen = NOW() WHERE id = $1 AND conn_gen = $2",
    )
    .bind(node_id)
    .bind(conn_gen)
    .execute(pool)
    .await
    .context("touch_heartbeat 失败")?;
    Ok(r.rows_affected())
}

/// 记录 master 推送的 desired_hash（每次 push_full_sync 计算后写回）。
pub async fn set_desired_hash(
    pool: &PgPool,
    node_id: &str,
    conn_gen: i64,
    desired_hash: &[u8],
) -> Result<u64> {
    let r = sqlx::query(
        "UPDATE v1_nodes SET desired_hash = $3, updated_at = NOW()
          WHERE id = $1 AND conn_gen = $2",
    )
    .bind(node_id)
    .bind(conn_gen)
    .bind(desired_hash)
    .execute(pool)
    .await
    .context("set_desired_hash 失败")?;
    Ok(r.rows_affected())
}

/// 节点离线标记（CAS on conn_gen，防误覆盖新流）。
pub async fn mark_offline(pool: &PgPool, node_id: &str, conn_gen: i64) -> Result<u64> {
    let r = sqlx::query(
        "UPDATE v1_nodes SET status = 'offline', updated_at = NOW()
          WHERE id = $1 AND conn_gen = $2",
    )
    .bind(node_id)
    .bind(conn_gen)
    .execute(pool)
    .await
    .context("mark_offline 失败")?;
    Ok(r.rows_affected())
}

pub async fn node_exists(pool: &PgPool, node_id: &str) -> Result<bool> {
    let r: (i64,) = sqlx::query_as("SELECT COUNT(1) FROM v1_nodes WHERE id = $1")
        .bind(node_id)
        .fetch_one(pool)
        .await?;
    Ok(r.0 > 0)
}

/// 首次接入时幂等插入（M2 Register 完成后应由 service.rs 调用以登记 node 记录）。
pub async fn upsert_node(
    pool: &PgPool,
    node_id: &str,
    name: &str,
    ca_bundle_version: i32,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO v1_nodes (id, name, ca_bundle_version)
              VALUES ($1, $2, $3)
         ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, updated_at = NOW()",
    )
    .bind(node_id)
    .bind(name)
    .bind(ca_bundle_version)
    .execute(pool)
    .await
    .context("upsert_node 失败")?;
    Ok(())
}

#[derive(Debug, Clone, Copy, sqlx::Type)]
#[sqlx(type_name = "v1_node_status", rename_all = "snake_case")]
pub enum NodeStatus {
    Pending,
    Ok,
    Degraded,
    Offline,
}

// ── 规则查询（M3 FullSync 构造） ──────────────────────────────

/// 加载某 node 上所有 segments（M3 初版：直接读当前态；未来可加缓存）。
pub async fn load_segments_for_node(
    pool: &PgPool,
    node_id: &str,
) -> Result<Vec<SegmentRow>> {
    let rows: Vec<SegmentRow> = sqlx::query_as(
        "SELECT id, chain_id, listen_node_id, listen, proto::text as proto, ipv6,
                next_kind::text as next_kind, next_segment_id,
                upstream_host, upstream_port_start, upstream_port_end,
                rate_limit_mbps, balance::text as balance, comment
           FROM v1_segments
          WHERE listen_node_id = $1
          ORDER BY id",
    )
    .bind(node_id)
    .fetch_all(pool)
    .await
    .context("load_segments_for_node 失败")?;
    Ok(rows)
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SegmentRow {
    pub id: String,
    pub chain_id: String,
    pub listen_node_id: String,
    pub listen: String,
    pub proto: String,
    pub ipv6: bool,
    pub next_kind: String,
    pub next_segment_id: Option<String>,
    pub upstream_host: Option<String>,
    pub upstream_port_start: Option<i32>,
    pub upstream_port_end: Option<i32>,
    pub rate_limit_mbps: Option<i32>,
    pub balance: String,
    pub comment: Option<String>,
}
