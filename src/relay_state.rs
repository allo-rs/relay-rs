use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use crate::config::BlockRule;

/// 最大并发 TCP 转发连接数（每连接在 Linux 上消耗约 6 个 FD：2 socket + 4 pipe）
pub const MAX_CONNS: usize = 4096;

// ── 流量统计 ──────────────────────────────────────────────────────

/// 单条规则的流量统计
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct RuleStats {
    /// 累计连接数
    pub total_conns: u64,
    /// client→target 字节数
    pub bytes_in: u64,
    /// target→client 字节数
    pub bytes_out: u64,
}

// ── 令牌桶限速器 ──────────────────────────────────────────────────

/// 令牌桶限速器（Mbps），粗粒度：每次连接建立时检查
pub struct TokenBucket {
    tokens: f64,
    capacity: f64, // bytes
    rate: f64,     // bytes/sec
    last: Instant,
}

impl TokenBucket {
    pub fn new(mbps: u32) -> Self {
        let cap = mbps as f64 * 1_000_000.0 / 8.0;
        Self {
            tokens: cap,
            capacity: cap,
            rate: cap,
            last: Instant::now(),
        }
    }

    /// 消耗 bytes，返回 false 表示超速（桶内 token 不足）
    pub fn consume(&mut self, bytes: u64) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + elapsed * self.rate).min(self.capacity);
        if self.tokens >= bytes as f64 {
            self.tokens -= bytes as f64;
            true
        } else {
            false
        }
    }
}

// ── 全局共享状态 ──────────────────────────────────────────────────

pub struct RelayState {
    /// 规则流量统计，key = 监听端口字符串
    pub stats: Mutex<HashMap<String, RuleStats>>,
    /// 速率限制令牌桶，key = 监听端口字符串
    pub limiters: Mutex<HashMap<String, TokenBucket>>,
    /// Block 规则列表（只读，启动时加载）
    pub block_rules: Vec<BlockRule>,
    /// 并发连接数上限信号量，防止 FD 耗尽
    pub conn_sem: Arc<Semaphore>,
}

pub type SharedState = Arc<RelayState>;

impl RelayState {
    pub fn new(block_rules: Vec<BlockRule>) -> SharedState {
        Arc::new(Self {
            stats: Mutex::new(HashMap::new()),
            limiters: Mutex::new(HashMap::new()),
            block_rules,
            conn_sem: Arc::new(Semaphore::new(MAX_CONNS)),
        })
    }

    /// 将统计写入 /tmp/relay-rs.stats（JSON 格式），忽略写入失败
    pub fn flush_to_file(&self) {
        let Ok(map) = self.stats.lock() else { return };
        let Ok(json) = serde_json::to_string_pretty(&*map) else { return };
        let _ = std::fs::write("/tmp/relay-rs.stats", json);
    }
}
