mod config;
mod ctl;
mod ip;
mod nft;

use clap::{CommandFactory, Parser, Subcommand};
use nft::{ResolvedForward, ResolvedTarget};
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::Duration;

const SERVICE: &str = "relay-rs";
const CONFIG_PATH: &str = "/etc/relay-rs/relay.toml";
/// 健康检查 TCP 连接超时
const HEALTH_TIMEOUT: Duration = Duration::from_secs(3);
/// 轮询间隔下界：避免极短 TTL 导致高频 DNS 查询
const MIN_INTERVAL: u64 = 15;
/// 轮询间隔上界
const MAX_INTERVAL: u64 = 300;

#[derive(Parser)]
#[command(name = "relay-rs", about = "基于 nftables 的 NAT 端口转发守护进程", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// 配置文件路径
    #[arg(short, long, default_value = CONFIG_PATH, global = true)]
    config: String,

    /// 守护进程轮询间隔上限（秒），TTL 较短时会自动缩短
    #[arg(short, long, default_value_t = 60, global = true)]
    interval: u64,
}

#[derive(Subcommand)]
enum Command {
    /// 启动服务
    Start,
    /// 停止服务
    Stop,
    /// 重启服务
    Restart,
    /// 查看服务状态
    Status,
    /// 实时查看日志
    Log,
    /// 编辑配置文件
    Config,
    /// 编辑配置并重启服务
    Reload,
    /// 列出所有规则
    List,
    /// 交互式添加规则
    Add,
    /// 交互式删除规则
    Del,
    /// 检查转发规则连通性
    Check,
    /// 查看各规则流量统计
    Stats,
    /// 以守护进程模式运行（供 systemd 调用）
    #[command(hide = true)]
    Daemon,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Some(cmd) => run_ctl(cmd, &cli.config, cli.interval),
        None => {
            // 无子命令时打印帮助，避免误触守护进程模式覆盖 nftables 规则
            let mut cmd = Cli::command();
            cmd.print_help().unwrap();
            println!();
        }
    }
}

// ── 管理子命令 ────────────────────────────────────────────────────

fn run_ctl(cmd: Command, config: &str, interval: u64) {
    match cmd {
        Command::Start   => systemctl("start"),
        Command::Stop    => systemctl("stop"),
        Command::Restart => systemctl("restart"),
        Command::Status  => systemctl("status"),
        Command::Log     => journalctl(),
        Command::Config  => edit_config(config),
        Command::Reload  => { edit_config(config); systemctl("restart"); }
        Command::Stats   => ctl::stats(),
        Command::Daemon  => run_daemon(config, interval),
        Command::Check   => {
            match config::load(config) {
                Ok(cfg) => {
                    if let Err(e) = ctl::check(&cfg) {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                }
                Err(e) => { eprintln!("{}", e); std::process::exit(1); }
            }
        }
        Command::List    => {
            match config::load(config) {
                Ok(cfg) => ctl::list(&cfg),
                Err(e)  => { eprintln!("{}", e); std::process::exit(1); }
            }
        }
        Command::Add => {
            let mut cfg = config::load(config).unwrap_or_default();
            match ctl::add(&mut cfg) {
                Ok(_) => match config::save(&cfg, config) {
                    Ok(_) => {
                        ctl::list(&cfg);
                        println!();
                        if ctl::confirm("立即重启服务使规则生效？[Y/n]", true) {
                            systemctl("restart");
                        }
                    }
                    Err(e) => { eprintln!("保存失败: {}", e); std::process::exit(1); }
                },
                Err(e) => { eprintln!("错误: {}", e); std::process::exit(1); }
            }
        }
        Command::Del => {
            let mut cfg = match config::load(config) {
                Ok(c) => c,
                Err(e) => { eprintln!("{}", e); std::process::exit(1); }
            };
            match ctl::del(&mut cfg) {
                Ok(true) => match config::save(&cfg, config) {
                    Ok(_) => {
                        println!();
                        ctl::list(&cfg);
                        println!();
                        if ctl::confirm("立即重启服务使规则生效？[Y/n]", true) {
                            systemctl("restart");
                        }
                    }
                    Err(e) => { eprintln!("保存失败: {}", e); std::process::exit(1); }
                },
                Ok(false) => {}
                Err(e) => { eprintln!("错误: {}", e); std::process::exit(1); }
            }
        }
    }
}

fn systemctl(action: &str) {
    let status = std::process::Command::new("systemctl")
        .args([action, SERVICE])
        .status()
        .unwrap_or_else(|e| { eprintln!("执行 systemctl 失败: {}", e); std::process::exit(1); });
    std::process::exit(status.code().unwrap_or(1));
}

fn journalctl() {
    let status = std::process::Command::new("journalctl")
        .args(["-u", SERVICE, "-f"])
        .status()
        .unwrap_or_else(|e| { eprintln!("执行 journalctl 失败: {}", e); std::process::exit(1); });
    std::process::exit(status.code().unwrap_or(1));
}

fn edit_config(config: &str) {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());
    std::process::Command::new(&editor)
        .arg(config)
        .status()
        .unwrap_or_else(|e| { eprintln!("打开编辑器 {} 失败: {}", editor, e); std::process::exit(1); });
}

// ── 守护进程模式 ──────────────────────────────────────────────────

fn run_daemon(config_path: &str, interval: u64) {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    enable_ip_forwarding();
    log::info!("relay-rs 启动，配置: {}，最大轮询间隔: {}s", config_path, interval);

    // 注册 SIGHUP 用于热重载（kill -HUP <pid>）
    let reload = Arc::new(AtomicBool::new(false));
    let reload_tx = Arc::clone(&reload);
    match signal_hook::iterator::Signals::new([signal_hook::consts::SIGHUP]) {
        Ok(mut signals) => {
            std::thread::spawn(move || {
                for _ in signals.forever() {
                    reload_tx.store(true, Ordering::Relaxed);
                }
            });
            log::info!("已注册 SIGHUP 热重载");
        }
        Err(e) => log::warn!("无法注册 SIGHUP: {}，热重载不可用", e),
    }

    let mut last_script = String::new();
    let mut next_sleep  = interval.clamp(MIN_INTERVAL, MAX_INTERVAL);

    loop {
        if reload.swap(false, Ordering::Relaxed) {
            log::info!("收到 SIGHUP，立即重新加载配置");
        }

        match tick(&mut last_script, config_path) {
            Ok((true, ttl))  => { log::info!("规则已更新并应用"); next_sleep = calc_interval(ttl, interval); }
            Ok((false, ttl)) => { log::debug!("规则无变化，跳过"); next_sleep = calc_interval(ttl, interval); }
            Err(e)           => log::error!("{}", e),
        }
        log::debug!("下次检查: {}s 后", next_sleep);

        // 分段睡眠，每秒检查一次 SIGHUP
        for _ in 0..next_sleep {
            if reload.load(Ordering::Relaxed) { break; }
            sleep(Duration::from_secs(1));
        }
    }
}

/// 根据最小 TTL 和用户配置计算实际轮询间隔
fn calc_interval(min_ttl: Option<u64>, configured: u64) -> u64 {
    match min_ttl {
        Some(ttl) => ttl.min(configured),
        None      => configured,
    }.clamp(MIN_INTERVAL, MAX_INTERVAL)
}

fn tick(last_script: &mut String, config_path: &str) -> Result<(bool, Option<u64>), Box<dyn std::error::Error>> {
    let cfg = config::load(config_path)?;

    if cfg.forward.is_empty() && cfg.block.is_empty() {
        log::warn!("配置中没有任何规则");
    }

    let mut min_ttl: Option<u64> = None;
    let mut forwards: Vec<ResolvedForward> = Vec::new();

    'rule: for rule in cfg.forward {
        let listen = match config::Listen::parse(&rule.listen) {
            Ok(l) => l,
            Err(e) => { log::warn!("跳过规则 [{}]: {}", rule.listen, e); continue; }
        };

        let mut resolved_targets: Vec<ResolvedTarget> = Vec::new();

        for to_str in &rule.to {
            let target = match config::Target::parse(to_str) {
                Ok(t) => t,
                Err(e) => { log::warn!("跳过目标 {}: {}", to_str, e); continue; }
            };

            // DNS 解析（含 TTL）
            let (ip, ttl) = match ip::resolve_with_ttl(&target.host, rule.ipv6) {
                Ok(r) => r,
                Err(e) => { log::warn!("跳过目标 {}: {}", to_str, e); continue; }
            };

            let ttl_display = if ttl == u32::MAX { "∞".to_string() } else { format!("{}s", ttl) };
            log::debug!("{} → {} (TTL {})", target.host, ip, ttl_display);

            // 更新全局最小 TTL（静态 IP 的 u32::MAX 不计入）
            if ttl != u32::MAX {
                let t = ttl as u64;
                min_ttl = Some(min_ttl.map_or(t, |m| m.min(t)));
            }

            // TCP 健康检查（UDP-only 规则跳过）
            if !matches!(rule.proto, config::Proto::Udp) {
                let addr_str = if ip.contains(':') {
                    format!("[{}]:{}", ip, target.port_start)
                } else {
                    format!("{}:{}", ip, target.port_start)
                };
                if let Ok(addr) = addr_str.parse() {
                    if TcpStream::connect_timeout(&addr, HEALTH_TIMEOUT).is_err() {
                        let label = rule.comment.as_deref().unwrap_or(to_str);
                        log::warn!("健康检查失败，暂时跳过目标「{}」({})", label, addr_str);
                        continue;
                    }
                }
            }

            resolved_targets.push(ResolvedTarget { target, ip });
        }

        if resolved_targets.is_empty() {
            let label = rule.comment.as_deref().unwrap_or(&rule.listen);
            log::warn!("规则「{}」所有目标均不可用，跳过", label);
            continue 'rule;
        }

        forwards.push(ResolvedForward { rule, listen, targets: resolved_targets });
    }

    let script = nft::build_script(&forwards, &cfg.block);
    if script == *last_script {
        return Ok((false, min_ttl));
    }

    nft::apply(&forwards, &cfg.block)?;
    *last_script = script;
    Ok((true, min_ttl))
}

fn enable_ip_forwarding() {
    for path in ["/proc/sys/net/ipv4/ip_forward", "/proc/sys/net/ipv6/conf/all/forwarding"] {
        match std::fs::write(path, "1") {
            Ok(_)  => log::debug!("已启用 {}", path),
            Err(e) => log::warn!("无法写入 {}: {}（非 root 运行？）", path, e),
        }
    }
}
