use axum::http::HeaderMap;
use jsonwebtoken::{Algorithm, decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

/// JWT Claims 结构
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: u64,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// 面板 Web 登录 JWT（HMAC-SHA256，7 天）
pub fn create_token(username: &str, secret: &str) -> Result<String, jsonwebtoken::errors::Error> {
    let claims = Claims { sub: username.to_string(), exp: now_secs() + 7 * 24 * 3600 };
    encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_bytes()))
}

/// 验证面板 Web JWT，返回 username
pub fn verify_token(token: &str, secret: &str) -> Result<String, jsonwebtoken::errors::Error> {
    let data = decode::<Claims>(token, &DecodingKey::from_secret(secret.as_bytes()), &Validation::default())?;
    Ok(data.claims.sub)
}

/// 创建主控→节点 API 调用 JWT（Ed25519，10 分钟短效）
pub fn sign_node_jwt(private_key_pem: &str) -> Result<String, jsonwebtoken::errors::Error> {
    let claims = Claims { sub: "master".to_string(), exp: now_secs() + 600 };
    encode(
        &Header::new(Algorithm::EdDSA),
        &claims,
        &EncodingKey::from_ed_pem(private_key_pem.as_bytes())?,
    )
}

/// 节点验证主控 JWT（Ed25519）
pub fn verify_node_jwt(token: &str, public_key_pem: &str) -> Result<(), jsonwebtoken::errors::Error> {
    let mut v = Validation::new(Algorithm::EdDSA);
    v.set_required_spec_claims(&["exp"]);
    decode::<Claims>(token, &DecodingKey::from_ed_pem(public_key_pem.as_bytes())?, &v)?;
    Ok(())
}

/// 从 `Authorization: Bearer <token>` 请求头提取 token 字符串
pub fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    value.strip_prefix("Bearer ").map(|s| s.to_string())
}
