use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use serde_json::Value;

use crate::config::{BlockRule, ForwardMode, ForwardRule};

pub struct Node {
    pub id: i32,
    pub name: String,
    pub url: String,
}

pub struct ForwardRuleRow {
    pub id: i32,
    pub rule: ForwardRule,
}

pub struct BlockRuleRow {
    pub id: i32,
    pub rule: BlockRule,
}

pub async fn connect(url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(url).await
}

/// 首次运行建表（幂等）
pub async fn ensure_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS nodes (
            id         SERIAL PRIMARY KEY,
            name       TEXT        NOT NULL,
            url        TEXT        NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS settings (
            key        TEXT        PRIMARY KEY,
            value      JSONB       NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS forward_rules (
            id   SERIAL PRIMARY KEY,
            rule JSONB  NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS block_rules (
            id   SERIAL PRIMARY KEY,
            rule JSONB  NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_setting(pool: &PgPool, key: &str) -> Result<Option<Value>, sqlx::Error> {
    let row = sqlx::query("SELECT value FROM settings WHERE key = $1")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.get::<Value, _>("value")))
}

pub async fn set_setting(pool: &PgPool, key: &str, value: &Value) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES ($1, $2, NOW())
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = NOW()",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_setting(pool: &PgPool, key: &str) -> Result<bool, sqlx::Error> {
    let r = sqlx::query("DELETE FROM settings WHERE key = $1")
        .bind(key)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn list_nodes(pool: &PgPool) -> Result<Vec<Node>, sqlx::Error> {
    let rows = sqlx::query("SELECT id, name, url FROM nodes ORDER BY id")
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| Node { id: r.get("id"), name: r.get("name"), url: r.get("url") })
        .collect())
}

pub async fn get_node(pool: &PgPool, id: i32) -> Result<Option<Node>, sqlx::Error> {
    let row = sqlx::query("SELECT id, name, url FROM nodes WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| Node { id: r.get("id"), name: r.get("name"), url: r.get("url") }))
}

pub async fn add_node(pool: &PgPool, name: &str, url: &str) -> Result<i32, sqlx::Error> {
    let row = sqlx::query("INSERT INTO nodes (name, url) VALUES ($1, $2) RETURNING id")
        .bind(name)
        .bind(url)
        .fetch_one(pool)
        .await?;
    Ok(row.get("id"))
}

pub async fn delete_node(pool: &PgPool, id: i32) -> Result<bool, sqlx::Error> {
    let r = sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

// ── 转发规则 ──────────────────────────────────────────────────────

pub async fn list_forward_rules(pool: &PgPool) -> Result<Vec<ForwardRuleRow>, sqlx::Error> {
    let rows = sqlx::query("SELECT id, rule FROM forward_rules ORDER BY id")
        .fetch_all(pool)
        .await?;
    let mut result = Vec::new();
    for row in rows {
        let id: i32 = row.get("id");
        let val: Value = row.get("rule");
        match serde_json::from_value::<ForwardRule>(val) {
            Ok(rule) => result.push(ForwardRuleRow { id, rule }),
            Err(e) => log::warn!("反序列化 forward_rule id={} 失败: {}", id, e),
        }
    }
    Ok(result)
}

pub async fn add_forward_rule(pool: &PgPool, rule: &ForwardRule) -> Result<i32, sqlx::Error> {
    let val = serde_json::to_value(rule).expect("序列化 ForwardRule 失败");
    let row = sqlx::query("INSERT INTO forward_rules (rule) VALUES ($1) RETURNING id")
        .bind(val)
        .fetch_one(pool)
        .await?;
    Ok(row.get("id"))
}

pub async fn update_forward_rule(pool: &PgPool, id: i32, rule: &ForwardRule) -> Result<(), sqlx::Error> {
    let val = serde_json::to_value(rule).expect("序列化 ForwardRule 失败");
    sqlx::query("UPDATE forward_rules SET rule = $1 WHERE id = $2")
        .bind(val)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_forward_rule(pool: &PgPool, id: i32) -> Result<bool, sqlx::Error> {
    let r = sqlx::query("DELETE FROM forward_rules WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

// ── 防火墙规则 ────────────────────────────────────────────────────

pub async fn list_block_rules(pool: &PgPool) -> Result<Vec<BlockRuleRow>, sqlx::Error> {
    let rows = sqlx::query("SELECT id, rule FROM block_rules ORDER BY id")
        .fetch_all(pool)
        .await?;
    let mut result = Vec::new();
    for row in rows {
        let id: i32 = row.get("id");
        let val: Value = row.get("rule");
        match serde_json::from_value::<BlockRule>(val) {
            Ok(rule) => result.push(BlockRuleRow { id, rule }),
            Err(e) => log::warn!("反序列化 block_rule id={} 失败: {}", id, e),
        }
    }
    Ok(result)
}

pub async fn add_block_rule(pool: &PgPool, rule: &BlockRule) -> Result<i32, sqlx::Error> {
    let val = serde_json::to_value(rule).expect("序列化 BlockRule 失败");
    let row = sqlx::query("INSERT INTO block_rules (rule) VALUES ($1) RETURNING id")
        .bind(val)
        .fetch_one(pool)
        .await?;
    Ok(row.get("id"))
}

pub async fn update_block_rule(pool: &PgPool, id: i32, rule: &BlockRule) -> Result<(), sqlx::Error> {
    let val = serde_json::to_value(rule).expect("序列化 BlockRule 失败");
    sqlx::query("UPDATE block_rules SET rule = $1 WHERE id = $2")
        .bind(val)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_block_rule(pool: &PgPool, id: i32) -> Result<bool, sqlx::Error> {
    let r = sqlx::query("DELETE FROM block_rules WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

// ── 鉴权与密钥设置 ────────────────────────────────────────────────

/// 获取或生成 panel_secret（40 字符随机串）
pub async fn get_or_create_secret(pool: &PgPool) -> Result<String, sqlx::Error> {
    use base64::Engine as _;
    if let Some(val) = get_setting(pool, "panel_secret").await? {
        if let Some(s) = val.as_str() {
            return Ok(s.to_string());
        }
    }
    // 用 /dev/urandom + base64 生成随机串，取前 40 字符
    // 注意：/dev/urandom 是字符设备，必须用 read_exact 读取固定字节数
    use std::io::Read;
    let mut buf = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .unwrap_or(());
    let b64 = base64::engine::general_purpose::STANDARD.encode(&buf);
    let secret: String = b64.chars().filter(|c| c.is_alphanumeric()).take(40).collect();
    set_setting(pool, "panel_secret", &Value::String(secret.clone())).await?;
    Ok(secret)
}

/// 获取 Ed25519 私钥 PEM
pub async fn get_private_key(pool: &PgPool) -> Result<Option<String>, sqlx::Error> {
    Ok(get_setting(pool, "panel_private_key")
        .await?
        .and_then(|v| v.as_str().map(|s| s.to_string())))
}

/// 写入 Ed25519 私钥 PEM
pub async fn set_private_key(pool: &PgPool, pem: &str) -> Result<(), sqlx::Error> {
    set_setting(pool, "panel_private_key", &Value::String(pem.to_string())).await
}

/// 获取转发模式，默认 Nat
pub async fn get_forward_mode(pool: &PgPool) -> Result<ForwardMode, sqlx::Error> {
    Ok(get_setting(pool, "forward_mode")
        .await?
        .and_then(|v| serde_json::from_value::<ForwardMode>(v).ok())
        .unwrap_or_default())
}

/// 写入转发模式
pub async fn set_forward_mode(pool: &PgPool, mode: &ForwardMode) -> Result<(), sqlx::Error> {
    let val = serde_json::to_value(mode).expect("序列化 ForwardMode 失败");
    set_setting(pool, "forward_mode", &val).await
}
