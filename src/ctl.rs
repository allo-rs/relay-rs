use crate::config::{BlockRule, Chain, Config, ForwardRule, Proto};
use std::io::{self, Write};

// ── 公共工具 ──────────────────────────────────────────────────────

/// 打印提示并读取一行输入，返回 default 若输入为空
fn prompt(msg: &str, default: &str) -> String {
    if default.is_empty() {
        print!("{}: ", msg);
    } else {
        print!("{} [{}]: ", msg, default);
    }
    io::stdout().flush().unwrap();
    let mut buf = String::new();
    io::stdin().read_line(&mut buf).unwrap();
    let input = buf.trim().to_string();
    if input.is_empty() { default.to_string() } else { input }
}

fn prompt_opt(msg: &str) -> Option<String> {
    let v = prompt(msg, "");
    if v.is_empty() { None } else { Some(v) }
}

fn parse_proto(s: &str) -> Proto {
    match s.to_lowercase().as_str() {
        "tcp" => Proto::Tcp,
        "udp" => Proto::Udp,
        _ => Proto::All,
    }
}

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
            let proto = match r.proto {
                Proto::All => String::new(),
                Proto::Tcp => "  tcp".to_string(),
                Proto::Udp => "  udp".to_string(),
            };
            let cmt = r.comment.as_deref().map(|s| format!("  # {}", s)).unwrap_or_default();
            println!("  #{:<3} {}  →  {}{}{}", idx, r.listen, r.to, proto, cmt);
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
            let chain = match b.chain {
                Chain::Input => String::new(),
                Chain::Forward => "  forward".to_string(),
            };
            let cmt = b.comment.as_deref().map(|s| format!("  # {}", s)).unwrap_or_default();
            println!("  #{:<3} {}{}{}", idx, parts.join("  "), chain, cmt);
            idx += 1;
        }
    }
}

// ── rr add ────────────────────────────────────────────────────────

pub fn add(config: &mut Config) -> Result<(), Box<dyn std::error::Error>> {
    let kind = prompt("类型 [forward/block]", "forward");

    match kind.as_str() {
        "block" => add_block(config),
        _ => add_forward(config),
    }
}

fn add_forward(config: &mut Config) -> Result<(), Box<dyn std::error::Error>> {
    let listen = prompt("本机端口（单端口 10000 或端口段 10000-10100）", "");
    if listen.is_empty() {
        return Err("本机端口不能为空".into());
    }
    let to = prompt("目标地址（host:port）", "");
    if to.is_empty() {
        return Err("目标地址不能为空".into());
    }
    let proto_str = prompt("协议 [all/tcp/udp]", "all");
    let ipv6_str  = prompt("使用 IPv6 [y/N]", "n");
    let comment   = prompt_opt("备注（可选）");

    config.forward.push(ForwardRule {
        listen,
        to,
        proto: parse_proto(&proto_str),
        ipv6: ipv6_str.to_lowercase() == "y",
        comment,
    });

    Ok(())
}

fn add_block(config: &mut Config) -> Result<(), Box<dyn std::error::Error>> {
    let src     = prompt_opt("源 IP 或 CIDR（可选，如 1.2.3.4 或 10.0.0.0/8）");
    let dst     = prompt_opt("目标 IP 或 CIDR（可选）");
    let port_s  = prompt_opt("目标端口（可选）");
    let port    = port_s.as_deref().and_then(|s| s.parse::<u16>().ok());
    let proto_s = prompt("协议 [all/tcp/udp]", "all");
    let chain_s = prompt("作用链 [input/forward]", "input");
    let ipv6_s  = prompt("匹配 IPv6 [y/N]", "n");
    let comment = prompt_opt("备注（可选）");

    if src.is_none() && dst.is_none() && port.is_none() {
        return Err("src / dst / port 至少填一项".into());
    }

    config.block.push(BlockRule {
        src,
        dst,
        port,
        proto: parse_proto(&proto_s),
        chain: if chain_s.to_lowercase() == "forward" { Chain::Forward } else { Chain::Input },
        ipv6: ipv6_s.to_lowercase() == "y",
        comment,
    });

    Ok(())
}

// ── rr del ────────────────────────────────────────────────────────

pub fn del(config: &mut Config, index: usize) -> Result<(), Box<dyn std::error::Error>> {
    let total = config.forward.len() + config.block.len();
    if index == 0 || index > total {
        return Err(format!("序号 {} 超出范围（共 {} 条规则）", index, total).into());
    }

    if index <= config.forward.len() {
        let removed = config.forward.remove(index - 1);
        println!("已删除转发规则: {} → {}", removed.listen, removed.to);
    } else {
        let bi = index - config.forward.len() - 1;
        let removed = config.block.remove(bi);
        let desc = removed.src.as_deref()
            .or(removed.dst.as_deref())
            .unwrap_or("block rule");
        println!("已删除防火墙规则: {}", desc);
    }

    Ok(())
}
