//! segment apply 管理器。
//!
//! M3 范围（**刻意受限**，明确写给未来读者）：
//!   - 仅处理 `Segment.proto == TCP` 且 `next == Upstream` 的 segment
//!   - 监听端口=`listen` 字段第一个端口；bind 成功视为 apply ok
//!   - accept 后简单双向转发到 upstream host:port_start（first port）—— 不做负载均衡、
//!     不做 health check、不做限速、不做连接池。**仅用于证明控制面通路可用**。
//!   - `next == NodeNext`（chain 下一段）→ fatal 错误 `"chain not supported until M4"`
//!   - `proto == UDP / ALL` → fatal 错误 `"udp/all not supported until M4.5"`
//!   - 多端口段（listen="80-100"）→ fatal 错误 `"port range not supported in M3"`
//!   - 失败分类：`EADDRINUSE`/`PermissionDenied` 视为 retryable（短 backoff 3 次）
//!
//! 管理策略：
//!   - 入口：`Applier::apply_full_sync(segments)`。
//!   - 内部比对 new vs 当前 running，diff 出 `to_start / to_stop / unchanged`。
//!   - 每个运行中的 segment 有一个 `JoinHandle` + 取消 token；stop 时取消并等结束。
//!   - 返回 `ApplyOutcome { per_segment: HashMap<id, ApplyResult>, actual_ids: Vec<String> }`。

use anyhow::{Result, anyhow};
use relay_proto::v1::{segment, ApplyResult, Proto, Segment};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub struct Applier {
    running: Arc<Mutex<HashMap<String, Running>>>,
}

struct Running {
    spec_hash: u64,       // 用于识别"同一 id 但改了参数"→ 需要重启
    cancel: CancellationToken,
    join: tokio::task::JoinHandle<()>,
}

pub struct ApplyOutcome {
    pub per_segment: HashMap<String, ApplyResult>,
    pub actual_ids: Vec<String>,
    /// 实际生效的 segment proto（用于计算 actual_hash）
    pub actual_segments: Vec<Segment>,
}

impl Applier {
    pub fn new() -> Self {
        Self {
            running: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn apply_full_sync(&self, segments: Vec<Segment>) -> ApplyOutcome {
        let mut per_segment: HashMap<String, ApplyResult> = HashMap::new();
        let mut actual_ids: Vec<String> = Vec::new();
        let mut actual_segments: Vec<Segment> = Vec::new();

        let new_by_id: HashMap<String, Segment> =
            segments.into_iter().map(|s| (s.id.clone(), s)).collect();
        let mut guard = self.running.lock().await;

        // 停掉不再存在的 / 改了规格的
        let existing_ids: Vec<String> = guard.keys().cloned().collect();
        for id in existing_ids {
            let need_stop = match new_by_id.get(&id) {
                None => true,
                Some(s) => {
                    let h = spec_hash(s);
                    guard.get(&id).map(|r| r.spec_hash != h).unwrap_or(true)
                }
            };
            if need_stop
                && let Some(r) = guard.remove(&id)
            {
                log::info!("apply: stop segment {}", id);
                r.cancel.cancel();
                let _ = r.join.await;
            }
        }

        // 启动新的 / 已改的
        for (id, seg) in new_by_id {
            if guard.contains_key(&id) {
                // spec 未变，跳过
                per_segment.insert(id.clone(), ok_result());
                actual_ids.push(id.clone());
                actual_segments.push(seg);
                continue;
            }

            match try_start(&seg).await {
                Ok((cancel, join)) => {
                    guard.insert(
                        id.clone(),
                        Running {
                            spec_hash: spec_hash(&seg),
                            cancel,
                            join,
                        },
                    );
                    per_segment.insert(id.clone(), ok_result());
                    actual_ids.push(id.clone());
                    actual_segments.push(seg);
                }
                Err(e) => {
                    log::warn!("apply: segment {} 失败: {}", id, e);
                    per_segment.insert(
                        id.clone(),
                        ApplyResult {
                            ok: false,
                            error: format!("{}", e),
                        },
                    );
                    // 不加入 actual_segments —— 这个 segment 未生效
                }
            }
        }

        ApplyOutcome {
            per_segment,
            actual_ids,
            actual_segments,
        }
    }

    /// 返回当前所有运行中的 segment id（用于 heartbeat/诊断）
    #[allow(dead_code)]
    pub async fn running_ids(&self) -> Vec<String> {
        self.running.lock().await.keys().cloned().collect()
    }
}

fn ok_result() -> ApplyResult {
    ApplyResult {
        ok: true,
        error: String::new(),
    }
}

fn spec_hash(s: &Segment) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.listen.hash(&mut h);
    s.proto.hash(&mut h);
    s.ipv6.hash(&mut h);
    match &s.next {
        Some(segment::Next::Upstream(u)) => {
            "upstream".hash(&mut h);
            u.host.hash(&mut h);
            u.port_start.hash(&mut h);
            u.port_end.hash(&mut h);
        }
        Some(segment::Next::NodeNext(n)) => {
            "node".hash(&mut h);
            n.segment_id.hash(&mut h);
        }
        None => "none".hash(&mut h),
    }
    h.finish()
}

async fn try_start(
    seg: &Segment,
) -> Result<(CancellationToken, tokio::task::JoinHandle<()>)> {
    // 约束校验
    let proto = Proto::try_from(seg.proto).unwrap_or(Proto::Unspecified);
    if proto != Proto::Tcp {
        return Err(anyhow!("udp/all not supported until M4.5"));
    }
    let upstream = match &seg.next {
        Some(segment::Next::Upstream(u)) => u.clone(),
        Some(segment::Next::NodeNext(_)) => {
            return Err(anyhow!("chain not supported until M4"));
        }
        None => return Err(anyhow!("segment.next 缺失")),
    };
    if seg.listen.contains('-') || seg.listen.contains(',') {
        return Err(anyhow!("port range not supported in M3"));
    }
    let port: u16 = seg
        .listen
        .trim()
        .parse()
        .map_err(|e| anyhow!("listen 无法解析为端口: {}", e))?;
    if upstream.port_end != 0 && upstream.port_end != upstream.port_start {
        return Err(anyhow!("upstream 端口段 M3 只支持单口"));
    }
    if upstream.host.is_empty() || upstream.port_start == 0 {
        return Err(anyhow!("upstream host/port 非法"));
    }

    let bind_addr: SocketAddr = if seg.ipv6 {
        format!("[::]:{}", port).parse()?
    } else {
        format!("0.0.0.0:{}", port).parse()?
    };

    // retry bind 3 次
    let listener = bind_with_retry(&bind_addr, 3).await?;
    log::info!(
        "apply: segment {} listening on {} → {}:{}",
        seg.id, bind_addr, upstream.host, upstream.port_start
    );

    let cancel = CancellationToken::new();
    let cancel_child = cancel.clone();
    let upstream_str = format!("{}:{}", upstream.host, upstream.port_start);
    let segment_id = seg.id.clone();

    let join = tokio::spawn(async move {
        loop {
            let accept = tokio::select! {
                _ = cancel_child.cancelled() => break,
                r = listener.accept() => r,
            };
            let (mut inbound, peer) = match accept {
                Ok(v) => v,
                Err(e) => {
                    log::warn!("{}: accept 错误 {}", segment_id, e);
                    continue;
                }
            };
            let upstream_str = upstream_str.clone();
            let sid = segment_id.clone();
            tokio::spawn(async move {
                match TcpStream::connect(&upstream_str).await {
                    Ok(mut outbound) => {
                        let _ = copy_bidirectional(&mut inbound, &mut outbound).await;
                    }
                    Err(e) => {
                        log::debug!("{}: upstream {} 连接失败（from {}）: {}", sid, upstream_str, peer, e);
                    }
                }
            });
        }
        log::info!("apply: segment {} listener 停止", segment_id);
    });

    Ok((cancel, join))
}

async fn bind_with_retry(addr: &SocketAddr, max: u32) -> Result<TcpListener> {
    let mut last: Option<std::io::Error> = None;
    for i in 0..max {
        match TcpListener::bind(addr).await {
            Ok(l) => return Ok(l),
            Err(e) => {
                if is_retryable(&e) && i + 1 < max {
                    let wait = Duration::from_millis(200 * (1 << i));
                    log::info!("bind {} 暂失败（{}），{:?} 后重试", addr, e, wait);
                    tokio::time::sleep(wait).await;
                    last = Some(e);
                } else {
                    return Err(anyhow!("bind {}: {}", addr, e));
                }
            }
        }
    }
    Err(anyhow!(
        "bind {} 重试耗尽: {}",
        addr,
        last.map(|e| e.to_string()).unwrap_or_default()
    ))
}

fn is_retryable(e: &std::io::Error) -> bool {
    use std::io::ErrorKind;
    matches!(e.kind(), ErrorKind::AddrInUse | ErrorKind::PermissionDenied)
}
