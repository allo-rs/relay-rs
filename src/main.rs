mod config;
mod ctl;
mod ip;
mod nft;

use clap::{CommandFactory, Parser, Subcommand};
use nft::ResolvedForward;
use std::thread::sleep;
use std::time::Duration;

const SERVICE: &str = "relay-rs";
const CONFIG_PATH: &str = "/etc/relay-rs/relay.toml";

#[derive(Parser)]
#[command(name = "relay-rs", about = "基于 nftables 的 NAT 端口转发守护进程", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// 配置文件路径
    #[arg(short, long, default_value = CONFIG_PATH, global = true)]
    config: String,

    /// 轮询间隔秒数
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
    log::info!("relay-rs 启动，配置: {}，轮询: {}s", config_path, interval);

    let mut last_script = String::new();
    loop {
        match tick(&mut last_script, config_path) {
            Ok(true)  => log::info!("规则已更新并应用"),
            Ok(false) => log::debug!("规则无变化，跳过"),
            Err(e)    => log::error!("{}", e),
        }
        sleep(Duration::from_secs(interval));
    }
}

fn tick(last_script: &mut String, config_path: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let cfg = config::load(config_path)?;

    if cfg.forward.is_empty() && cfg.block.is_empty() {
        log::warn!("配置中没有任何规则");
    }

    // 解析转发规则的 DNS
    let forwards: Vec<ResolvedForward> = cfg.forward.into_iter().filter_map(|rule| {
        let listen = match config::Listen::parse(&rule.listen) {
            Ok(l) => l,
            Err(e) => { log::warn!("跳过转发规则: {}", e); return None; }
        };
        let target = match config::Target::parse(&rule.to) {
            Ok(t) => t,
            Err(e) => { log::warn!("跳过转发规则: {}", e); return None; }
        };
        match ip::resolve(&target.host, rule.ipv6) {
            Ok(ip) => {
                log::debug!("{} → {}", target.host, ip);
                Some(ResolvedForward { rule, listen, target, ip })
            }
            Err(e) => { log::warn!("跳过转发规则 ({}): {}", target.host, e); None }
        }
    }).collect();

    let script = nft::build_script(&forwards, &cfg.block);
    if script == *last_script {
        return Ok(false);
    }

    nft::apply(&forwards, &cfg.block)?;
    *last_script = script;
    Ok(true)
}

fn enable_ip_forwarding() {
    for path in ["/proc/sys/net/ipv4/ip_forward", "/proc/sys/net/ipv6/conf/all/forwarding"] {
        match std::fs::write(path, "1") {
            Ok(_)  => log::debug!("已启用 {}", path),
            Err(e) => log::warn!("无法写入 {}: {}（非 root 运行？）", path, e),
        }
    }
}
