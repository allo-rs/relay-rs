use std::fs;
use std::process::Command;

use crate::config::{IpVersion, Protocol, Rule};

const NFT_BIN: &str = "/usr/sbin/nft";
const SCRIPT_DIR: &str = "/etc/relay-rs";
const SCRIPT_PATH: &str = "/etc/relay-rs/rules.nft";
const TABLE: &str = "relay-nat";

/// 生成并应用 nftables 规则脚本
pub fn apply(resolved: &[(Rule, String)]) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(SCRIPT_DIR)?;
    let script = build_script(resolved);
    fs::write(SCRIPT_PATH, &script)?;

    let output = Command::new(NFT_BIN)
        .arg("-f")
        .arg(SCRIPT_PATH)
        .output()
        .map_err(|e| format!("执行 nft 失败（是否已安装 nftables？）: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("nft 返回错误:\n{}", stderr).into());
    }

    Ok(())
}

/// 生成 nftables 脚本字符串（供外部比对变化）
pub fn build_script(resolved: &[(Rule, String)]) -> String {
    let mut s = String::from("#!/usr/sbin/nft -f\n\n");

    // 重建 IPv4 和 IPv6 的 NAT 表
    for family in ["ip", "ip6"] {
        s.push_str(&format!("add table {family} {TABLE}\n"));
        s.push_str(&format!("delete table {family} {TABLE}\n"));
        s.push_str(&format!("add table {family} {TABLE}\n"));
        s.push_str(&format!(
            "add chain {family} {TABLE} PREROUTING {{ type nat hook prerouting priority -110 ; }}\n"
        ));
        s.push_str(&format!(
            "add chain {family} {TABLE} POSTROUTING {{ type nat hook postrouting priority 110 ; }}\n"
        ));
        s.push('\n');
    }

    // 逐条生成 Single 转发规则
    for (rule, ip) in resolved {
        let versions = expand_ip_version(&rule.ip_version, ip);
        for version in versions {
            append_single_rule(&mut s, rule, ip, &version);
        }
    }

    s
}

/// 将 IpVersion::All 展开为实际版本列表（根据解析到的 IP 类型）
fn expand_ip_version(version: &IpVersion, ip: &str) -> Vec<IpVersion> {
    match version {
        IpVersion::Ipv4 => vec![IpVersion::Ipv4],
        IpVersion::Ipv6 => vec![IpVersion::Ipv6],
        IpVersion::All => {
            // 根据实际解析到的 IP 判断
            if ip.contains(':') {
                vec![IpVersion::Ipv6]
            } else {
                vec![IpVersion::Ipv4]
            }
        }
    }
}

/// 生成一条 Single 规则对应的 PREROUTING + POSTROUTING 行
fn append_single_rule(s: &mut String, rule: &Rule, ip: &str, version: &IpVersion) {
    let family = match version {
        IpVersion::Ipv4 => "ip",
        IpVersion::Ipv6 => "ip6",
        IpVersion::All => unreachable!(),
    };

    let proto = match rule.protocol {
        Protocol::Tcp => "tcp dport".to_string(),
        Protocol::Udp => "udp dport".to_string(),
        Protocol::All => "meta l4proto { tcp, udp } th dport".to_string(),
    };

    // IPv6 地址需要用方括号包裹
    let dnat_addr = match version {
        IpVersion::Ipv6 => format!("[{}]:{}", ip, rule.dport),
        _ => format!("{}:{}", ip, rule.dport),
    };

    let addr_match = match version {
        IpVersion::Ipv4 => format!("ip daddr {}", ip),
        IpVersion::Ipv6 => format!("ip6 daddr {}", ip),
        IpVersion::All => unreachable!(),
    };

    let comment = rule
        .comment
        .clone()
        .unwrap_or_else(|| format!("{}→{}:{}", rule.sport, rule.target, rule.dport));

    // PREROUTING: DNAT
    s.push_str(&format!(
        "add rule {family} {TABLE} PREROUTING ct state new {proto} {sport} counter dnat to {dnat_addr} comment \"{comment}\"\n",
        sport = rule.sport,
    ));

    // POSTROUTING: SNAT (masquerade 自动使用出口 IP)
    s.push_str(&format!(
        "add rule {family} {TABLE} POSTROUTING ct state new {addr_match} {proto} {dport} counter masquerade comment \"{comment}\"\n",
        dport = rule.dport,
    ));
}
