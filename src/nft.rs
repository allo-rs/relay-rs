use std::fs;
use std::process::Command;

use crate::config::{BlockRule, Chain, ForwardRule, Listen, Proto, Target};

const NFT_BIN: &str = "/usr/sbin/nft";
const SCRIPT_DIR: &str = "/etc/relay-rs";
const SCRIPT_PATH: &str = "/etc/relay-rs/rules.nft";
const NAT_TABLE: &str = "relay-nat";
const FILTER_TABLE: &str = "relay-filter";

// ── 已解析的转发规则（含 DNS 结果） ──────────────────────────────

pub struct ResolvedForward {
    pub rule: ForwardRule,
    pub listen: Listen,
    pub target: Target,
    pub ip: String,
}

// ── 应用规则 ─────────────────────────────────────────────────────

pub fn apply(
    forwards: &[ResolvedForward],
    blocks: &[BlockRule],
) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(SCRIPT_DIR)?;
    let script = build_script(forwards, blocks);
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

// ── 脚本生成 ─────────────────────────────────────────────────────

pub fn build_script(forwards: &[ResolvedForward], blocks: &[BlockRule]) -> String {
    let mut s = String::from("#!/usr/sbin/nft -f\n\n");

    // NAT 表
    for fam in ["ip", "ip6"] {
        s.push_str(&format!("add table {fam} {NAT_TABLE}\n"));
        s.push_str(&format!("delete table {fam} {NAT_TABLE}\n"));
        s.push_str(&format!("add table {fam} {NAT_TABLE}\n"));
        s.push_str(&format!(
            "add chain {fam} {NAT_TABLE} PREROUTING {{ type nat hook prerouting priority -110 ; }}\n"
        ));
        s.push_str(&format!(
            "add chain {fam} {NAT_TABLE} POSTROUTING {{ type nat hook postrouting priority 110 ; }}\n"
        ));
        s.push('\n');
    }

    // Filter 表
    for fam in ["ip", "ip6"] {
        s.push_str(&format!("add table {fam} {FILTER_TABLE}\n"));
        s.push_str(&format!("delete table {fam} {FILTER_TABLE}\n"));
        s.push_str(&format!("add table {fam} {FILTER_TABLE}\n"));
        s.push_str(&format!(
            "add chain {fam} {FILTER_TABLE} INPUT {{ type filter hook input priority filter - 1 ; }}\n"
        ));
        s.push_str(&format!(
            "add chain {fam} {FILTER_TABLE} FORWARD {{ type filter hook forward priority filter - 1 ; }}\n"
        ));
        s.push('\n');
    }

    // 转发规则
    for r in forwards {
        append_forward(&mut s, r);
    }

    // Block 规则
    for b in blocks {
        append_block(&mut s, b);
    }

    s
}

// ── 转发规则生成 ──────────────────────────────────────────────────

fn append_forward(s: &mut String, r: &ResolvedForward) {
    let is_ipv6 = r.ip.contains(':');
    let fam = if is_ipv6 { "ip6" } else { "ip" };
    let proto = proto_expr(&r.rule.proto);
    let cmt = r.rule.comment.as_deref().unwrap_or("forward").replace('"', "'");
    let addr_kw = if is_ipv6 { "ip6 daddr" } else { "ip daddr" };

    match &r.listen {
        Listen::Single(sport) => {
            let dnat = fmt_addr(is_ipv6, &r.ip, r.target.port_start);
            s.push_str(&format!(
                "add rule {fam} {NAT_TABLE} PREROUTING ct state new {proto} dport {sport} counter dnat to {dnat} comment \"{cmt}\"\n"
            ));
            s.push_str(&format!(
                "add rule {fam} {NAT_TABLE} POSTROUTING ct state new {addr_kw} {} {proto} dport {} counter masquerade comment \"{cmt}\"\n",
                r.ip, r.target.port_start
            ));
        }
        Listen::Range(sport_start, sport_end) => {
            let dport_start = r.target.port_start;
            let dport_end = dport_start + r.listen.size();
            let dnat = if is_ipv6 {
                format!("[{}]:{}-{}", r.ip, dport_start, dport_end)
            } else {
                format!("{}:{}-{}", r.ip, dport_start, dport_end)
            };
            s.push_str(&format!(
                "add rule {fam} {NAT_TABLE} PREROUTING ct state new {proto} dport {sport_start}-{sport_end} counter dnat to {dnat} comment \"{cmt}\"\n"
            ));
            s.push_str(&format!(
                "add rule {fam} {NAT_TABLE} POSTROUTING ct state new {addr_kw} {} {proto} dport {dport_start}-{dport_end} counter masquerade comment \"{cmt}\"\n",
                r.ip
            ));
        }
    }
}

// ── Block 规则生成 ────────────────────────────────────────────────

fn append_block(s: &mut String, b: &BlockRule) {
    let fam = if b.ipv6 { "ip6" } else { "ip" };
    let chain = match b.chain {
        Chain::Input => "INPUT",
        Chain::Forward => "FORWARD",
    };
    let cmt = b.comment.as_deref().unwrap_or("block").replace('"', "'");

    let mut conds: Vec<String> = Vec::new();

    let (saddr, daddr) = if b.ipv6 {
        ("ip6 saddr", "ip6 daddr")
    } else {
        ("ip saddr", "ip daddr")
    };

    if let Some(ip) = &b.src { conds.push(format!("{} {}", saddr, ip)); }
    if let Some(ip) = &b.dst { conds.push(format!("{} {}", daddr, ip)); }

    match &b.proto {
        Proto::Tcp | Proto::Udp => {
            let p = if matches!(b.proto, Proto::Tcp) { "tcp" } else { "udp" };
            if b.port.is_none() {
                conds.push(format!("meta l4proto {}", p));
            } else {
                conds.push(format!("{} dport {}", p, b.port.unwrap()));
            }
        }
        Proto::All => {
            if let Some(port) = b.port {
                conds.push(format!("th dport {}", port));
            }
        }
    }

    let cond_str = if conds.is_empty() {
        String::new()
    } else {
        format!("{} ", conds.join(" "))
    };

    s.push_str(&format!(
        "add rule {fam} {FILTER_TABLE} {chain} {cond_str}drop comment \"{cmt}\"\n"
    ));
}

// ── 工具函数 ──────────────────────────────────────────────────────

fn proto_expr(proto: &Proto) -> &'static str {
    match proto {
        Proto::Tcp => "tcp",
        Proto::Udp => "udp",
        Proto::All => "meta l4proto { tcp, udp } th",
    }
}

fn fmt_addr(is_ipv6: bool, ip: &str, port: u16) -> String {
    if is_ipv6 {
        format!("[{}]:{}", ip, port)
    } else {
        format!("{}:{}", ip, port)
    }
}
