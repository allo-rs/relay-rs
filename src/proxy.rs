/// 用户态 TCP 代理模块
///
/// Linux：使用 splice(2) 零拷贝转发，数据在内核 pipe buffer ↔ socket buffer 间移动，
///        不经过 userspace 内存。
/// 其他平台：回退到 tokio::io::copy_bidirectional（userspace 8KB 缓冲区）。

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use tokio::io;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;

use crate::config::{Balance, ForwardRule, Listen, Proto};

static RR_CTR: AtomicUsize = AtomicUsize::new(0);

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

        if !cfg.block.is_empty() {
            log::warn!("用户态模式不支持 block 规则（已忽略），如需封禁 IP 请配置系统防火墙");
        }

        let mut set: JoinSet<()> = JoinSet::new();

        for rule in cfg.forward {
            if matches!(rule.proto, Proto::Udp) {
                log::warn!("用户态模式暂不支持 UDP，跳过: {}", rule.listen);
                continue;
            }
            let listen = match crate::config::Listen::parse(&rule.listen) {
                Ok(l) => l,
                Err(e) => { log::warn!("跳过规则 [{}]: {}", rule.listen, e); continue; }
            };
            if matches!(listen, Listen::Range(_, _)) {
                log::warn!("用户态模式暂不支持端口段，跳过: {}", rule.listen);
                continue;
            }
            set.spawn(listen_rule(rule, listen));
        }

        loop {
            if reload.swap(false, Ordering::Relaxed) {
                log::info!("收到 SIGHUP，重载配置");
                set.abort_all();
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

// ── 单规则监听 ────────────────────────────────────────────────────

async fn listen_rule(rule: ForwardRule, listen: Listen) {
    let port = match listen { Listen::Single(p) => p, Listen::Range(..) => return };
    let bind = format!("0.0.0.0:{}", port);

    let listener = match TcpListener::bind(&bind).await {
        Ok(l) => l,
        Err(e) => { log::error!("监听 {} 失败: {}", bind, e); return; }
    };

    let label = rule.comment.as_deref().unwrap_or(&rule.listen).to_string();
    log::info!(
        "用户态监听 {}  [{}]  ({})",
        bind, label,
        if cfg!(target_os = "linux") { "splice 零拷贝" } else { "copy 模式" }
    );

    loop {
        match listener.accept().await {
            Ok((client, peer)) => {
                let rule = rule.clone();
                tokio::spawn(relay(client, peer, rule));
            }
            Err(e) => {
                log::error!("accept 失败 {}: {}", bind, e);
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

// ── 单连接转发 ────────────────────────────────────────────────────

async fn relay(mut client: TcpStream, peer: SocketAddr, rule: ForwardRule) {
    let to_str = pick_target(&rule.to, &rule.balance);
    if to_str.is_empty() { return; }

    let target = match crate::config::Target::parse(&to_str) {
        Ok(t) => t,
        Err(e) => { log::warn!("目标解析失败 {}: {}", to_str, e); return; }
    };

    let lookup_str = format!("{}:{}", target.host, target.port_start);
    let addrs: Vec<SocketAddr> = match tokio::net::lookup_host(&lookup_str).await {
        Ok(iter) => iter.collect(),
        Err(e) => { log::warn!("DNS 解析 {} 失败: {}", lookup_str, e); return; }
    };

    let resolved = if rule.ipv6 {
        addrs.iter().find(|a| a.is_ipv6()).copied()
    } else {
        addrs.iter().find(|a| a.is_ipv4()).copied()
            .or_else(|| addrs.iter().find(|a| a.is_ipv6()).copied())
    };

    let target_addr = match resolved {
        Some(a) => a,
        None => { log::warn!("无法解析 {}", lookup_str); return; }
    };

    let mut server = match TcpStream::connect(target_addr).await {
        Ok(s) => s,
        Err(e) => { log::warn!("连接 {} ({}) 失败: {}", to_str, target_addr, e); return; }
    };

    let result = do_relay(&mut client, &mut server).await;

    match result {
        Ok((c2s, s2c)) => log::debug!(
            "{} ↔ {}  ↑{} ↓{}", peer, to_str, fmt_bytes(c2s), fmt_bytes(s2c)
        ),
        Err(e) => log::debug!("{} 断开: {}", peer, e),
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
        let (c2s, s2c) = io::copy_bidirectional(client, server).await?;
        Ok((c2s, s2c))
    }
}

// ── Linux 零拷贝实现 ──────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod zero_copy {
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
    use std::ptr;
    use tokio::io::unix::AsyncFd;
    use tokio::net::TcpStream;

    const CHUNK: usize = 65536; // 每次 splice 的最大字节数

    /// 用 splice(2) 实现双向零拷贝转发。
    /// 为每个方向建立一条内核 pipe，数据在 socket buffer ↔ pipe buffer 间移动，
    /// 全程不经过 userspace。
    pub async fn splice_bidirectional(
        client: &mut TcpStream,
        server: &mut TcpStream,
    ) -> io::Result<(u64, u64)> {
        // 为每个方向建立一对非阻塞 pipe
        let (c2s_rd, c2s_wr) = make_pipe()?;
        let (s2c_rd, s2c_wr) = make_pipe()?;

        // dup socket fd 用于 AsyncFd 就绪通知（不影响原 TcpStream 的生命周期）
        let c_afd = AsyncFd::new(dup_owned(client.as_raw_fd())?)?;
        let s_afd = AsyncFd::new(dup_owned(server.as_raw_fd())?)?;

        let c_fd = client.as_raw_fd();
        let s_fd = server.as_raw_fd();

        // 两个方向并发运行
        let (c2s, s2c) = tokio::join!(
            splice_half(c_fd, s_fd, &c_afd, &s_afd, c2s_rd.as_raw_fd(), c2s_wr.as_raw_fd()),
            splice_half(s_fd, c_fd, &s_afd, &c_afd, s2c_rd.as_raw_fd(), s2c_wr.as_raw_fd()),
        );

        Ok((c2s?, s2c?))
    }

    /// 单方向 splice 循环：src_fd → pipe → dst_fd
    async fn splice_half(
        src_fd: RawFd,
        dst_fd: RawFd,
        src_afd: &AsyncFd<OwnedFd>,
        dst_afd: &AsyncFd<OwnedFd>,
        pipe_rd: RawFd,
        pipe_wr: RawFd,
    ) -> io::Result<u64> {
        let mut total = 0u64;

        loop {
            // 等待 src 可读，然后 splice src → pipe
            let n = loop {
                let mut guard = src_afd.readable().await?;
                let n = unsafe {
                    libc::splice(
                        src_fd, ptr::null_mut(),
                        pipe_wr, ptr::null_mut(),
                        CHUNK,
                        libc::SPLICE_F_MOVE | libc::SPLICE_F_NONBLOCK,
                    )
                };
                if n == 0 { return Ok(total); } // EOF：对端关闭连接
                if n < 0 {
                    let e = io::Error::last_os_error();
                    if e.kind() == io::ErrorKind::WouldBlock {
                        guard.clear_ready(); // 清除就绪标记，等待下次 epoll 通知
                        continue;
                    }
                    return Err(e);
                }
                break n as usize;
            };

            // 把 pipe 里的数据全部 splice 到 dst
            let mut rem = n;
            while rem > 0 {
                let m = unsafe {
                    libc::splice(
                        pipe_rd, ptr::null_mut(),
                        dst_fd, ptr::null_mut(),
                        rem,
                        libc::SPLICE_F_MOVE | libc::SPLICE_F_NONBLOCK,
                    )
                };
                if m < 0 {
                    let e = io::Error::last_os_error();
                    if e.kind() == io::ErrorKind::WouldBlock {
                        // dst 暂时不可写，等待后重试
                        let mut guard = dst_afd.writable().await?;
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

    /// 创建一对非阻塞 pipe，返回 (read_end, write_end)
    fn make_pipe() -> io::Result<(OwnedFd, OwnedFd)> {
        let mut fds = [0i32; 2];
        let ret = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) };
        if ret < 0 { return Err(io::Error::last_os_error()); }
        Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
    }

    /// dup 一个 fd 并包装为 OwnedFd（用于 AsyncFd 注册，不影响原 fd 的所有权）
    fn dup_owned(fd: RawFd) -> io::Result<OwnedFd> {
        let dup = unsafe { libc::dup(fd) };
        if dup < 0 { return Err(io::Error::last_os_error()); }
        Ok(unsafe { OwnedFd::from_raw_fd(dup) })
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
