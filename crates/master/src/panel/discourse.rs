//! Discourse Connect (SSO Provider) 集成，**逐字**从 v0 `src/panel/discourse.rs` 移植。
//!
//! 流程：
//! 1. panel 生成 nonce，构造 payload=`nonce=<n>&return_sso_url=<callback>`
//! 2. sso = base64(payload), sig = hex(HMAC-SHA256(secret, sso))
//! 3. 跳转 `{discourse_url}/session/sso_provider?sso=..&sig=..`
//! 4. discourse 登录后回跳 callback，校验 sig/nonce，取用户信息

use base64::Engine as _;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use crate::panel::auth::{UserClaims, now_secs};

type HmacSha256 = Hmac<Sha256>;

const NONCE_TTL_SECS: u64 = 600;

/// 简单内存 nonce 仓库：nonce → (创建时间, 可选的回跳路径)
pub struct NonceStore {
    inner: Mutex<HashMap<String, (Instant, String)>>,
}

impl NonceStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    fn gc(&self, map: &mut HashMap<String, (Instant, String)>) {
        let ttl = std::time::Duration::from_secs(NONCE_TTL_SECS);
        map.retain(|_, (t, _)| t.elapsed() < ttl);
    }

    /// 创建新 nonce
    pub fn issue(&self, return_to: String) -> String {
        let nonce = random_hex(32);
        let mut map = self.inner.lock().unwrap();
        self.gc(&mut map);
        map.insert(nonce.clone(), (Instant::now(), return_to));
        nonce
    }

    /// 消费 nonce（一次性）；成功返回对应的 return_to
    pub fn consume(&self, nonce: &str) -> Option<String> {
        let mut map = self.inner.lock().unwrap();
        self.gc(&mut map);
        map.remove(nonce).map(|(_, r)| r)
    }
}

/// 生成 len 个十六进制字符的随机串（CSPRNG）。
fn random_hex(len: usize) -> String {
    use rand::RngCore;
    let bytes = len.div_ceil(2);
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(&buf).chars().take(len).collect()
}

/// 构造登录跳转 URL（panel → discourse）。
pub fn build_login_redirect(
    discourse_url: &str,
    secret: &str,
    return_sso_url: &str,
    nonce: &str,
) -> String {
    let payload = format!(
        "nonce={}&return_sso_url={}",
        urlencoding::encode(nonce),
        urlencoding::encode(return_sso_url)
    );
    let sso = base64::engine::general_purpose::STANDARD.encode(payload.as_bytes());
    let sig = hmac_hex(secret, &sso);
    format!(
        "{}/session/sso_provider?sso={}&sig={}",
        discourse_url.trim_end_matches('/'),
        urlencoding::encode(&sso),
        sig
    )
}

/// 校验回调参数并解析 payload，返回 UserClaims（不含 exp，需调用方填）。
pub fn verify_and_parse(
    sso_b64: &str,
    sig_hex: &str,
    secret: &str,
    expected_nonce_fn: impl FnOnce(&str) -> bool,
) -> Result<UserClaims, String> {
    let expected_sig = hmac_hex(secret, sso_b64);
    if !constant_time_eq(expected_sig.as_bytes(), sig_hex.as_bytes()) {
        return Err("SSO 签名校验失败".into());
    }

    let raw = base64::engine::general_purpose::STANDARD
        .decode(sso_b64)
        .map_err(|e| format!("sso base64 解码失败: {}", e))?;
    let payload = String::from_utf8(raw).map_err(|e| format!("sso payload 非 UTF-8: {}", e))?;

    let mut fields: HashMap<String, String> = HashMap::new();
    for pair in payload.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            let k = urlencoding::decode(k).map_err(|e| e.to_string())?.into_owned();
            let v = urlencoding::decode(v).map_err(|e| e.to_string())?.into_owned();
            fields.insert(k, v);
        }
    }

    let nonce = fields.get("nonce").ok_or("payload 缺少 nonce")?;
    if !expected_nonce_fn(nonce) {
        return Err("nonce 无效或已过期".into());
    }

    let external_id = fields
        .get("external_id")
        .cloned()
        .ok_or("payload 缺少 external_id")?;
    let username = fields
        .get("username")
        .cloned()
        .ok_or("payload 缺少 username")?;
    let admin = fields.get("admin").map(|v| v == "true").unwrap_or(false);

    Ok(UserClaims {
        sub: external_id,
        username,
        name: fields.get("name").cloned().filter(|s| !s.is_empty()),
        email: fields.get("email").cloned().filter(|s| !s.is_empty()),
        avatar: fields
            .get("avatar_url")
            .cloned()
            .filter(|s| !s.is_empty()),
        admin,
        exp: now_secs() + 7 * 24 * 3600,
    })
}

fn hmac_hex(secret: &str, msg: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC 任意 key 长度");
    mac.update(msg.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
