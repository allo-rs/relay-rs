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

    for (rule, ip) in resolved {
        match rule {
            Rule::Single {
                sport,
                dport,
                protocol,
                ip_version,
                comment,
                ..
            } => {
                let versions = resolve_versions(ip_version, ip);
                for version in versions {
                    append_single(
                        &mut s, *sport, *dport, ip, protocol, &version, comment,
                    );
                }
            }
            Rule::Range {
                sport_start,
                sport_end,
                dport_start,
                protocol,
                ip_version,
                comment,
                ..
            } => {
                // dport_start 未指定时与 sport_start 相同（仅改目标 IP，不改端口）
                let dport_begin = dport_start.unwrap_or(*sport_start);
                let dport_end =
                    dport_begin + (sport_end - sport_start);
                let versions = resolve_versions(ip_version, ip);
                for version in versions {
                    append_range(
                        &mut s,
                        *sport_start,
                        *sport_end,
                        dport_begin,
                        dport_end,
                        ip,
                        protocol,
                        &version,
                        comment,
                    );
                }
            }
        }
    }

    s
}

/// 将 IpVersion::All 展开为实际版本（根据解析到的 IP 类型判断）
fn resolve_versions(version: &IpVersion, ip: &str) -> Vec<IpVersion> {
    match version {
        IpVersion::Ipv4 => vec![IpVersion::Ipv4],
        IpVersion::Ipv6 => vec![IpVersion::Ipv6],
        IpVersion::All => {
            if ip.contains(':') {
                vec![IpVersion::Ipv6]
            } else {
                vec![IpVersion::Ipv4]
            }
        }
    }
}

fn family(version: &IpVersion) -> &'static str {
    match version {
        IpVersion::Ipv4 => "ip",
        IpVersion::Ipv6 => "ip6",
        IpVersion::All => unreachable!(),
    }
}

fn proto_expr(protocol: &Protocol) -> &'static str {
    match protocol {
        Protocol::Tcp => "tcp",
        Protocol::Udp => "udp",
        Protocol::All => "meta l4proto { tcp, udp } th",
    }
}

fn addr_match(version: &IpVersion, ip: &str) -> String {
    match version {
        IpVersion::Ipv4 => format!("ip daddr {}", ip),
        IpVersion::Ipv6 => format!("ip6 daddr {}", ip),
        IpVersion::All => unreachable!(),
    }
}

/// 生成 Single 规则的 PREROUTING + POSTROUTING
fn append_single(
    s: &mut String,
    sport: u16,
    dport: u16,
    ip: &str,
    protocol: &Protocol,
    version: &IpVersion,
    comment: &Option<String>,
) {
    let fam = family(version);
    let proto = proto_expr(protocol);
    let dnat_addr = fmt_dnat_addr(version, ip, dport);
    let addr = addr_match(version, ip);
    let cmt = comment
        .as_deref()
        .unwrap_or("single")
        .replace('"', "'");

    s.push_str(&format!(
        "add rule {fam} {TABLE} PREROUTING ct state new {proto} dport {sport} counter dnat to {dnat_addr} comment \"{cmt}\"\n"
    ));
    s.push_str(&format!(
        "add rule {fam} {TABLE} POSTROUTING ct state new {addr} {proto} dport {dport} counter masquerade comment \"{cmt}\"\n"
    ));
}

/// 生成 Range 规则的 PREROUTING + POSTROUTING
fn append_range(
    s: &mut String,
    sport_start: u16,
    sport_end: u16,
    dport_start: u16,
    dport_end: u16,
    ip: &str,
    protocol: &Protocol,
    version: &IpVersion,
    comment: &Option<String>,
) {
    let fam = family(version);
    let proto = proto_expr(protocol);
    let addr = addr_match(version, ip);
    let cmt = comment
        .as_deref()
        .unwrap_or("range")
        .replace('"', "'");

    // 端口段 DNAT：IPv6 地址需方括号
    let dnat_addr = match version {
        IpVersion::Ipv6 => format!("[{}]:{}-{}", ip, dport_start, dport_end),
        _ => format!("{}:{}-{}", ip, dport_start, dport_end),
    };

    s.push_str(&format!(
        "add rule {fam} {TABLE} PREROUTING ct state new {proto} dport {sport_start}-{sport_end} counter dnat to {dnat_addr} comment \"{cmt}\"\n"
    ));
    s.push_str(&format!(
        "add rule {fam} {TABLE} POSTROUTING ct state new {addr} {proto} dport {dport_start}-{dport_end} counter masquerade comment \"{cmt}\"\n"
    ));
}

/// 格式化 DNAT 目标地址（IPv6 需方括号）
fn fmt_dnat_addr(version: &IpVersion, ip: &str, port: u16) -> String {
    match version {
        IpVersion::Ipv6 => format!("[{}]:{}", ip, port),
        _ => format!("{}:{}", ip, port),
    }
}
