use sqlx::{postgres::PgPoolOptions, PgPool, Row};

pub struct Node {
    pub id: i32,
    pub name: String,
    pub url: String,
}

pub async fn connect(url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new().max_connections(5).connect(url).await
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
    Ok(())
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
