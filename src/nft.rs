use std::fs;
use std::process::Command;

use crate::config::{Balance, BlockRule, Chain, ForwardRule, Listen, Proto, Target};

const NFT_BIN: &str = "/usr/sbin/nft";
const SCRIPT_DIR: &str = "/etc/relay-rs";
const SCRIPT_PATH: &str = "/etc/relay-rs/rules.nft";
const NAT_TABLE: &str = "relay-nat";
const FILTER_TABLE: &str = "relay-filter";

// ── 已解析的转发规则（含 DNS 结果） ──────────────────────────────

pub struct ResolvedTarget {
    pub target: Target,
    pub ip: String,
}

pub struct ResolvedForward {
    pub rule: ForwardRule,
    pub listen: Listen,
    /// 健康检查后存活的目标列表（单目标或多目标）
    pub targets: Vec<ResolvedTarget>,
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

    for r in forwards {
        append_forward(&mut s, r);
    }

    for b in blocks {
        append_block(&mut s, b);
    }

    s
}

// ── 转发规则生成 ──────────────────────────────────────────────────

fn append_forward(s: &mut String, r: &ResolvedForward) {
    if r.targets.is_empty() { return; }

    let cmt   = r.rule.comment.as_deref().unwrap_or("forward").replace('"', "'");
    let proto = proto_expr(&r.rule.proto);

    // 以第一个目标的地址族为准（所有目标应同族）
    let is_ipv6 = r.targets[0].ip.contains(':');
    let fam     = if is_ipv6 { "ip6" } else { "ip" };
    let addr_kw = if is_ipv6 { "ip6 daddr" } else { "ip daddr" };

    // 带宽限速：对该端口所有数据包计速，超速直接丢弃
    if let Some(mbps) = r.rule.rate_limit {
        let kbytes_per_sec = mbps * 1000 / 8;
        let sport = listen_range_expr(&r.listen);
        s.push_str(&format!(
            "add rule {fam} {NAT_TABLE} PREROUTING {proto} dport {sport} \
             limit rate over {kbytes_per_sec} kbytes/second counter drop comment \"{cmt}\"\n"
        ));
    }

    match r.targets.len() {
        1 => append_single(s, r, &r.targets[0], fam, proto, addr_kw, &cmt, is_ipv6),
        _ => append_multi(s, r, fam, proto, addr_kw, &cmt, is_ipv6),
    }
}

/// 单目标转发（原有逻辑）
fn append_single(
    s: &mut String,
    r: &ResolvedForward,
    t: &ResolvedTarget,
    fam: &str, proto: &str, addr_kw: &str, cmt: &str, is_ipv6: bool,
) {
    match &r.listen {
        Listen::Single(sport) => {
            let dnat = fmt_addr(is_ipv6, &t.ip, t.target.port_start);
            s.push_str(&format!(
                "add rule {fam} {NAT_TABLE} PREROUTING ct state new {proto} dport {sport} \
                 counter dnat to {dnat} comment \"{cmt}\"\n"
            ));
            s.push_str(&format!(
                "add rule {fam} {NAT_TABLE} POSTROUTING ct state new {addr_kw} {} {proto} dport {} \
                 counter masquerade comment \"{cmt}\"\n",
                t.ip, t.target.port_start
            ));
        }
        Listen::Range(sport_start, sport_end) => {
            let dport_end = t.target.port_start + r.listen.size();
            let dnat = if is_ipv6 {
                format!("[{}]:{}-{}", t.ip, t.target.port_start, dport_end)
            } else {
                format!("{}:{}-{}", t.ip, t.target.port_start, dport_end)
            };
            s.push_str(&format!(
                "add rule {fam} {NAT_TABLE} PREROUTING ct state new {proto} dport {sport_start}-{sport_end} \
                 counter dnat to {dnat} comment \"{cmt}\"\n"
            ));
            s.push_str(&format!(
                "add rule {fam} {NAT_TABLE} POSTROUTING ct state new {addr_kw} {} {proto} dport {}-{} \
                 counter masquerade comment \"{cmt}\"\n",
                t.ip, t.target.port_start, dport_end
            ));
        }
    }
}

/// 多目标负载均衡（numgen），仅支持单端口
fn append_multi(
    s: &mut String,
    r: &ResolvedForward,
    fam: &str, proto: &str, addr_kw: &str, cmt: &str, is_ipv6: bool,
) {
    let sport = match &r.listen {
        Listen::Single(p) => *p,
        Listen::Range(_, _) => {
            log::warn!("多目标负载均衡不支持端口段，跳过规则: {}", r.rule.listen);
            return;
        }
    };

    let n    = r.targets.len();
    let mode = match r.rule.balance.as_ref().unwrap_or(&Balance::RoundRobin) {
        Balance::RoundRobin => "inc",
        Balance::Random     => "random",
    };

    // numgen map 条目：0 : IP:PORT, 1 : IP:PORT, ...
    let entries = r.targets.iter().enumerate()
        .map(|(i, t)| format!("{} : {}", i, fmt_addr(is_ipv6, &t.ip, t.target.port_start)))
        .collect::<Vec<_>>()
        .join(", ");

    s.push_str(&format!(
        "add rule {fam} {NAT_TABLE} PREROUTING ct state new {proto} dport {sport} \
         counter dnat to numgen {mode} mod {n} map {{ {entries} }} comment \"{cmt}\"\n"
    ));

    // 每个目标各自添加 masquerade
    for t in &r.targets {
        s.push_str(&format!(
            "add rule {fam} {NAT_TABLE} POSTROUTING ct state new {addr_kw} {} {proto} dport {} \
             counter masquerade comment \"{cmt}\"\n",
            t.ip, t.target.port_start
        ));
    }
}

// ── Block 规则生成 ────────────────────────────────────────────────

fn append_block(s: &mut String, b: &BlockRule) {
    let fam   = if b.ipv6 { "ip6" } else { "ip" };
    let chain = match b.chain { Chain::Input => "INPUT", Chain::Forward => "FORWARD" };
    let cmt   = b.comment.as_deref().unwrap_or("block").replace('"', "'");

    let mut conds: Vec<String> = Vec::new();

    let (saddr, daddr) = if b.ipv6 { ("ip6 saddr", "ip6 daddr") } else { ("ip saddr", "ip daddr") };

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

    let cond_str = if conds.is_empty() { String::new() } else { format!("{} ", conds.join(" ")) };
    s.push_str(&format!(
        "add rule {fam} {FILTER_TABLE} {chain} {cond_str}drop comment \"{cmt}\"\n"
    ));
}

// ── 清理残留规则（切换到用户态模式时调用） ────────────────────────

pub fn clear_tables() {
    for fam in ["ip", "ip6"] {
        for table in [NAT_TABLE, FILTER_TABLE] {
            std::process::Command::new(NFT_BIN)
                .args(["delete", "table", fam, table])
                .output()
                .ok(); // 表不存在时忽略错误
        }
    }
    log::info!("已清理 nftables 规则表");
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
    if is_ipv6 { format!("[{}]:{}", ip, port) } else { format!("{}:{}", ip, port) }
}

fn listen_range_expr(listen: &Listen) -> String {
    match listen {
        Listen::Single(p)    => p.to_string(),
        Listen::Range(s, e)  => format!("{}-{}", s, e),
    }
}
