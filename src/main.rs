mod config;
mod ip;
mod nft;

use clap::Parser;
use std::thread::sleep;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "relay-rs", about = "基于 nftables 的 NAT 端口转发守护进程", version)]
struct Args {
    /// 配置文件路径
    #[arg(short, long, default_value = "/etc/relay-rs/relay.toml")]
    config: String,

    /// 轮询间隔（秒）
    #[arg(short, long, default_value_t = 60)]
    interval: u64,
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    // 开启内核 IP 转发
    enable_ip_forwarding();

    log::info!("relay-rs 启动，配置文件: {}，轮询间隔: {}s", args.config, args.interval);

    let mut last_script = String::new();

    loop {
        match tick(&mut last_script, &args.config) {
            Ok(true) => log::info!("规则已更新并应用"),
            Ok(false) => log::debug!("规则无变化，跳过"),
            Err(e) => log::error!("{}", e),
        }
        sleep(Duration::from_secs(args.interval));
    }
}

/// 执行一次轮询：加载配置 → 解析 IP → 比对脚本 → 按需应用
fn tick(last_script: &mut String, config_path: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let config = config::load(config_path)?;

    if config.rules.is_empty() {
        log::warn!("配置中没有任何规则");
    }

    // 解析每条规则的目标 IP（解析失败的规则跳过并告警）
    let resolved: Vec<(config::Rule, String)> = config
        .rules
        .into_iter()
        .filter_map(|rule| {
            match ip::resolve(rule.target(), rule.ip_version()) {
                Ok(ips) => {
                    let ip = ips.into_iter().next().unwrap();
                    log::debug!("解析 {} → {}", rule.target(), ip);
                    Some((rule, ip))
                }
                Err(e) => {
                    log::warn!("跳过规则 (target={}): {}", rule.target(), e);
                    None
                }
            }
        })
        .collect();

    // 生成脚本并与上次比对
    let script = nft::build_script(&resolved);
    if script == *last_script {
        return Ok(false);
    }

    nft::apply(&resolved)?;
    *last_script = script;
    Ok(true)
}

fn enable_ip_forwarding() {
    for path in [
        "/proc/sys/net/ipv4/ip_forward",
        "/proc/sys/net/ipv6/conf/all/forwarding",
    ] {
        match std::fs::write(path, "1") {
            Ok(_) => log::debug!("已启用 {}", path),
            Err(e) => log::warn!("无法写入 {}: {}（非 root 运行？）", path, e),
        }
    }
}
