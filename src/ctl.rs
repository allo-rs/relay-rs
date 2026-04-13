use crate::config::{BlockRule, Chain, Config, ForwardRule, Proto};
use dialoguer::{Confirm, Input, Select, theme::ColorfulTheme};

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

    let to: String = Input::with_theme(theme)
        .with_prompt("目标地址（host:port）")
        .interact_text()?;

    let proto_idx = Select::with_theme(theme)
        .with_prompt("协议")
        .items(&["all（默认）", "tcp", "udp"])
        .default(0)
        .interact()?;

    let proto = match proto_idx {
        1 => Proto::Tcp,
        2 => Proto::Udp,
        _ => Proto::All,
    };

    let comment: String = Input::with_theme(theme)
        .with_prompt("备注（可选，直接回车跳过）")
        .allow_empty(true)
        .interact_text()?;

    config.forward.push(ForwardRule {
        listen,
        to,
        proto,
        ipv6: false,
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

    let src = if src.is_empty() { None } else { Some(src) };
    let dst = if dst.is_empty() { None } else { Some(dst) };

    if src.is_none() && dst.is_none() && port.is_none() {
        return Err("src / dst / port 至少填一项".into());
    }

    let proto_idx = Select::with_theme(theme)
        .with_prompt("协议")
        .items(&["all（默认）", "tcp", "udp"])
        .default(0)
        .interact()?;

    let proto = match proto_idx {
        1 => Proto::Tcp,
        2 => Proto::Udp,
        _ => Proto::All,
    };

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
        src,
        dst,
        port,
        proto,
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

    // 构建选项列表
    let mut items: Vec<String> = Vec::new();
    for r in &config.forward {
        let cmt = r.comment.as_deref().map(|s| format!(" # {}", s)).unwrap_or_default();
        items.push(format!("[转发] {}  →  {}{}", r.listen, r.to, cmt));
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

    // 最后一项是取消
    if selection == total {
        println!("已取消");
        return Ok(false);
    }

    if selection < config.forward.len() {
        let removed = config.forward.remove(selection);
        println!("已删除: [转发] {} → {}", removed.listen, removed.to);
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

// ── 确认提示 ──────────────────────────────────────────────────────

pub fn confirm(msg: &str, default: bool) -> bool {
    Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(msg)
        .default(default)
        .interact()
        .unwrap_or(false)
}
