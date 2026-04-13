use std::io;
use std::net::{IpAddr, ToSocketAddrs};

/// 将域名或 IP 解析为地址列表。
/// - `ipv6 = false`：优先 IPv4，没有再取 IPv6
/// - `ipv6 = true`：只取 IPv6
pub fn resolve(host: &str, ipv6: bool) -> io::Result<String> {
    // 直接解析为 IP 地址
    if let Ok(ip) = host.parse::<IpAddr>() {
        if ipv6 && ip.is_ipv4() {
            return Err(io::Error::other(format!("{} 是 IPv4 地址，但配置了 ipv6 = true", host)));
        }
        return Ok(ip.to_string());
    }

    // DNS 查询
    let addrs: Vec<_> = format!("{}:80", host)
        .to_socket_addrs()
        .map_err(|e| io::Error::other(format!("DNS 解析 {} 失败: {}", host, e)))?
        .collect();

    if ipv6 {
        addrs.iter()
            .find(|a| a.is_ipv6())
            .map(|a| a.ip().to_string())
            .ok_or_else(|| io::Error::other(format!("无法解析 {} 的 IPv6 地址", host)))
    } else {
        // 优先 IPv4，没有再取 IPv6
        addrs.iter()
            .find(|a| a.is_ipv4())
            .or_else(|| addrs.iter().find(|a| a.is_ipv6()))
            .map(|a| a.ip().to_string())
            .ok_or_else(|| io::Error::other(format!("无法解析 {} 的 IP 地址", host)))
    }
}
