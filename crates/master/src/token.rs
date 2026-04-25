//! Enrollment token：master 生成一次性 token 授权新 node 首次注册。
//!
//! v1.0 MVP 使用文件存储；后续 M3+ 迁移到 DB。

use anyhow::{Context, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const TOKEN_BYTES: usize = 24;
/// token 默认有效期：24 小时
const DEFAULT_TTL_SECS: u64 = 86400;

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenRecord {
    pub node_name: String,
    pub expires_at: u64,
    pub created_at: u64,
}

pub struct TokenStore {
    dir: PathBuf,
}

impl TokenStore {
    pub fn new(dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&dir)
            .with_context(|| format!("创建 token 目录 {:?} 失败", dir))?;
        Ok(Self { dir })
    }

    /// 生成新 token 并写盘，返回明文 token 给运维
    pub fn create(&self, node_name: &str, ttl_secs: Option<u64>) -> Result<String> {
        if node_name.trim().is_empty() {
            anyhow::bail!("node_name 不能为空");
        }
        let mut buf = [0u8; TOKEN_BYTES];
        rand::thread_rng().fill_bytes(&mut buf);
        use base64::Engine;
        let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf);
        let now = now();
        let rec = TokenRecord {
            node_name: node_name.to_string(),
            expires_at: now + ttl_secs.unwrap_or(DEFAULT_TTL_SECS),
            created_at: now,
        };
        let path = self.path_for(&token);
        fs::write(&path, serde_json::to_vec_pretty(&rec)?)
            .with_context(|| format!("写 token 文件 {:?} 失败", path))?;
        Ok(token)
    }

    /// 消费 token：校验存在 + 未过期；消费成功后删除文件（一次性）。
    /// 用 `rename` 原子争抢避免 TOCTOU：两个并发调用最多一个 rename 成功。
    pub fn consume(&self, token: &str) -> Result<TokenRecord> {
        let path = self.path_for(token);
        // 生成一个争抢用的唯一目标名
        let mut rng_buf = [0u8; 8];
        rand::thread_rng().fill_bytes(&mut rng_buf);
        let claim_path = self.dir.join(format!(
            "{}.consuming-{}-{}",
            token,
            std::process::id(),
            hex::encode(rng_buf)
        ));
        // 原子 rename：失败说明 token 不存在或已被并发消费
        fs::rename(&path, &claim_path)
            .with_context(|| "token 不存在或已被消费".to_string())?;
        let content = match fs::read_to_string(&claim_path) {
            Ok(c) => c,
            Err(e) => {
                let _ = fs::remove_file(&claim_path);
                return Err(anyhow::anyhow!("读 token 文件失败: {}", e));
            }
        };
        let rec: TokenRecord = match serde_json::from_str(&content) {
            Ok(r) => r,
            Err(e) => {
                let _ = fs::remove_file(&claim_path);
                return Err(anyhow::anyhow!("token 记录损坏: {}", e));
            }
        };
        // 一次性消费：无论过期与否都删
        let _ = fs::remove_file(&claim_path);
        if rec.expires_at < now() {
            anyhow::bail!("token 已过期");
        }
        Ok(rec)
    }

    fn path_for(&self, token: &str) -> PathBuf {
        // token 本身是 base64url（无斜杠、无 +），可直接用作文件名
        self.dir.join(format!("{}.json", token))
    }
}

pub fn default_token_dir() -> PathBuf {
    std::env::var("RELAY_MASTER_TOKEN_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let mut p = super::ca::default_ca_dir();
            p.push("enrollment-tokens");
            p
        })
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
