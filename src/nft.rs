use std::fs;
use std::process::Command;

use crate::config::{Chain, IpVersion, Protocol, Rule};

const NFT_BIN: &str = "/usr/sbin/nft";
const SCRIPT_DIR: &str = "/etc/relay-rs";
const SCRIPT_PATH: &str = "/etc/relay-rs/rules.nft";
const NAT_TABLE: &str = "relay-nat";
const FILTER_TABLE: &str = "relay-filter";

/// 生成并应用 nftables 规则脚本
pub fn apply(resolved: &[(Rule, Option<String>)]) -> Result<(), Box<dyn std::error::Error>> {
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
pub fn build_script(resolved: &[(Rule, Option<String>)]) -> String {
    let mut s = String::from("#!/usr/sbin/nft -f\n\n");

    // ── NAT 表（转发规则） ────────────────────────────────────────
    for family in ["ip", "ip6"] {
        s.push_str(&format!("add table {family} {NAT_TABLE}\n"));
        s.push_str(&format!("delete table {family} {NAT_TABLE}\n"));
        s.push_str(&format!("add table {family} {NAT_TABLE}\n"));
        s.push_str(&format!(
            "add chain {family} {NAT_TABLE} PREROUTING {{ type nat hook prerouting priority -110 ; }}\n"
        ));
        s.push_str(&format!(
            "add chain {family} {NAT_TABLE} POSTROUTING {{ type nat hook postrouting priority 110 ; }}\n"
        ));
        s.push('\n');
    }

    // ── Filter 表（Drop 规则） ────────────────────────────────────
    for family in ["ip", "ip6"] {
        s.push_str(&format!("add table {family} {FILTER_TABLE}\n"));
        s.push_str(&format!("delete table {family} {FILTER_TABLE}\n"));
        s.push_str(&format!("add table {family} {FILTER_TABLE}\n"));
        s.push_str(&format!(
            "add chain {family} {FILTER_TABLE} INPUT {{ type filter hook input priority filter - 1 ; }}\n"
        ));
        s.push_str(&format!(
            "add chain {family} {FILTER_TABLE} FORWARD {{ type filter hook forward priority filter - 1 ; }}\n"
        ));
        s.push('\n');
    }

    // ── 逐条生成规则 ──────────────────────────────────────────────
    for (rule, ip) in resolved {
        match rule {
            Rule::Single { sport, dport, protocol, ip_version, comment, .. } => {
                let resolved_ip = ip.as_deref().unwrap();
                for version in expand_version(ip_version, resolved_ip) {
                    append_single(&mut s, *sport, *dport, resolved_ip, protocol, &version, comment);
                }
            }
            Rule::Range { sport_start, sport_end, dport_start, protocol, ip_version, comment, .. } => {
                let resolved_ip = ip.as_deref().unwrap();
                let dport_begin = dport_start.unwrap_or(*sport_start);
                let dport_end = dport_begin + (sport_end - sport_start);
                for version in expand_version(ip_version, resolved_ip) {
                    append_range(
                        &mut s, *sport_start, *sport_end, dport_begin, dport_end,
                        resolved_ip, protocol, &version, comment,
                    );
                }
            }
            Rule::Drop { chain, src_ip, dst_ip, src_port, dst_port, protocol, ip_version, comment } => {
                for version in expand_version_static(ip_version) {
                    append_drop(
                        &mut s, chain, src_ip, dst_ip, *src_port, *dst_port,
                        protocol, &version, comment,
                    );
                }
            }
        }
    }

    s
}

// ── Drop 规则 ─────────────────────────────────────────────────────

fn append_drop(
    s: &mut String,
    chain: &Chain,
    src_ip: &Option<String>,
    dst_ip: &Option<String>,
    src_port: Option<u16>,
    dst_port: Option<u16>,
    protocol: &Protocol,
    version: &IpVersion,
    comment: &Option<String>,
) {
    let fam = family(version);
    let chain_name = match chain {
        Chain::Input => "INPUT",
        Chain::Forward => "FORWARD",
    };

    let mut conditions = Vec::new();

    // IP 匹配
    let (saddr_kw, daddr_kw) = match version {
        IpVersion::Ipv4 => ("ip saddr", "ip daddr"),
        IpVersion::Ipv6 => ("ip6 saddr", "ip6 daddr"),
        IpVersion::All => unreachable!(),
    };
    if let Some(ip) = src_ip { conditions.push(format!("{} {}", saddr_kw, ip)); }
    if let Some(ip) = dst_ip { conditions.push(format!("{} {}", daddr_kw, ip)); }

    // 协议 + 端口匹配
    match protocol {
        Protocol::Tcp | Protocol::Udp => {
            let p = if matches!(protocol, Protocol::Tcp) { "tcp" } else { "udp" };
            if src_port.is_none() && dst_port.is_none() {
                conditions.push(format!("meta l4proto {}", p));
            } else {
                if let Some(port) = src_port { conditions.push(format!("{} sport {}", p, port)); }
                if let Some(port) = dst_port { conditions.push(format!("{} dport {}", p, port)); }
            }
        }
        Protocol::All => {
            if let Some(port) = src_port { conditions.push(format!("th sport {}", port)); }
            if let Some(port) = dst_port { conditions.push(format!("th dport {}", port)); }
        }
    }

    let cmt = comment.as_deref().unwrap_or("drop").replace('"', "'");
    let cond_str = if conditions.is_empty() {
        String::new()
    } else {
        format!("{} ", conditions.join(" "))
    };

    s.push_str(&format!(
        "add rule {fam} {FILTER_TABLE} {chain_name} {cond_str}drop comment \"{cmt}\"\n"
    ));
}

// ── Single / Range 规则 ───────────────────────────────────────────

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
    let cmt = comment.as_deref().unwrap_or("single").replace('"', "'");

    s.push_str(&format!(
        "add rule {fam} {NAT_TABLE} PREROUTING ct state new {proto} dport {sport} counter dnat to {dnat_addr} comment \"{cmt}\"\n"
    ));
    s.push_str(&format!(
        "add rule {fam} {NAT_TABLE} POSTROUTING ct state new {addr} {proto} dport {dport} counter masquerade comment \"{cmt}\"\n"
    ));
}

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
    let cmt = comment.as_deref().unwrap_or("range").replace('"', "'");
    let dnat_addr = match version {
        IpVersion::Ipv6 => format!("[{}]:{}-{}", ip, dport_start, dport_end),
        _ => format!("{}:{}-{}", ip, dport_start, dport_end),
    };

    s.push_str(&format!(
        "add rule {fam} {NAT_TABLE} PREROUTING ct state new {proto} dport {sport_start}-{sport_end} counter dnat to {dnat_addr} comment \"{cmt}\"\n"
    ));
    s.push_str(&format!(
        "add rule {fam} {NAT_TABLE} POSTROUTING ct state new {addr} {proto} dport {dport_start}-{dport_end} counter masquerade comment \"{cmt}\"\n"
    ));
}

// ── 工具函数 ──────────────────────────────────────────────────────

/// 转发规则：根据解析到的 IP 展开版本
fn expand_version(version: &IpVersion, ip: &str) -> Vec<IpVersion> {
    match version {
        IpVersion::All => if ip.contains(':') { vec![IpVersion::Ipv6] } else { vec![IpVersion::Ipv4] },
        IpVersion::Ipv4 => vec![IpVersion::Ipv4],
        IpVersion::Ipv6 => vec![IpVersion::Ipv6],
    }
}

/// Drop 规则：直接按配置展开（不依赖 DNS）
fn expand_version_static(version: &IpVersion) -> Vec<IpVersion> {
    match version {
        IpVersion::All => vec![IpVersion::Ipv4, IpVersion::Ipv6],
        IpVersion::Ipv4 => vec![IpVersion::Ipv4],
        IpVersion::Ipv6 => vec![IpVersion::Ipv6],
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

fn fmt_dnat_addr(version: &IpVersion, ip: &str, port: u16) -> String {
    match version {
        IpVersion::Ipv6 => format!("[{}]:{}", ip, port),
        _ => format!("{}:{}", ip, port),
    }
}
