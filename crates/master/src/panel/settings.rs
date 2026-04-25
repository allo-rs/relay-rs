//! v1_settings KV 表的辅助函数。
//!
//! 当前只用于 discourse SSO 配置，但表本身是通用的（key, value JSONB）。

use anyhow::{Context, Result};
use serde_json::Value;
use sqlx::PgPool;

pub async fn get_setting(pool: &PgPool, key: &str) -> Result<Option<Value>> {
    let row: Option<(Value,)> = sqlx::query_as("SELECT value FROM v1_settings WHERE key = $1")
        .bind(key)
        .fetch_optional(pool)
        .await
        .with_context(|| format!("读取 v1_settings.{} 失败", key))?;
    Ok(row.map(|r| r.0))
}

pub async fn put_setting(pool: &PgPool, key: &str, value: &Value) -> Result<()> {
    sqlx::query(
        "INSERT INTO v1_settings (key, value, updated_at) VALUES ($1, $2, NOW())
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = NOW()",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await
    .with_context(|| format!("写入 v1_settings.{} 失败", key))?;
    Ok(())
}

pub async fn delete_setting(pool: &PgPool, key: &str) -> Result<bool> {
    let r = sqlx::query("DELETE FROM v1_settings WHERE key = $1")
        .bind(key)
        .execute(pool)
        .await
        .with_context(|| format!("删除 v1_settings.{} 失败", key))?;
    Ok(r.rows_affected() > 0)
}
