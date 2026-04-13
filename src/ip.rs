use std::io;
use std::net::IpAddr;
use std::sync::OnceLock;

static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

fn rt() -> &'static tokio::runtime::Runtime {
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("无法初始化 DNS runtime")
    })
}

/// 解析域名，返回 (IP 地址, TTL 秒数)。
/// 静态 IP 地址返回 TTL = u32::MAX（视为永不过期）。
pub fn resolve_with_ttl(host: &str, ipv6: bool) -> io::Result<(String, u32)> {
    // 直接是 IP 地址，不走 DNS
    if let Ok(ip) = host.parse::<IpAddr>() {
        if ipv6 && ip.is_ipv4() {
            return Err(io::Error::other(format!("{} 是 IPv4 地址，但配置了 ipv6 = true", host)));
        }
        return Ok((ip.to_string(), u32::MAX));
    }

    let host = host.to_string();
    rt().block_on(async move {
        use hickory_resolver::TokioAsyncResolver;
        use hickory_resolver::config::{ResolverConfig, ResolverOpts};

        let resolver = TokioAsyncResolver::tokio(ResolverConfig::default(), ResolverOpts::default());

        if ipv6 {
            let lookup = resolver.ipv6_lookup(&host).await
                .map_err(|e| io::Error::other(format!("DNS 解析 {} 失败: {}", host, e)))?;
            let ttl = lookup.as_lookup().records().iter()
                .map(|r: &hickory_resolver::proto::rr::Record| r.ttl())
                .min().unwrap_or(60);
            let ip = lookup.iter().next()
                .map(|ip| ip.to_string())
                .ok_or_else(|| io::Error::other(format!("无法解析 {} 的 IPv6 地址", host)))?;
            Ok((ip, ttl))
        } else {
            let lookup = resolver.lookup_ip(&host).await
                .map_err(|e| io::Error::other(format!("DNS 解析 {} 失败: {}", host, e)))?;
            let ttl = lookup.as_lookup().records().iter()
                .map(|r: &hickory_resolver::proto::rr::Record| r.ttl())
                .min().unwrap_or(60);
            let ip = lookup.iter()
                .find(|ip| ip.is_ipv4())
                .or_else(|| lookup.iter().next())
                .map(|ip| ip.to_string())
                .ok_or_else(|| io::Error::other(format!("无法解析 {} 的 IP 地址", host)))?;
            Ok((ip, ttl))
        }
    })
}

/// 解析域名为 IP 地址字符串（不返回 TTL）。
pub fn resolve(host: &str, ipv6: bool) -> io::Result<String> {
    resolve_with_ttl(host, ipv6).map(|(ip, _)| ip)
}
