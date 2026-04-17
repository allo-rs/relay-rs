use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;

const DEFAULT_TTL: Duration = Duration::from_secs(60);
/// 最多缓存的条目数，防止内存无界增长（超出时淘汰最早过期的条目）
const MAX_ENTRIES: usize = 1024;

struct Entry {
    addrs: Vec<SocketAddr>,
    expires: Instant,
}

/// in-flight 请求的广播通道：None = 未完成，Some(Ok) = 成功，Some(Err) = 失败
type InflightSender = watch::Sender<Option<Result<Vec<SocketAddr>, String>>>;

struct Inner {
    cache: HashMap<String, Entry>,
    /// 正在进行的 DNS 查询，同 key 的后续请求订阅等待，不重复发起
    inflight: HashMap<String, Arc<InflightSender>>,
}

#[derive(Clone)]
pub struct DnsCache {
    inner: Arc<Mutex<Inner>>,
}

/// Drop guard：确保发起 DNS 查询的任务被 abort 时，in-flight 条目被清理，
/// 不会让等待同一 key 的后续请求永久挂死。
struct InflightGuard {
    inner: Arc<Mutex<Inner>>,
    key: String,
    tx: Arc<InflightSender>,
}

impl Drop for InflightGuard {
    fn drop(&mut self) {
        // 发送中断信号唤醒所有等待者，再从 inflight 移除
        let _ = self.tx.send(Some(Err("DNS 查询被中断".to_string())));
        if let Ok(mut inner) = self.inner.lock() {
            inner.inflight.remove(&self.key);
        }
    }
}

impl DnsCache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                cache: HashMap::new(),
                inflight: HashMap::new(),
            })),
        }
    }

    /// 解析 host:port，优先 IPv4，ipv6=true 时优先 IPv6。
    /// 先查缓存，缓存未命中时同 key 的并发请求只发起一次 DNS 查询。
    pub async fn resolve(
        &self,
        host: &str,
        port: u16,
        ipv6: bool,
    ) -> std::io::Result<SocketAddr> {
        let key = format!("{}:{}:{}", host, port, ipv6);

        let maybe_rx: Option<watch::Receiver<_>> = {
            let mut inner = self.inner.lock().unwrap();

            // 缓存命中
            if let Some(entry) = inner.cache.get(&key) {
                if entry.expires > Instant::now() {
                    if let Some(addr) = pick_addr(&entry.addrs, ipv6) {
                        return Ok(addr);
                    }
                }
            }

            if let Some(tx) = inner.inflight.get(&key) {
                // 已有 in-flight 请求，订阅等待结果
                Some(tx.subscribe())
            } else {
                // 自己发起，插入占位
                let (tx, _) = watch::channel(None);
                inner.inflight.insert(key.clone(), Arc::new(tx));
                None
            }
        };

        if let Some(mut rx) = maybe_rx {
            // 等待 in-flight 完成（sender 已在 mutex 释放前创建，send 发生在 mutex 释放后，不会错过）
            // Err 表示 sender 已 drop（发起任务被 abort），直接返回错误让调用方重试
            if rx.changed().await.is_err() {
                return Err(std::io::Error::other(format!("DNS 查询 {} 被中断，请重试", host)));
            }
            return match rx.borrow().as_ref() {
                Some(Ok(addrs)) => pick_addr(addrs, ipv6)
                    .ok_or_else(|| std::io::Error::other(format!("无法解析 {} 的地址", host))),
                Some(Err(e)) => Err(std::io::Error::other(e.clone())),
                None => Err(std::io::Error::other(format!("DNS 查询 {} 被中断，请重试", host))),
            };
        }

        // 创建 guard：确保即使当前任务被 abort，in-flight 条目也会被清理
        let guard = InflightGuard {
            inner: Arc::clone(&self.inner),
            key: key.clone(),
            tx: self.inner.lock().unwrap().inflight.get(&key).cloned().unwrap(),
        };

        // 发起 DNS 查询，EAI_AGAIN（临时失败）最多重试 3 次
        let lookup = format!("{}:{}", host, port);
        let result: Result<Vec<SocketAddr>, String> = {
            let mut last_err = String::new();
            let mut ok = None;
            for attempt in 0..3u8 {
                match tokio::net::lookup_host(&lookup).await {
                    Ok(iter) => { ok = Some(iter.collect::<Vec<_>>()); break; }
                    Err(e) => {
                        last_err = e.to_string();
                        // 仅对临时性错误重试（EAI_AGAIN）
                        if !last_err.contains("Try again") && !last_err.contains("SERVFAIL") {
                            break;
                        }
                        if attempt < 2 {
                            tokio::time::sleep(Duration::from_millis(200)).await;
                        }
                    }
                }
            }
            ok.map(Ok).unwrap_or(Err(last_err))
        };

        // 正常完成：通知等待者 + 写缓存，然后阻止 guard 的 Drop 重复清理
        {
            let mut inner = self.inner.lock().unwrap();
            if let Some(tx) = inner.inflight.remove(&key) {
                let _ = tx.send(Some(result.clone()));
            }
            if let Ok(ref addrs) = result {
                if !addrs.is_empty() {
                    if inner.cache.len() >= MAX_ENTRIES {
                        let now = Instant::now();
                        inner.cache.retain(|_, e| e.expires > now);
                    }
                    if inner.cache.len() < MAX_ENTRIES {
                        inner.cache.insert(key, Entry {
                            addrs: addrs.clone(),
                            expires: Instant::now() + DEFAULT_TTL,
                        });
                    }
                }
            }
        }
        // inflight 已手动清理，不需要 guard 的 Drop 再跑一次
        std::mem::forget(guard);

        match result {
            Ok(addrs) if !addrs.is_empty() => pick_addr(&addrs, ipv6)
                .ok_or_else(|| std::io::Error::other(format!("无法解析 {} 的地址", host))),
            Ok(_) => Err(std::io::Error::other(format!("无法解析 {}", host))),
            Err(e) => Err(std::io::Error::other(e)),
        }
    }

    /// 失效某个条目（连接失败时调用，强制下次重新解析）
    pub fn invalidate(&self, host: &str, port: u16, ipv6: bool) {
        let key = format!("{}:{}:{}", host, port, ipv6);
        self.inner.lock().unwrap().cache.remove(&key);
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
