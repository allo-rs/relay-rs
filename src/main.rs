mod config;
mod ctl;
mod dns_cache;
mod ip;
mod nft;
mod proxy;
mod relay_state;

use clap::{Parser, Subcommand};
use dialoguer::{Input, Select, theme::ColorfulTheme};
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
    /// 交互式编辑已有规则
    Edit,
    /// 检查转发规则连通性
    Check,
    /// 直接探测指定地址端口（如 1.2.3.4:443 或 example.com:80）
    Ping {
        /// 目标地址，格式 host:port
        target: String,
    },
    /// 切换转发模式（nat / relay）
    Mode,
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
        None => run_menu(&cli.config),
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
        Command::Mode    => {
            let cfg = config::load(config).unwrap_or_default();
            match ctl::mode_cmd(cfg, config) {
                Ok(true) => {
                    if ctl::confirm("立即重启服务使变更生效？[Y/n]", true) {
                        systemctl("restart");
                    }
                }
                Ok(false) => {}
                Err(e) => { eprintln!("{}", e); std::process::exit(1); }
            }
        }
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
        Command::Ping { target } => ctl::ping(&target),
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
        Command::Edit => {
            let mut cfg = match config::load(config) {
                Ok(c) => c,
                Err(e) => { eprintln!("{}", e); std::process::exit(1); }
            };
            match ctl::edit(&mut cfg) {
                Ok(true) => match config::save(&cfg, config) {
                    Ok(_) => {
                        println!();
                        ctl::list(&cfg);
                        println!();
                        if ctl::confirm("立即重启服务使变更生效？[Y/n]", true) {
                            systemctl("restart");
                        }
                    }
                    Err(e) => { eprintln!("保存失败: {}", e); std::process::exit(1); }
                },
                Ok(false) => {}
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

// ── 主菜单 ───────────────────────────────────────────────────────

fn run_menu(config: &str) {
    let theme = ColorfulTheme::default();
    let choices = [
        "添加规则",
        "编辑规则",
        "删除规则",
        "检查连通性",
        "Ping 端口",
        "流量统计",
        "切换模式",
        "退出",
    ];

    loop {
        // 清屏
        print!("\x1b[2J\x1b[H");
        use std::io::Write;
        std::io::stdout().flush().ok();

        println!("relay-rs — 规则管理\n");
        match config::load(config) {
            Ok(ref cfg) => ctl::list(cfg),
            Err(_) => println!("（暂无规则）"),
        }
        println!();

        let sel = match Select::with_theme(&theme)
            .with_prompt("操作（↑↓ 选择，回车确认，Esc 退出）")
            .items(&choices)
            .default(0)
            .interact_opt()
        {
            Ok(Some(s)) => s,
            _ => break,
        };

        println!();

        match sel {
            0 => { // 添加规则
                let mut cfg = config::load(config).unwrap_or_default();
                match ctl::add(&mut cfg) {
                    Ok(_) => match config::save(&cfg, config) {
                        Ok(_) => {
                            println!("\n规则已添加。");
                            if ctl::confirm("立即重启服务？[Y/n]", true) {
                                systemctl_quiet("restart");
                            }
                        }
                        Err(e) => { eprintln!("保存失败: {}", e); pause(); }
                    },
                    Err(e) => { eprintln!("错误: {}", e); pause(); }
                }
            }
            1 => { // 编辑规则
                let mut cfg = match config::load(config) {
                    Ok(c) => c,
                    Err(e) => { eprintln!("{}", e); pause(); continue; }
                };
                match ctl::edit(&mut cfg) {
                    Ok(true) => match config::save(&cfg, config) {
                        Ok(_) => {
                            println!();
                            ctl::list(&cfg);
                            println!();
                            if ctl::confirm("立即重启服务？[Y/n]", true) {
                                systemctl_quiet("restart");
                            }
                        }
                        Err(e) => { eprintln!("保存失败: {}", e); pause(); }
                    },
                    Ok(false) => {}
                    Err(e) => { eprintln!("错误: {}", e); pause(); }
                }
            }
            2 => { // 删除规则
                let mut cfg = match config::load(config) {
                    Ok(c) => c,
                    Err(e) => { eprintln!("{}", e); pause(); continue; }
                };
                match ctl::del(&mut cfg) {
                    Ok(true) => match config::save(&cfg, config) {
                        Ok(_) => {
                            if ctl::confirm("立即重启服务？[Y/n]", true) {
                                systemctl_quiet("restart");
                            }
                        }
                        Err(e) => { eprintln!("保存失败: {}", e); pause(); }
                    },
                    Ok(false) => {}
                    Err(e) => { eprintln!("错误: {}", e); pause(); }
                }
            }
            3 => { // 检查连通性
                match config::load(config) {
                    Ok(cfg) => { let _ = ctl::check(&cfg); }
                    Err(e) => eprintln!("{}", e),
                }
                pause();
            }
            4 => { // Ping 端口
                if let Ok(target) = Input::<String>::with_theme(&theme)
                    .with_prompt("目标地址（host:port）")
                    .interact_text()
                {
                    ctl::ping(&target);
                    pause();
                }
            }
            5 => { // 流量统计
                ctl::stats();
                pause();
            }
            6 => { // 切换模式
                let cfg = match config::load(config) {
                    Ok(c) => c,
                    Err(e) => { eprintln!("{}", e); pause(); continue; }
                };
                match ctl::mode_cmd(cfg, config) {
                    Ok(true) => {
                        if ctl::confirm("立即重启服务？[Y/n]", true) {
                            systemctl_quiet("restart");
                        }
                    }
                    Ok(false) => {}
                    Err(e) => { eprintln!("{}", e); pause(); }
                }
            }
            _ => break, // 退出
        }
    }
}

fn pause() {
    use std::io::{Read, Write};
    print!("\n按回车返回菜单...");
    std::io::stdout().flush().ok();
    let _ = std::io::stdin().read(&mut [0u8]);
}

fn systemctl_quiet(action: &str) {
    let _ = std::process::Command::new("systemctl")
        .args([action, SERVICE])
        .status();
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

    let mode = config::load(config_path).map(|c| c.mode).unwrap_or_default();
    log::info!("relay-rs 启动，模式: {}", match mode {
        config::ForwardMode::Nat   => "nat (nftables)",
        config::ForwardMode::Relay => "relay (tokio + splice)",
    });

    // 注册 SIGHUP 用于热重载，两种模式共用
    let reload = Arc::new(AtomicBool::new(false));
    let reload_tx = Arc::clone(&reload);
    match signal_hook::iterator::Signals::new([signal_hook::consts::SIGHUP]) {
        Ok(mut signals) => {
            std::thread::spawn(move || {
                for _ in signals.forever() { reload_tx.store(true, Ordering::Relaxed); }
            });
            log::info!("已注册 SIGHUP 热重载");
        }
        Err(e) => log::warn!("无法注册 SIGHUP: {}，热重载不可用", e),
    }

    match mode {
        config::ForwardMode::Nat => {
            enable_ip_forwarding();
            log::info!("配置: {}，最大轮询间隔: {}s", config_path, interval);
            run_nat_daemon(config_path, interval, reload);
        }
        config::ForwardMode::Relay => {
            // 清理可能残留的 nftables 规则（从内核模式切换过来时）
            nft::clear_tables();
            run_relay_daemon(config_path, reload);
        }
    }
}

fn run_nat_daemon(config_path: &str, interval: u64, reload: Arc<AtomicBool>) {
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

        for _ in 0..next_sleep {
            if reload.load(Ordering::Relaxed) { break; }
            sleep(Duration::from_secs(1));
        }
    }
}

fn run_relay_daemon(config_path: &str, reload: Arc<AtomicBool>) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("无法创建 proxy runtime");
    rt.block_on(proxy::run(config_path, reload));
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
