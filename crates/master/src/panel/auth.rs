//! 面板用户 JWT（来自 Discourse 登录）；从 v0 `src/panel/auth.rs` 移植，
//! 仅保留 panel-user 相关部分（节点 Ed25519 JWT 在 v1 由 mTLS 替代）。

use axum::http::HeaderMap;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

pub const AUTH_COOKIE: &str = "relay_token";

/// 面板用户 JWT Claims（来自 Discourse 登录）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserClaims {
    /// Discourse external_id
    pub sub: String,
    pub username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    #[serde(default)]
    pub admin: bool,
    pub exp: u64,
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 签发面板 Web 登录 JWT（HMAC-SHA256，默认 7 天）。
pub fn create_user_token(
    claims: &UserClaims,
    secret: &str,
) -> Result<String, jsonwebtoken::errors::Error> {
    encode(
        &Header::default(),
        claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
}

/// 验证面板 Web JWT，返回 Claims。
pub fn verify_user_token(
    token: &str,
    secret: &str,
) -> Result<UserClaims, jsonwebtoken::errors::Error> {
    let data = decode::<UserClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;
    Ok(data.claims)
}

/// 从 `Authorization: Bearer <token>` 请求头提取 token 字符串。
#[allow(dead_code)] // panel 默认走 cookie；保留便于未来 API 客户端
pub fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    value.strip_prefix("Bearer ").map(|s| s.to_string())
}

/// 从 Cookie 请求头中提取指定名称的值。
pub fn extract_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let cookie_header = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some((k, v)) = part.split_once('=')
            && k == name
        {
            return Some(v.to_string());
        }
    }
    None
}
