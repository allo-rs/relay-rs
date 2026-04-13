use std::io;
use std::net::{IpAddr, ToSocketAddrs};

use crate::config::IpVersion;

/// 将域名或 IP 字符串解析为目标地址列表。
/// - 若输入已是合法 IP，直接返回（验证版本匹配）
/// - 否则进行 DNS 查询，根据 ip_version 筛选结果
pub fn resolve(target: &str, ip_version: &IpVersion) -> io::Result<Vec<String>> {
    // 直接解析为 IP 地址
    if let Ok(ip) = target.parse::<IpAddr>() {
        return match ip_version {
            IpVersion::Ipv4 if ip.is_ipv4() => Ok(vec![ip.to_string()]),
            IpVersion::Ipv6 if ip.is_ipv6() => Ok(vec![ip.to_string()]),
            IpVersion::All => Ok(vec![ip.to_string()]),
            _ => Err(io::Error::other(format!(
                "{} 是 {} 地址，与配置的 ip_version 不匹配",
                target,
                if ip.is_ipv4() { "IPv4" } else { "IPv6" }
            ))),
        };
    }

    // DNS 查询（拼 :80 只是为了满足 to_socket_addrs 接口）
    let addrs: Vec<_> = format!("{}:80", target)
        .to_socket_addrs()
        .map_err(|e| io::Error::other(format!("DNS 解析 {} 失败: {}", target, e)))?
        .collect();

    let ips: Vec<String> = match ip_version {
        IpVersion::Ipv4 => addrs
            .iter()
            .filter(|a| a.is_ipv4())
            .map(|a| a.ip().to_string())
            .collect(),
        IpVersion::Ipv6 => addrs
            .iter()
            .filter(|a| a.is_ipv6())
            .map(|a| a.ip().to_string())
            .collect(),
        IpVersion::All => {
            // 优先 IPv4，没有再取 IPv6
            let v4: Vec<_> = addrs
                .iter()
                .filter(|a| a.is_ipv4())
                .map(|a| a.ip().to_string())
                .collect();
            if !v4.is_empty() {
                v4
            } else {
                addrs
                    .iter()
                    .filter(|a| a.is_ipv6())
                    .map(|a| a.ip().to_string())
                    .collect()
            }
        }
    };

    if ips.is_empty() {
        return Err(io::Error::other(format!(
            "无法解析 {} 的 {} 地址",
            target,
            match ip_version {
                IpVersion::Ipv4 => "IPv4",
                IpVersion::Ipv6 => "IPv6",
                IpVersion::All => "任意",
            }
        )));
    }

    Ok(ips)
}
