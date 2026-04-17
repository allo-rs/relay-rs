use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const DEFAULT_TTL: Duration = Duration::from_secs(60);
/// 最多缓存的条目数，防止内存无界增长（超出时淘汰最早过期的条目）
const MAX_ENTRIES: usize = 1024;

struct Entry {
    addrs: Vec<SocketAddr>,
    expires: Instant,
}

#[derive(Clone)]
pub struct DnsCache {
    inner: Arc<Mutex<HashMap<String, Entry>>>,
}

impl DnsCache {
    pub fn new() -> Self {
        Self { inner: Arc::new(Mutex::new(HashMap::new())) }
    }

    /// 解析 host:port，优先 IPv4，ipv6=true 时优先 IPv6。
    /// 先查缓存，过期或不存在则重新解析并缓存 TTL。
    pub async fn resolve(
        &self,
        host: &str,
        port: u16,
        ipv6: bool,
    ) -> std::io::Result<SocketAddr> {
        let key = format!("{}:{}:{}", host, port, ipv6);

        // 读缓存
        {
            let cache = self.inner.lock().unwrap();
            if let Some(entry) = cache.get(&key) {
                if entry.expires > Instant::now() {
                    if let Some(addr) = pick_addr(&entry.addrs, ipv6) {
                        return Ok(addr);
                    }
                }
            }
        }

        // 重新解析
        let lookup = format!("{}:{}", host, port);
        let addrs: Vec<SocketAddr> = tokio::net::lookup_host(&lookup).await?.collect();
        if addrs.is_empty() {
            return Err(std::io::Error::other(format!("无法解析 {}", host)));
        }

        let addr = pick_addr(&addrs, ipv6)
            .ok_or_else(|| std::io::Error::other(format!("无法解析 {} 的地址", host)))?;

        // 写缓存
        {
            let mut cache = self.inner.lock().unwrap();
            // 超出上限时淘汰所有已过期条目；若仍满则放弃缓存（不阻塞请求）
            if cache.len() >= MAX_ENTRIES {
                let now = Instant::now();
                cache.retain(|_, e| e.expires > now);
                if cache.len() >= MAX_ENTRIES {
                    return Ok(addr);
                }
            }
            cache.insert(key, Entry {
                addrs: addrs.clone(),
                expires: Instant::now() + DEFAULT_TTL,
            });
        }

        Ok(addr)
    }

    /// 失效某个条目（连接失败时调用，强制下次重新解析）
    pub fn invalidate(&self, host: &str, port: u16, ipv6: bool) {
        let key = format!("{}:{}:{}", host, port, ipv6);
        self.inner.lock().unwrap().remove(&key);
    }
}

fn pick_addr(addrs: &[SocketAddr], ipv6: bool) -> Option<SocketAddr> {
    if ipv6 {
        addrs.iter().find(|a| a.is_ipv6()).copied()
    } else {
        addrs.iter().find(|a| a.is_ipv4()).copied()
            .or_else(|| addrs.iter().find(|a| a.is_ipv6()).copied())
    }
}
