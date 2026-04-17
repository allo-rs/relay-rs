use std::net::TcpStream;
use std::time::Duration;

use dialoguer::{Confirm, Input, Select, theme::ColorfulTheme};

use crate::config::{Balance, BlockRule, Chain, Config, ForwardMode, ForwardRule, Proto};
use crate::ip;

// ── rr list ───────────────────────────────────────────────────────

pub fn list(config: &Config) {
    let total = config.forward.len() + config.block.len();
    if total == 0 {
        println!("（暂无规则）");
        return;
    }

    let mut idx = 1usize;

    if !config.forward.is_empty() {
        println!("转发规则:");
        for r in &config.forward {
            let proto_hint = match r.proto {
                Proto::All => String::new(),
                Proto::Tcp => "  tcp".to_string(),
                Proto::Udp => "  udp".to_string(),
            };
            let to_display = r.to.join(", ");
            let balance_hint = if r.to.len() > 1 {
                let b = match r.balance.as_ref().unwrap_or(&Balance::RoundRobin) {
                    Balance::RoundRobin => "round-robin",
                    Balance::Random     => "random",
                };
                format!("  [{}]", b)
            } else {
                String::new()
            };
            let rate_hint = r.rate_limit
                .map(|mbps| format!("  [rate: {} Mbps]", mbps))
                .unwrap_or_default();
            let cmt = r.comment.as_deref().map(|s| format!("  # {}", s)).unwrap_or_default();
            println!("  #{:<3} {}  →  {}{}{}{}{}", idx, r.listen, to_display, proto_hint, balance_hint, rate_hint, cmt);
            idx += 1;
        }
    }

    if !config.block.is_empty() {
        if !config.forward.is_empty() { println!(); }
        println!("防火墙规则:");
        for b in &config.block {
            let mut parts = Vec::new();
            if let Some(s) = &b.src  { parts.push(format!("src={}", s)); }
            if let Some(d) = &b.dst  { parts.push(format!("dst={}", d)); }
            if let Some(p) = b.port  { parts.push(format!("port={}", p)); }
            if b.proto != Proto::All {
                parts.push(format!("proto={}", match b.proto {
                    Proto::Tcp => "tcp", Proto::Udp => "udp", Proto::All => "all",
                }));
            }
            let chain = match b.chain { Chain::Input => String::new(), Chain::Forward => "  forward".to_string() };
            let cmt = b.comment.as_deref().map(|s| format!("  # {}", s)).unwrap_or_default();
            println!("  #{:<3} {}{}{}", idx, parts.join("  "), chain, cmt);
            idx += 1;
        }
    }
}

// ── rr ping ──────────────────────────────────────────────────────

pub fn ping(target: &str) {
    probe_target(target, false, &Proto::All);
}

// ── rr check ─────────────────────────────────────────────────────

pub fn check(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    if config.forward.is_empty() {
        println!("（暂无转发规则）");
        return Ok(());
    }

    let theme = ColorfulTheme::default();

    // 构建选项，首项为「检查全部」
    let mut items: Vec<String> = vec!["检查全部规则".to_string()];
    for r in &config.forward {
        let to_display = r.to.join(", ");
        let cmt = r.comment.as_deref().map(|s| format!(" # {}", s)).unwrap_or_default();
        items.push(format!("{}  →  {}{}", r.listen, to_display, cmt));
    }

    let selection = Select::with_theme(&theme)
        .with_prompt("选择要检查的规则（↑↓ 选择，回车确认）")
        .items(&items)
        .default(0)
        .interact()?;

    println!();

    let rules_to_check: Vec<&ForwardRule> = if selection == 0 {
        config.forward.iter().collect()
    } else {
        vec![&config.forward[selection - 1]]
    };

    for rule in rules_to_check {
        let label = rule.comment.as_deref().unwrap_or(&rule.listen);
        println!("[ {} ]", label);

        for to_str in &rule.to {
            probe_target(to_str, rule.ipv6, &rule.proto);
        }
        println!();
    }

    Ok(())
}

fn probe_target(to_str: &str, ipv6: bool, proto: &Proto) {
    let target = match crate::config::Target::parse(to_str) {
        Ok(t) => t,
        Err(e) => { println!("  ✗  {} — 解析失败: {}", to_str, e); return; }
    };

    // DNS 解析
    let ip = match ip::resolve(&target.host, ipv6) {
        Ok(ip) => ip,
        Err(e) => { println!("  ✗  {} — DNS 失败: {}", to_str, e); return; }
    };

    // UDP 规则无法 TCP 探测
    if matches!(proto, Proto::Udp) {
        println!("  ?  {}  (→ {})  UDP 规则跳过 TCP 探测", to_str, ip);
        return;
    }

    let addr_str = if ip.contains(':') {
        format!("[{}]:{}", ip, target.port_start)
    } else {
        format!("{}:{}", ip, target.port_start)
    };

    match addr_str.parse::<std::net::SocketAddr>() {
        Ok(addr) => {
            let t0 = std::time::Instant::now();
            match TcpStream::connect_timeout(&addr, Duration::from_secs(5)) {
                Ok(_)  => println!("  ✓  {}  (→ {})  {}ms", to_str, ip, t0.elapsed().as_millis()),
                Err(e) => {
                    use std::io::ErrorKind::*;
                    let reason = match e.kind() {
                        ConnectionRefused  => "端口未开放".to_string(),
                        TimedOut           => format!("连接超时（{}ms）", t0.elapsed().as_millis()),
                        NetworkUnreachable => "网络不可达".to_string(),
                        HostUnreachable    => "主机不可达".to_string(),
                        ConnectionReset    => "连接被重置".to_string(),
                        _                  => e.to_string(),
                    };
                    println!("  ✗  {}  (→ {})  {}", to_str, ip, reason);
                }
            }
        }
        Err(e) => println!("  ✗  {} — 地址解析失败: {}", to_str, e),
    }
}

// ── rr add ────────────────────────────────────────────────────────

pub fn add(config: &mut Config) -> Result<(), Box<dyn std::error::Error>> {
    let theme = ColorfulTheme::default();

    let kind = Select::with_theme(&theme)
        .with_prompt("规则类型")
        .items(&["转发规则 (forward)", "防火墙规则 (block)"])
        .default(0)
        .interact()?;

    match kind {
        0 => add_forward(config, &theme),
        _ => add_block(config, &theme),
    }
}

fn add_forward(config: &mut Config, theme: &ColorfulTheme) -> Result<(), Box<dyn std::error::Error>> {
    let listen: String = Input::with_theme(theme)
        .with_prompt("本机端口（单端口 10000 或端口段 10000-10100）")
        .interact_text()?;

    // 第一个目标
    let first: String = Input::with_theme(theme)
        .with_prompt("目标地址（host:port）")
        .interact_text()?;
    let mut to_list = vec![first];

    // 追加更多目标（负载均衡）
    while Confirm::with_theme(theme)
        .with_prompt("继续添加目标（负载均衡）？")
        .default(false)
        .interact()?
    {
        let extra: String = Input::with_theme(theme)
            .with_prompt("额外目标地址（host:port）")
            .interact_text()?;
        to_list.push(extra);
    }

    // 多目标：询问负载均衡策略
    let balance = if to_list.len() > 1 {
        let idx = Select::with_theme(theme)
            .with_prompt("负载均衡策略")
            .items(&["round-robin（轮询，默认）", "random（随机）"])
            .default(0)
            .interact()?;
        Some(if idx == 1 { Balance::Random } else { Balance::RoundRobin })
    } else {
        None
    };

    let proto_idx = Select::with_theme(theme)
        .with_prompt("协议")
        .items(&["all（默认）", "tcp", "udp"])
        .default(0)
        .interact()?;

    let proto = match proto_idx { 1 => Proto::Tcp, 2 => Proto::Udp, _ => Proto::All };

    let ip_ver = Select::with_theme(theme)
        .with_prompt("目标域名解析方式")
        .items(&["IPv4（默认）", "IPv6"])
        .default(0)
        .interact()?;

    // 带宽限速（可选）
    let rate_str: String = Input::with_theme(theme)
        .with_prompt("带宽限速 Mbps（可选，如 200，回车跳过）")
        .allow_empty(true)
        .interact_text()?;
    let rate_limit: Option<u32> = if rate_str.is_empty() {
        None
    } else {
        match rate_str.trim().parse::<u32>() {
            Ok(v) => Some(v),
            Err(_) => {
                eprintln!("无效数字，跳过限速");
                None
            }
        }
    };

    let comment: String = Input::with_theme(theme)
        .with_prompt("备注（可选，直接回车跳过）")
        .allow_empty(true)
        .interact_text()?;

    config.forward.push(ForwardRule {
        listen,
        to: to_list,
        proto,
        ipv6: ip_ver == 1,
        balance,
        rate_limit,
        comment: if comment.is_empty() { None } else { Some(comment) },
    });

    Ok(())
}

fn add_block(config: &mut Config, theme: &ColorfulTheme) -> Result<(), Box<dyn std::error::Error>> {
    let src: String = Input::with_theme(theme)
        .with_prompt("源 IP 或 CIDR（可选，直接回车跳过）")
        .allow_empty(true)
        .interact_text()?;

    let dst: String = Input::with_theme(theme)
        .with_prompt("目标 IP 或 CIDR（可选）")
        .allow_empty(true)
        .interact_text()?;

    let port: String = Input::with_theme(theme)
        .with_prompt("目标端口（可选）")
        .allow_empty(true)
        .interact_text()?;

    let port = port.parse::<u16>().ok();
    let src  = if src.is_empty() { None } else { Some(src) };
    let dst  = if dst.is_empty() { None } else { Some(dst) };

    if src.is_none() && dst.is_none() && port.is_none() {
        return Err("src / dst / port 至少填一项".into());
    }

    let proto_idx = Select::with_theme(theme)
        .with_prompt("协议")
        .items(&["all（默认）", "tcp", "udp"])
        .default(0)
        .interact()?;

    let proto = match proto_idx { 1 => Proto::Tcp, 2 => Proto::Udp, _ => Proto::All };

    let chain_idx = Select::with_theme(theme)
        .with_prompt("作用链")
        .items(&["input（入站，默认）", "forward（转发）"])
        .default(0)
        .interact()?;

    let comment: String = Input::with_theme(theme)
        .with_prompt("备注（可选）")
        .allow_empty(true)
        .interact_text()?;

    config.block.push(BlockRule {
        src, dst, port, proto,
        chain: if chain_idx == 1 { Chain::Forward } else { Chain::Input },
        ipv6: false,
        comment: if comment.is_empty() { None } else { Some(comment) },
    });

    Ok(())
}

// ── rr del ────────────────────────────────────────────────────────

/// 返回 Ok(true) 表示已删除，Ok(false) 表示用户取消
pub fn del(config: &mut Config) -> Result<bool, Box<dyn std::error::Error>> {
    let total = config.forward.len() + config.block.len();
    if total == 0 {
        println!("（暂无规则）");
        return Ok(false);
    }

    let mut items: Vec<String> = Vec::new();
    for r in &config.forward {
        let to_display = r.to.join(", ");
        let cmt = r.comment.as_deref().map(|s| format!(" # {}", s)).unwrap_or_default();
        items.push(format!("[转发] {}  →  {}{}", r.listen, to_display, cmt));
    }
    for b in &config.block {
        let mut parts = Vec::new();
        if let Some(s) = &b.src  { parts.push(format!("src={}", s)); }
        if let Some(d) = &b.dst  { parts.push(format!("dst={}", d)); }
        if let Some(p) = b.port  { parts.push(format!("port={}", p)); }
        let cmt = b.comment.as_deref().map(|s| format!(" # {}", s)).unwrap_or_default();
        items.push(format!("[防火墙] {}{}", parts.join(" "), cmt));
    }
    items.push("取消".to_string());

    let theme = ColorfulTheme::default();
    let selection = Select::with_theme(&theme)
        .with_prompt("选择要删除的规则（↑↓ 选择，回车确认）")
        .items(&items)
        .default(0)
        .interact()?;

    if selection == total {
        println!("已取消");
        return Ok(false);
    }

    let preview = &items[selection];
    if !Confirm::with_theme(&theme)
        .with_prompt(format!("确认删除 {}？", preview))
        .default(false)
        .interact()?
    {
        println!("已取消");
        return Ok(false);
    }

    if selection < config.forward.len() {
        let removed = config.forward.remove(selection);
        let to_display = removed.to.join(", ");
        println!("已删除: [转发] {} → {}", removed.listen, to_display);
    } else {
        let bi = selection - config.forward.len();
        let removed = config.block.remove(bi);
        let desc = removed.src.as_deref()
            .or(removed.dst.as_deref())
            .map(|s| s.to_string())
            .or(removed.port.map(|p| format!("port={}", p)))
            .unwrap_or_else(|| "block".to_string());
        println!("已删除: [防火墙] {}", desc);
    }

    Ok(true)
}

// ── rr edit ──────────────────────────────────────────────────────

/// 返回 Ok(true) 表示已编辑，Ok(false) 表示用户取消
pub fn edit(config: &mut Config) -> Result<bool, Box<dyn std::error::Error>> {
    let total = config.forward.len() + config.block.len();
    if total == 0 {
        println!("（暂无规则）");
        return Ok(false);
    }

    let mut items: Vec<String> = Vec::new();
    for r in &config.forward {
        let to_display = r.to.join(", ");
        let cmt = r.comment.as_deref().map(|s| format!(" # {}", s)).unwrap_or_default();
        items.push(format!("[转发] {}  →  {}{}", r.listen, to_display, cmt));
    }
    for b in &config.block {
        let mut parts = Vec::new();
        if let Some(s) = &b.src  { parts.push(format!("src={}", s)); }
        if let Some(d) = &b.dst  { parts.push(format!("dst={}", d)); }
        if let Some(p) = b.port  { parts.push(format!("port={}", p)); }
        let cmt = b.comment.as_deref().map(|s| format!(" # {}", s)).unwrap_or_default();
        items.push(format!("[防火墙] {}{}", parts.join(" "), cmt));
    }
    items.push("取消".to_string());

    let theme = ColorfulTheme::default();
    let selection = Select::with_theme(&theme)
        .with_prompt("选择要编辑的规则（↑↓ 选择，回车确认）")
        .items(&items)
        .default(0)
        .interact()?;

    if selection == total {
        println!("已取消");
        return Ok(false);
    }

    if selection < config.forward.len() {
        let rule = config.forward[selection].clone();
        config.forward[selection] = edit_forward(rule, &theme)?;
    } else {
        let bi = selection - config.forward.len();
        let rule = config.block[bi].clone();
        config.block[bi] = edit_block(rule, &theme)?;
    }

    Ok(true)
}

fn edit_forward(rule: ForwardRule, theme: &ColorfulTheme) -> Result<ForwardRule, Box<dyn std::error::Error>> {
    let listen: String = Input::with_theme(theme)
        .with_prompt("本机端口（单端口 10000 或端口段 10000-10100）")
        .with_initial_text(&rule.listen)
        .interact_text()?;

    // 逐个编辑已有目标地址
    let mut to_list: Vec<String> = Vec::new();
    for (i, existing) in rule.to.iter().enumerate() {
        let target: String = Input::with_theme(theme)
            .with_prompt(format!("目标地址 {}", i + 1))
            .with_initial_text(existing)
            .interact_text()?;
        to_list.push(target);
    }

    // 追加额外目标
    while Confirm::with_theme(theme)
        .with_prompt("继续添加目标（负载均衡）？")
        .default(false)
        .interact()?
    {
        let extra: String = Input::with_theme(theme)
            .with_prompt("额外目标地址（host:port）")
            .interact_text()?;
        to_list.push(extra);
    }

    let balance = if to_list.len() > 1 {
        let current = match rule.balance.as_ref().unwrap_or(&Balance::RoundRobin) {
            Balance::RoundRobin => 0,
            Balance::Random     => 1,
        };
        let idx = Select::with_theme(theme)
            .with_prompt("负载均衡策略")
            .items(&["round-robin（轮询，默认）", "random（随机）"])
            .default(current)
            .interact()?;
        Some(if idx == 1 { Balance::Random } else { Balance::RoundRobin })
    } else {
        None
    };

    let current_proto = match rule.proto { Proto::All => 0, Proto::Tcp => 1, Proto::Udp => 2 };
    let proto_idx = Select::with_theme(theme)
        .with_prompt("协议")
        .items(&["all（默认）", "tcp", "udp"])
        .default(current_proto)
        .interact()?;
    let proto = match proto_idx { 1 => Proto::Tcp, 2 => Proto::Udp, _ => Proto::All };

    let ip_ver = Select::with_theme(theme)
        .with_prompt("目标域名解析方式")
        .items(&["IPv4（默认）", "IPv6"])
        .default(if rule.ipv6 { 1 } else { 0 })
        .interact()?;

    let rate_str: String = Input::with_theme(theme)
        .with_prompt("带宽限速 Mbps（可选，回车清除）")
        .with_initial_text(rule.rate_limit.map(|v| v.to_string()).unwrap_or_default())
        .allow_empty(true)
        .interact_text()?;
    let rate_limit = if rate_str.is_empty() {
        None
    } else {
        match rate_str.trim().parse::<u32>() {
            Ok(v) => Some(v),
            Err(_) => { eprintln!("无效数字，跳过限速"); None }
        }
    };

    let comment: String = Input::with_theme(theme)
        .with_prompt("备注（可选，回车清除）")
        .with_initial_text(rule.comment.as_deref().unwrap_or(""))
        .allow_empty(true)
        .interact_text()?;

    Ok(ForwardRule {
        listen,
        to: to_list,
        proto,
        ipv6: ip_ver == 1,
        balance,
        rate_limit,
        comment: if comment.is_empty() { None } else { Some(comment) },
    })
}

fn edit_block(rule: BlockRule, theme: &ColorfulTheme) -> Result<BlockRule, Box<dyn std::error::Error>> {
    let src: String = Input::with_theme(theme)
        .with_prompt("源 IP 或 CIDR（可选，回车清除）")
        .with_initial_text(rule.src.as_deref().unwrap_or(""))
        .allow_empty(true)
        .interact_text()?;

    let dst: String = Input::with_theme(theme)
        .with_prompt("目标 IP 或 CIDR（可选）")
        .with_initial_text(rule.dst.as_deref().unwrap_or(""))
        .allow_empty(true)
        .interact_text()?;

    let port: String = Input::with_theme(theme)
        .with_prompt("目标端口（可选）")
        .with_initial_text(rule.port.map(|p| p.to_string()).unwrap_or_default())
        .allow_empty(true)
        .interact_text()?;

    let port = port.parse::<u16>().ok();
    let src  = if src.is_empty() { None } else { Some(src) };
    let dst  = if dst.is_empty() { None } else { Some(dst) };

    if src.is_none() && dst.is_none() && port.is_none() {
        return Err("src / dst / port 至少填一项".into());
    }

    let current_proto = match rule.proto { Proto::All => 0, Proto::Tcp => 1, Proto::Udp => 2 };
    let proto_idx = Select::with_theme(theme)
        .with_prompt("协议")
        .items(&["all（默认）", "tcp", "udp"])
        .default(current_proto)
        .interact()?;
    let proto = match proto_idx { 1 => Proto::Tcp, 2 => Proto::Udp, _ => Proto::All };

    let chain_default = match rule.chain { Chain::Input => 0, Chain::Forward => 1 };
    let chain_idx = Select::with_theme(theme)
        .with_prompt("作用链")
        .items(&["input（入站，默认）", "forward（转发）"])
        .default(chain_default)
        .interact()?;

    let comment: String = Input::with_theme(theme)
        .with_prompt("备注（可选）")
        .with_initial_text(rule.comment.as_deref().unwrap_or(""))
        .allow_empty(true)
        .interact_text()?;

    Ok(BlockRule {
        src, dst, port, proto,
        chain: if chain_idx == 1 { Chain::Forward } else { Chain::Input },
        ipv6: rule.ipv6,
        comment: if comment.is_empty() { None } else { Some(comment) },
    })
}

// ── rr stats ─────────────────────────────────────────────────────

pub fn stats(config_path: &str) {
    let mut found = false;
    for family in ["ip", "ip6"] {
        let output = std::process::Command::new("nft")
            .args(["list", "table", family, "relay-nat"])
            .output();

        let Ok(out) = output else { continue };
        if !out.status.success() { continue }

        let text = String::from_utf8_lossy(&out.stdout);
        let entries = parse_counters(&text);
        if entries.is_empty() { continue }

        if !found {
            println!("{:<4} {:<30} {:>10} {:>12}", "#", "规则", "包数", "流量");
            println!("{}", "─".repeat(60));
            found = true;
        }

        for (i, e) in entries.iter().enumerate() {
            println!(
                "{:<4} {:<30} {:>10} {:>12}",
                i + 1,
                truncate(&e.comment, 30),
                format_packets(e.packets),
                format_bytes(e.bytes),
            );
        }
    }

    if !found {
        println!("暂无统计数据（服务是否已启动？）");
    }

    // relay 用户态代理模式统计（仅 relay 模式下显示）
    let is_relay = crate::config::load(config_path)
        .map(|c| c.mode == crate::config::ForwardMode::Relay)
        .unwrap_or(false);
    if is_relay {
        if let Ok(content) = std::fs::read_to_string("/tmp/relay-rs.stats") {
            if let Ok(map) = serde_json::from_str::<std::collections::HashMap<String, serde_json::Value>>(&content) {
                if !map.is_empty() {
                    println!();
                    println!("Relay 模式统计:");
                    println!("{:<20} {:>12} {:>12} {:>10}", "监听端口", "流入", "流出", "连接数");
                    println!("{}", "─".repeat(58));
                    for (port, val) in &map {
                        let conns  = val["total_conns"].as_u64().unwrap_or(0);
                        let b_in   = val["bytes_in"].as_u64().unwrap_or(0);
                        let b_out  = val["bytes_out"].as_u64().unwrap_or(0);
                        println!(
                            "{:<20} {:>12} {:>12} {:>10}",
                            port,
                            format_bytes(b_in),
                            format_bytes(b_out),
                            conns,
                        );
                    }
                }
            }
        }
    }
}

struct CounterEntry { comment: String, packets: u64, bytes: u64 }

fn parse_counters(text: &str) -> Vec<CounterEntry> {
    let mut in_prerouting = false;
    let mut entries = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.contains("chain PREROUTING") { in_prerouting = true; continue; }
        if trimmed.starts_with("chain ") { in_prerouting = false; continue; }
        if !in_prerouting { continue; }
        if !trimmed.contains("counter packets") { continue; }
        // 跳过限速丢包规则（limit rate ... drop），仅统计流量 counter 专用规则
        if trimmed.contains("limit rate") { continue; }

        entries.push(CounterEntry {
            packets: extract_u64(trimmed, "packets"),
            bytes:   extract_u64(trimmed, "bytes"),
            comment: extract_comment(trimmed),
        });
    }

    entries
}

fn extract_u64(s: &str, keyword: &str) -> u64 {
    s.split_whitespace()
        .skip_while(|w| *w != keyword)
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

fn extract_comment(s: &str) -> String {
    if let Some(start) = s.find("comment \"") {
        let rest = &s[start + 9..];
        if let Some(end) = rest.find('"') { return rest[..end].to_string(); }
    }
    if let Some(idx) = s.find("dnat to ") {
        return s[idx..].split_whitespace().take(3).collect::<Vec<_>>().join(" ");
    }
    "—".to_string()
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max { return s.to_string(); }
    // 避免切在 UTF-8 多字节字符中间：回退到最近的 char boundary
    let mut end = max.saturating_sub(3);
    while end > 0 && !s.is_char_boundary(end) { end -= 1; }
    format!("{}…", &s[..end])
}

fn format_packets(n: u64) -> String {
    if n >= 1_000_000 { format!("{:.1}M", n as f64 / 1_000_000.0) }
    else if n >= 1_000 { format!("{:.1}K", n as f64 / 1_000.0) }
    else { n.to_string() }
}

fn format_bytes(b: u64) -> String {
    if b >= 1 << 30 { format!("{:.2} GB", b as f64 / (1u64 << 30) as f64) }
    else if b >= 1 << 20 { format!("{:.2} MB", b as f64 / (1u64 << 20) as f64) }
    else if b >= 1 << 10 { format!("{:.2} KB", b as f64 / (1u64 << 10) as f64) }
    else { format!("{} B", b) }
}

// ── rr mode ──────────────────────────────────────────────────────

/// 返回 Ok(true) 表示需要重启服务
pub fn mode_cmd(mut config: Config, config_path: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let theme = ColorfulTheme::default();

    let current = match config.mode {
        ForwardMode::Nat   => 0usize,
        ForwardMode::Relay => 1,
    };

    let idx = Select::with_theme(&theme)
        .with_prompt(format!(
            "转发模式（当前: {}）",
            if current == 0 { "nat" } else { "relay" }
        ))
        .items(&[
            "nat    — nftables DNAT，内核直转，性能最优（推荐）",
            "relay  — tokio + splice 零拷贝，无需 root，支持复杂场景",
        ])
        .default(current)
        .interact()?;

    let new_mode = if idx == 0 { ForwardMode::Nat } else { ForwardMode::Relay };

    if new_mode == config.mode {
        println!("模式未变更（{}）", if idx == 0 { "nat" } else { "relay" });
        return Ok(false);
    }

    config.mode = new_mode;
    crate::config::save(&config, config_path)?;
    println!("已切换 → {}", if idx == 0 { "nat" } else { "relay" });
    println!();
    Ok(true)
}

// ── 确认提示 ──────────────────────────────────────────────────────

pub fn confirm(msg: &str, default: bool) -> bool {
    Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(msg)
        .default(default)
        .interact()
        .unwrap_or(false)
}
