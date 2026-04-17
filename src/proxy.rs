/// 用户态 TCP/UDP 代理模块
///
/// Linux：使用 splice(2) 零拷贝转发，数据在内核 pipe buffer ↔ socket buffer 间移动，
///        不经过 userspace 内存。
/// 其他平台：回退到 tokio::io::copy_bidirectional（userspace 8KB 缓冲区）。
///
/// 特性：
///   - TCP 转发（单端口 + 端口段）
///   - UDP 中继（单端口 + 端口段）
///   - splice 零拷贝（Linux）
///   - Block 规则（精确 IP 匹配，拒绝连接）
///   - 速率限制（令牌桶，新连接时检查）
///   - Stats 统计（bytes_in/out/connections，写入 /tmp/relay-rs.stats）
///   - DNS TTL 缓存（60s，失败时失效）
///   - 健康检查后台任务（每 30s，TCP 探测，失败时失效 DNS 缓存）

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use tokio::io;
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::task::JoinSet;
use tokio::time::timeout;

use crate::config::{Balance, ForwardRule, Listen, Proto};
use crate::dns_cache::DnsCache;
use crate::relay_state::{RelayState, SharedState, TokenBucket};

static RR_CTR: AtomicUsize = AtomicUsize::new(0);

/// TCP 连接单方向空闲超时：任一方向 300s 无数据即断开
const TCP_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
/// UDP session 空闲超时时间
const UDP_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
/// UDP 收包缓冲区大小
const UDP_BUF_SIZE: usize = 65536;

// ── 主循环 ────────────────────────────────────────────────────────

pub async fn run(config_path: &str, reload: Arc<AtomicBool>) {
    loop {
        let cfg = match crate::config::load(config_path) {
            Ok(c) => c,
            Err(e) => {
                log::error!("配置加载失败: {}", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        // 初始化 RelayState（block 规则 + 限速令牌桶）
        let state = RelayState::new(cfg.block.clone());
        {
            let mut limiters = state.limiters.lock().unwrap();
            for rule in &cfg.forward {
                if let Some(mbps) = rule.rate_limit {
                    limiters.insert(rule.listen.clone(), TokenBucket::new(mbps));
                }
            }
        }

        // 初始化 DNS 缓存
        let dns_cache = DnsCache::new();

        let mut set: JoinSet<()> = JoinSet::new();

        // 健康检查后台任务（每 30s，TCP 探测，失败时失效 DNS 缓存）
        {
            let rules = cfg.forward.clone();
            let cache = dns_cache.clone();
            set.spawn(async move {
                health_check_loop(rules, cache).await;
            });
        }

        // 定期将统计刷盘，避免每次连接都做阻塞文件 I/O
        {
            let state_flush = Arc::clone(&state);
            set.spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    let s = Arc::clone(&state_flush);
                    tokio::task::spawn_blocking(move || s.flush_to_file()).await.ok();
                }
            });
        }

        for rule in cfg.forward {
            let listen = match crate::config::Listen::parse(&rule.listen) {
                Ok(l) => l,
                Err(e) => { log::warn!("跳过规则 [{}]: {}", rule.listen, e); continue; }
            };

            // 根据 proto 决定启动 TCP / UDP / 两者
            let need_tcp = !matches!(rule.proto, Proto::Udp);
            let need_udp = matches!(rule.proto, Proto::Udp | Proto::All);

            if need_tcp {
                let rule_clone = rule.clone();
                let listen_clone = listen.clone();
                let state_clone = Arc::clone(&state);
                let cache_clone = dns_cache.clone();
                set.spawn(listen_rule(rule_clone, listen_clone, state_clone, cache_clone));
            }

            if need_udp {
                match &listen {
                    Listen::Single(port) => {
                        let p = *port;
                        let rule_clone = rule.clone();
                        let cache_clone = dns_cache.clone();
                        set.spawn(listen_udp_rule(rule_clone, p, 0, cache_clone));
                    }
                    Listen::Range(start, end) => {
                        for i in 0..=(*end - *start) {
                            let port = start + i;
                            let rule_clone = rule.clone();
                            let offset = i;
                            let cache_clone = dns_cache.clone();
                            set.spawn(listen_udp_rule(rule_clone, port, offset, cache_clone));
                        }
                    }
                }
            }
        }

        loop {
            tokio::select! {
                biased;
                result = set.join_next() => {
                    match result {
                        None => break, // 所有任务均已退出
                        Some(Err(e)) if e.is_cancelled() => {} // abort 导致的正常取消
                        Some(_) => {
                            // 监听任务意外退出（返回或 panic），5s 后重启全部规则
                            log::warn!("监听任务意外退出，5s 后重启");
                            set.abort_all();
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            break;
                        }
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    if reload.swap(false, Ordering::Relaxed) {
                        log::info!("收到 SIGHUP，重载配置");
                        set.abort_all();
                        break;
                    }
                }
            }
        }
    }
}

// ── 单规则 TCP 监听（支持端口段） ─────────────────────────────────

async fn listen_rule(rule: ForwardRule, listen: Listen, state: SharedState, dns_cache: DnsCache) {
    match listen {
        Listen::Single(port) => {
            listen_single_tcp(rule, port, 0, state, dns_cache).await;
        }
        Listen::Range(start, end) => {
            let mut set: JoinSet<()> = JoinSet::new();
            for i in 0..=(end - start) {
                let port = start + i;
                let offset = i;
                let rule = rule.clone();
                let state = Arc::clone(&state);
                let cache = dns_cache.clone();
                set.spawn(async move {
                    listen_single_tcp(rule, port, offset, state, cache).await;
                });
            }
            // 等待所有子任务结束（通常不会结束，除非被 abort）
            while set.join_next().await.is_some() {}
        }
    }
}

/// 在单个端口上监听 TCP 连接，port_offset 用于端口段的目标端口偏移
async fn listen_single_tcp(
    rule: ForwardRule,
    port: u16,
    port_offset: u16,
    state: SharedState,
    dns_cache: DnsCache,
) {
    let bind = format!("0.0.0.0:{}", port);

    let listener = match TcpListener::bind(&bind).await {
        Ok(l) => l,
        Err(e) => { log::error!("TCP 监听 {} 失败: {}", bind, e); return; }
    };

    let label = rule.comment.as_deref().unwrap_or(&rule.listen).to_string();
    log::info!(
        "用户态 TCP 监听 {}  [{}]  ({})",
        bind, label,
        if cfg!(target_os = "linux") { "splice 零拷贝" } else { "copy 模式" }
    );

    loop {
        match listener.accept().await {
            Ok((client, peer)) => {
                let rule = rule.clone();
                let state = Arc::clone(&state);
                let cache = dns_cache.clone();
                tokio::spawn(relay(client, peer, rule, port_offset, state, cache));
            }
            Err(e) => {
                // EMFILE/ENFILE：FD 耗尽，等待已有连接释放资源后再重试
                if matches!(e.raw_os_error(), Some(libc::EMFILE) | Some(libc::ENFILE)) {
                    log::error!("文件描述符耗尽 {}，等待资源释放: {}", bind, e);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                } else {
                    log::error!("TCP accept 失败 {}: {}", bind, e);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }
}

// ── UDP 中继 ──────────────────────────────────────────────────────

/// 在单个端口上监听 UDP 数据报并中继到目标，port_offset 用于端口段的目标端口偏移
async fn listen_udp_rule(rule: ForwardRule, port: u16, port_offset: u16, dns_cache: DnsCache) {
    let bind = format!("0.0.0.0:{}", port);

    let local_sock = match UdpSocket::bind(&bind).await {
        Ok(s) => Arc::new(s),
        Err(e) => { log::error!("UDP 监听 {} 失败: {}", bind, e); return; }
    };

    let label = rule.comment.as_deref().unwrap_or(&rule.listen).to_string();
    log::info!("用户态 UDP 监听 {}  [{}]", bind, label);

    // client_addr → 已连接到目标的 UdpSocket
    let sessions: Arc<Mutex<HashMap<SocketAddr, Arc<UdpSocket>>>> =
        Arc::new(Mutex::new(HashMap::<SocketAddr, Arc<UdpSocket>>::new()));

    let mut buf = vec![0u8; UDP_BUF_SIZE];

    loop {
        // 接收来自客户端的数据报
        let (n, client_addr) = match local_sock.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(e) => {
                log::warn!("UDP recv_from {} 失败: {}", bind, e);
                continue;
            }
        };

        let data = buf[..n].to_vec();

        // 解析目标地址
        let to_str = pick_target(&rule.to, &rule.balance);
        if to_str.is_empty() { continue; }

        let target = match crate::config::Target::parse(&to_str) {
            Ok(t) => t,
            Err(e) => { log::warn!("目标解析失败 {}: {}", to_str, e); continue; }
        };

        // 端口段偏移
        let target_port = target.port_start.saturating_add(port_offset);

        // 使用 DNS 缓存解析目标地址
        let target_addr = match dns_cache.resolve(&target.host, target_port, rule.ipv6).await {
            Ok(a) => a,
            Err(e) => { log::warn!("UDP DNS 解析 {}:{} 失败: {}", target.host, target_port, e); continue; }
        };

        // 查找或创建 per-client 的出站 UdpSocket
        let target_sock: Arc<UdpSocket> = {
            // 先在锁内检查是否已有 session
            let existing = sessions.lock().unwrap().get(&client_addr).cloned();
            if let Some(s) = existing {
                s
            } else {
                // 绑定到随机端口，connect 到目标
                let sock = match UdpSocket::bind("0.0.0.0:0").await {
                    Ok(s) => s,
                    Err(e) => { log::warn!("UDP 出站 socket bind 失败: {}", e); continue; }
                };
                if let Err(e) = sock.connect(target_addr).await {
                    log::warn!("UDP connect {} 失败: {}", target_addr, e);
                    continue;
                }
                let sock = Arc::new(sock);
                sessions.lock().unwrap().insert(client_addr, sock.clone());

                // spawn 回包任务：target → client
                let sock_rx = sock.clone();
                let local_sock_tx = local_sock.clone();
                let sessions_gc = sessions.clone();
                tokio::spawn(async move {
                    let mut rbuf = vec![0u8; UDP_BUF_SIZE];
                    loop {
                        match timeout(UDP_IDLE_TIMEOUT, sock_rx.recv(&mut rbuf)).await {
                            Ok(Ok(m)) => {
                                if let Err(e) = local_sock_tx.send_to(&rbuf[..m], client_addr).await {
                                    log::debug!("UDP 回包 → {} 失败: {}", client_addr, e);
                                    break;
                                }
                            }
                            Ok(Err(e)) => {
                                log::debug!("UDP recv 失败 ({}): {}", client_addr, e);
                                break;
                            }
                            Err(_) => {
                                // 120s 空闲超时，清除 session
                                log::debug!("UDP session 超时，清除: {}", client_addr);
                                break;
                            }
                        }
                    }
                    // 从 sessions map 中移除
                    sessions_gc.lock().unwrap().remove(&client_addr);
                });

                sock
            }
        };

        // 转发数据到目标
        if let Err(e) = target_sock.send(&data).await {
            log::warn!("UDP 转发 → {} 失败: {}", target_addr, e);
        }
    }
}

// ── 单连接 TCP 转发 ───────────────────────────────────────────────

async fn relay(
    mut client: TcpStream,
    peer: SocketAddr,
    rule: ForwardRule,
    port_offset: u16,
    state: SharedState,
    dns_cache: DnsCache,
) {
    // Block 规则检查（精确 IP 或 CIDR，如 10.0.0.0/8）
    let peer_ip = peer.ip();
    let blocked = state.block_rules.iter().any(|b| {
        b.src.as_deref().map(|s| ip_matches(peer_ip, s)).unwrap_or(false)
    });
    if blocked {
        log::debug!("拦截来自 {} 的连接（block 规则）", peer);
        return;
    }

    // 速率限制：每次新连接消耗固定 token（64KB 作为基准单位）
    let key = rule.listen.clone();
    {
        let mut limiters = state.limiters.lock().unwrap();
        if let Some(bucket) = limiters.get_mut(&key) {
            if !bucket.consume(65536) {
                log::debug!("限速拦截来自 {} 的连接（规则 {}）", peer, key);
                return;
            }
        }
    }

    let to_str = pick_target(&rule.to, &rule.balance);
    if to_str.is_empty() { return; }

    let target = match crate::config::Target::parse(&to_str) {
        Ok(t) => t,
        Err(e) => { log::warn!("目标解析失败 {}: {}", to_str, e); return; }
    };

    // 端口段偏移
    let target_port = target.port_start.saturating_add(port_offset);

    // 使用 DNS 缓存解析目标地址
    let target_addr = match dns_cache.resolve(&target.host, target_port, rule.ipv6).await {
        Ok(a) => a,
        Err(e) => { log::warn!("DNS 解析 {}:{} 失败: {}", target.host, target_port, e); return; }
    };

    let mut server = match TcpStream::connect(target_addr).await {
        Ok(s) => s,
        Err(e) => {
            // 连接失败时使 DNS 缓存失效，下次重新解析
            dns_cache.invalidate(&target.host, target_port, rule.ipv6);
            log::warn!("连接 {} ({}) 失败: {}", to_str, target_addr, e);
            return;
        }
    };

    let result = do_relay(&mut client, &mut server).await;

    // 更新统计（bytes_in/out/connections，写入 /tmp/relay-rs.stats）
    let (c2s, s2c) = match result {
        Ok((c2s, s2c)) => {
            log::debug!("{} ↔ {}  ↑{} ↓{}", peer, to_str, fmt_bytes(c2s), fmt_bytes(s2c));
            (c2s, s2c)
        }
        Err(e) => {
            log::debug!("{} 断开: {}", peer, e);
            (0, 0)
        }
    };

    match state.stats.lock() {
        Ok(mut map) => {
            let s = map.entry(key).or_default();
            s.total_conns += 1;
            s.bytes_in += c2s;
            s.bytes_out += s2c;
        }
        Err(e) => log::warn!("统计锁获取失败: {}", e),
    }
}

// ── 转发策略分发 ──────────────────────────────────────────────────

async fn do_relay(client: &mut TcpStream, server: &mut TcpStream) -> io::Result<(u64, u64)> {
    #[cfg(target_os = "linux")]
    {
        return zero_copy::splice_bidirectional(client, server).await;
    }
    #[cfg(not(target_os = "linux"))]
    {
        let (mut cr, mut cw) = client.split();
        let (mut sr, mut sw) = server.split();
        let (c2s, s2c) = tokio::try_join!(
            copy_with_idle(&mut cr, &mut sw),
            copy_with_idle(&mut sr, &mut cw),
        )?;
        Ok((c2s, s2c))
    }
}

/// 单方向带空闲超时的 copy，EOF 时主动 shutdown 写端
#[cfg(not(target_os = "linux"))]
async fn copy_with_idle<R, W>(r: &mut R, w: &mut W) -> io::Result<u64>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut buf = [0u8; 8192];
    let mut total = 0u64;
    loop {
        let n = match timeout(TCP_IDLE_TIMEOUT, r.read(&mut buf)).await {
            Ok(Ok(0)) => { w.shutdown().await.ok(); return Ok(total); }
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(io::Error::new(io::ErrorKind::TimedOut, "TCP 空闲超时")),
        };
        w.write_all(&buf[..n]).await?;
        total += n as u64;
    }
}

// ── Linux 零拷贝实现 ──────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod zero_copy {
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
    use std::ptr;
    use tokio::net::TcpStream;

    const CHUNK: usize = 65536;

    /// 用 splice(2) 实现双向零拷贝转发。
    /// 直接复用 TcpStream 在 tokio reactor 中已注册的 epoll fd，
    /// 省去 dup，每连接节省 2 个 FD。
    pub async fn splice_bidirectional(
        client: &TcpStream,
        server: &TcpStream,
    ) -> io::Result<(u64, u64)> {
        let (c2s_rd, c2s_wr) = make_pipe()?;
        let (s2c_rd, s2c_wr) = make_pipe()?;

        let c_fd = client.as_raw_fd();
        let s_fd = server.as_raw_fd();

        let (c2s, s2c) = tokio::join!(
            splice_half(client, server, c_fd, s_fd, c2s_rd.as_raw_fd(), c2s_wr.as_raw_fd()),
            splice_half(server, client, s_fd, c_fd, s2c_rd.as_raw_fd(), s2c_wr.as_raw_fd()),
        );

        Ok((c2s?, s2c?))
    }

    /// 单方向 splice 循环：src_fd → pipe → dst_fd
    async fn splice_half(
        src: &TcpStream,
        dst: &TcpStream,
        src_fd: RawFd,
        dst_fd: RawFd,
        pipe_rd: RawFd,
        pipe_wr: RawFd,
    ) -> io::Result<u64> {
        let mut total = 0u64;

        loop {
            let n = loop {
                let mut guard = match tokio::time::timeout(
                    super::TCP_IDLE_TIMEOUT,
                    src.readable(),
                ).await {
                    Ok(Ok(g)) => g,
                    Ok(Err(e)) => return Err(e),
                    Err(_) => return Err(io::Error::new(io::ErrorKind::TimedOut, "TCP 空闲超时")),
                };
                let n = unsafe {
                    libc::splice(src_fd, ptr::null_mut(), pipe_wr, ptr::null_mut(), CHUNK,
                        libc::SPLICE_F_MOVE | libc::SPLICE_F_NONBLOCK)
                };
                if n == 0 { return Ok(total); }
                if n < 0 {
                    let e = io::Error::last_os_error();
                    if e.kind() == io::ErrorKind::WouldBlock {
                        guard.clear_ready();
                        continue;
                    }
                    return Err(e);
                }
                break n as usize;
            };

            let mut rem = n;
            while rem > 0 {
                let m = unsafe {
                    libc::splice(pipe_rd, ptr::null_mut(), dst_fd, ptr::null_mut(), rem,
                        libc::SPLICE_F_MOVE | libc::SPLICE_F_NONBLOCK)
                };
                if m < 0 {
                    let e = io::Error::last_os_error();
                    if e.kind() == io::ErrorKind::WouldBlock {
                        let mut guard = dst.writable().await?;
                        guard.clear_ready();
                        continue;
                    }
                    return Err(e);
                }
                if m == 0 {
                    return Err(io::Error::new(io::ErrorKind::BrokenPipe, "目标连接已关闭"));
                }
                rem -= m as usize;
            }

            total += n as u64;
        }
    }

    fn make_pipe() -> io::Result<(OwnedFd, OwnedFd)> {
        let mut fds = [0i32; 2];
        let ret = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) };
        if ret < 0 { return Err(io::Error::last_os_error()); }
        Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
    }
}

// ── 健康检查后台任务 ──────────────────────────────────────────────

/// 每 30s 对所有 TCP 目标做 TCP 探测，失败时使 DNS 缓存失效
async fn health_check_loop(rules: Vec<ForwardRule>, dns_cache: DnsCache) {
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;

        for rule in &rules {
            // UDP-only 规则无需 TCP 健康检查
            if matches!(rule.proto, Proto::Udp) { continue; }

            for to_str in &rule.to {
                let target = match crate::config::Target::parse(to_str) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                let addr = match dns_cache.resolve(&target.host, target.port_start, rule.ipv6).await {
                    Ok(a) => a,
                    Err(_) => continue,
                };
                match TcpStream::connect(addr).await {
                    Ok(_) => log::debug!("健康检查 OK: {}", to_str),
                    Err(e) => {
                        log::warn!("健康检查失败: {} ({}): {}", to_str, addr, e);
                        dns_cache.invalidate(&target.host, target.port_start, rule.ipv6);
                    }
                }
            }
        }
    }
}

// ── 工具函数 ──────────────────────────────────────────────────────

fn pick_target(targets: &[String], balance: &Option<Balance>) -> String {
    match targets.len() {
        0 => String::new(),
        1 => targets[0].clone(),
        n => {
            let idx = match balance.as_ref().unwrap_or(&Balance::RoundRobin) {
                Balance::RoundRobin => RR_CTR.fetch_add(1, Ordering::Relaxed) % n,
                Balance::Random => {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .subsec_nanos() as usize % n
                }
            };
            targets[idx].clone()
        }
    }
}

fn fmt_bytes(b: u64) -> String {
    if b >= 1 << 20      { format!("{:.1} MB", b as f64 / (1u64 << 20) as f64) }
    else if b >= 1 << 10 { format!("{:.1} KB", b as f64 / (1u64 << 10) as f64) }
    else                 { format!("{} B", b) }
}

/// 判断 IP 是否匹配 `spec`（精确 IP 或 CIDR，如 "1.2.3.4" 或 "10.0.0.0/8"）
fn ip_matches(ip: std::net::IpAddr, spec: &str) -> bool {
    use std::net::IpAddr;

    // 先尝试解析为精确 IP
    if let Ok(target) = spec.parse::<IpAddr>() {
        return ip == target;
    }

    // 解析 CIDR：拆分 "地址/前缀长度"
    let Some((addr_str, prefix_str)) = spec.split_once('/') else { return false };
    let Ok(prefix_len) = prefix_str.parse::<u32>() else { return false };

    match (ip, addr_str.parse::<IpAddr>()) {
        (IpAddr::V4(peer), Ok(IpAddr::V4(net))) => {
            if prefix_len > 32 { return false; }
            let mask = if prefix_len == 0 { 0u32 } else { !0u32 << (32 - prefix_len) };
            (u32::from(peer) & mask) == (u32::from(net) & mask)
        }
        (IpAddr::V6(peer), Ok(IpAddr::V6(net))) => {
            if prefix_len > 128 { return false; }
            let peer = u128::from(peer);
            let net  = u128::from(net);
            let mask = if prefix_len == 0 { 0u128 } else { !0u128 << (128 - prefix_len) };
            (peer & mask) == (net & mask)
        }
        _ => false, // 地址族不匹配
    }
}
