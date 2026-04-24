mod config;
mod ctl;
mod db;
mod dns_cache;
mod ip;
mod nft;
mod panel;
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

fn service_name() -> &'static str {
    if std::env::var("DATABASE_URL").is_ok() { "relay-rs-master" } else { "relay-rs" }
}
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

    /// 主控模式：数据库连接串（也可用 DATABASE_URL 环境变量）
    #[arg(long, env = "DATABASE_URL", global = true)]
    db: Option<String>,

    /// 主控模式：面板监听地址（也可用 PANEL_LISTEN 环境变量）
    #[arg(short = 'p', long, env = "PANEL_LISTEN", default_value = "0.0.0.0:9090", global = true)]
    listen: String,
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
    /// 初始化面板：生成 Ed25519 主控密钥（master 模式首次运行）
    PanelInit,
    /// 清除 DB 中的 Discourse 登录配置（恢复开放模式，用于锁死救援）
    PanelResetAuth,
    /// 以守护进程模式运行（供 systemd 调用）
    #[command(hide = true)]
    Daemon,
}

fn main() {
    // 自动加载 /etc/relay-rs/env（DATABASE_URL 未设置时）
    if std::env::var("DATABASE_URL").is_err() {
        if let Ok(content) = std::fs::read_to_string("/etc/relay-rs/env") {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') { continue; }
                if let Some((k, v)) = line.split_once('=') {
                    unsafe { std::env::set_var(k.trim(), v.trim()); }
                }
            }
        }
    }

    // rustls 0.23 需要显式安装加密提供者，在任何 TLS 操作之前调用
    rustls::crypto::ring::default_provider().install_default().ok();

    let cli = Cli::parse();
    match cli.command {
        Some(cmd) => run_ctl(cmd, &cli.config, cli.db.as_deref(), &cli.listen, cli.interval),
        None => {
            if cli.db.is_some() {
                println!("主控模式请使用子命令，如 rr daemon 或 rr list");
            } else {
                run_menu(&cli.config);
            }
        }
    }
}

// ── 管理子命令 ────────────────────────────────────────────────────

fn run_ctl(cmd: Command, config: &str, db_url: Option<&str>, listen: &str, interval: u64) {
    if let Some(db) = db_url {
        run_ctl_master(cmd, db, listen, interval);
    } else {
        run_ctl_node(cmd, config, interval);
    }
}

fn run_ctl_node(cmd: Command, config: &str, interval: u64) {
    match cmd {
        Command::Start   => systemctl("start"),
        Command::Stop    => systemctl("stop"),
        Command::Restart => systemctl("restart"),
        Command::Status  => systemctl("status"),
        Command::Log     => journalctl(),
        Command::Config  => edit_config(config),
        Command::Reload  => { edit_config(config); systemctl("restart"); }
        Command::Stats   => {
            let is_relay = config::load(config)
                .map(|c| c.mode == config::ForwardMode::Relay)
                .unwrap_or(false);
            ctl::stats(is_relay);
        }
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
                    if let Err(e) = ctl::check(&cfg.forward) {
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
                Ok(cfg) => ctl::list(&cfg.forward, &cfg.block),
                Err(e)  => { eprintln!("{}", e); std::process::exit(1); }
            }
        }
        Command::Add => {
            let mut cfg = config::load(config).unwrap_or_default();
            match ctl::add(&mut cfg) {
                Ok(_) => match config::save(&cfg, config) {
                    Ok(_) => {
                        ctl::list(&cfg.forward, &cfg.block);
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
                        ctl::list(&cfg.forward, &cfg.block);
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
                        ctl::list(&cfg.forward, &cfg.block);
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
        Command::PanelInit => panel_init(config),
        Command::PanelResetAuth => panel_reset_auth(config),
    }
}

// ── 主控模式子命令 ────────────────────────────────────────────────

fn run_ctl_master(cmd: Command, db_url: &str, listen: &str, interval: u64) {
    match cmd {
        Command::List => master_list(db_url),
        Command::Add  => master_add(db_url),
        Command::Del  => master_del(db_url),
        Command::Edit => master_edit(db_url),
        Command::Mode => master_mode(db_url),
        Command::Stats => {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            let pool = rt.block_on(db::connect(db_url)).expect("DB连接失败");
            let is_relay = rt.block_on(db::get_forward_mode(&pool))
                .map(|m| m == config::ForwardMode::Relay)
                .unwrap_or(false);
            ctl::stats(is_relay);
        }
        Command::PanelInit => panel_init_db(db_url),
        Command::PanelResetAuth => panel_reset_auth_db(db_url),
        Command::Daemon => run_master_daemon(db_url, listen, interval),
        Command::Check => {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            let pool = rt.block_on(db::connect(db_url)).expect("DB连接失败");
            let rows = rt.block_on(db::list_forward_rules(&pool)).unwrap_or_default();
            let forward: Vec<_> = rows.into_iter().map(|r| r.rule).collect();
            let _ = ctl::check(&forward);
        }
        Command::Ping { target } => ctl::ping(&target),
        _ => println!("主控模式不支持此命令，请通过 Web 面板操作"),
    }
}

fn master_list(db_url: &str) {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let pool = db::connect(db_url).await.expect("DB连接失败");
        let forward_rows = db::list_forward_rules(&pool).await.unwrap_or_default();
        let block_rows = db::list_block_rules(&pool).await.unwrap_or_default();
        let forward: Vec<_> = forward_rows.into_iter().map(|r| r.rule).collect();
        let block: Vec<_> = block_rows.into_iter().map(|r| r.rule).collect();
        ctl::list(&forward, &block);
    });
}

fn master_add(db_url: &str) {
    let theme = dialoguer::theme::ColorfulTheme::default();
    let kind = dialoguer::Select::with_theme(&theme)
        .with_prompt("规则类型")
        .items(&["转发规则 (forward)", "防火墙规则 (block)"])
        .default(0)
        .interact()
        .unwrap_or(0);

    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let pool = db::connect(db_url).await.expect("DB连接失败");
        match kind {
            0 => match ctl::build_forward_rule(&theme) {
                Ok(rule) => {
                    db::add_forward_rule(&pool, &rule).await.expect("写入转发规则失败");
                    println!("已添加转发规则：{} → {}", rule.listen, rule.to.join(", "));
                }
                Err(e) => { eprintln!("错误: {}", e); }
            },
            _ => match ctl::build_block_rule(&theme) {
                Ok(rule) => {
                    db::add_block_rule(&pool, &rule).await.expect("写入防火墙规则失败");
                    println!("已添加防火墙规则");
                }
                Err(e) => { eprintln!("错误: {}", e); }
            },
        }
    });
    if ctl::confirm("立即重启服务使规则生效？[Y/n]", true) {
        systemctl_quiet("restart");
    }
}

fn master_del(db_url: &str) {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let pool = db::connect(db_url).await.expect("DB连接失败");
        let forward_rows = db::list_forward_rules(&pool).await.unwrap_or_default();
        let block_rows = db::list_block_rules(&pool).await.unwrap_or_default();

        if forward_rows.is_empty() && block_rows.is_empty() {
            println!("（暂无规则）");
            return;
        }

        let mut items: Vec<String> = Vec::new();
        for r in &forward_rows {
            let to_display = r.rule.to.join(", ");
            let cmt = r.rule.comment.as_deref().map(|s| format!(" # {}", s)).unwrap_or_default();
            items.push(format!("[转发] id={} {}  →  {}{}", r.id, r.rule.listen, to_display, cmt));
        }
        for b in &block_rows {
            let mut parts = Vec::new();
            if let Some(s) = &b.rule.src { parts.push(format!("src={}", s)); }
            if let Some(d) = &b.rule.dst { parts.push(format!("dst={}", d)); }
            if let Some(p) = b.rule.port { parts.push(format!("port={}", p)); }
            let cmt = b.rule.comment.as_deref().map(|s| format!(" # {}", s)).unwrap_or_default();
            items.push(format!("[防火墙] id={} {}{}", b.id, parts.join(" "), cmt));
        }
        items.push("取消".to_string());

        let theme = dialoguer::theme::ColorfulTheme::default();
        let total = forward_rows.len() + block_rows.len();
        let selection = dialoguer::Select::with_theme(&theme)
            .with_prompt("选择要删除的规则（↑↓ 选择，回车确认）")
            .items(&items)
            .default(0)
            .interact()
            .unwrap_or(total);

        if selection == total {
            println!("已取消");
            return;
        }

        let confirmed = dialoguer::Confirm::with_theme(&theme)
            .with_prompt(format!("确认删除 {}？", &items[selection]))
            .default(false)
            .interact()
            .unwrap_or(false);

        if !confirmed {
            println!("已取消");
            return;
        }

        if selection < forward_rows.len() {
            let id = forward_rows[selection].id;
            db::delete_forward_rule(&pool, id).await.expect("删除失败");
            println!("已删除转发规则 id={}", id);
        } else {
            let bi = selection - forward_rows.len();
            let id = block_rows[bi].id;
            db::delete_block_rule(&pool, id).await.expect("删除失败");
            println!("已删除防火墙规则 id={}", id);
        }
    });
    if ctl::confirm("立即重启服务使变更生效？[Y/n]", true) {
        systemctl_quiet("restart");
    }
}

fn master_edit(db_url: &str) {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let pool = db::connect(db_url).await.expect("DB连接失败");
        let forward_rows = db::list_forward_rules(&pool).await.unwrap_or_default();
        let block_rows = db::list_block_rules(&pool).await.unwrap_or_default();

        if forward_rows.is_empty() && block_rows.is_empty() {
            println!("（暂无规则）");
            return;
        }

        let mut items: Vec<String> = Vec::new();
        for r in &forward_rows {
            let to_display = r.rule.to.join(", ");
            let cmt = r.rule.comment.as_deref().map(|s| format!(" # {}", s)).unwrap_or_default();
            items.push(format!("[转发] id={} {}  →  {}{}", r.id, r.rule.listen, to_display, cmt));
        }
        for b in &block_rows {
            let mut parts = Vec::new();
            if let Some(s) = &b.rule.src { parts.push(format!("src={}", s)); }
            if let Some(d) = &b.rule.dst { parts.push(format!("dst={}", d)); }
            if let Some(p) = b.rule.port { parts.push(format!("port={}", p)); }
            let cmt = b.rule.comment.as_deref().map(|s| format!(" # {}", s)).unwrap_or_default();
            items.push(format!("[防火墙] id={} {}{}", b.id, parts.join(" "), cmt));
        }
        items.push("取消".to_string());

        let theme = dialoguer::theme::ColorfulTheme::default();
        let total = forward_rows.len() + block_rows.len();
        let selection = dialoguer::Select::with_theme(&theme)
            .with_prompt("选择要编辑的规则（↑↓ 选择，回车确认）")
            .items(&items)
            .default(0)
            .interact()
            .unwrap_or(total);

        if selection == total {
            println!("已取消");
            return;
        }

        if selection < forward_rows.len() {
            let row = &forward_rows[selection];
            match ctl::edit_forward(row.rule.clone(), &theme) {
                Ok(new_rule) => {
                    db::update_forward_rule(&pool, row.id, &new_rule).await.expect("更新失败");
                    println!("已更新转发规则 id={}", row.id);
                }
                Err(e) => eprintln!("错误: {}", e),
            }
        } else {
            let bi = selection - forward_rows.len();
            let row = &block_rows[bi];
            match ctl::edit_block(row.rule.clone(), &theme) {
                Ok(new_rule) => {
                    db::update_block_rule(&pool, row.id, &new_rule).await.expect("更新失败");
                    println!("已更新防火墙规则 id={}", row.id);
                }
                Err(e) => eprintln!("错误: {}", e),
            }
        }
    });
    if ctl::confirm("立即重启服务使变更生效？[Y/n]", true) {
        systemctl_quiet("restart");
    }
}

fn master_mode(db_url: &str) {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let pool = db::connect(db_url).await.expect("DB连接失败");
        let current = db::get_forward_mode(&pool).await.unwrap_or_default();
        let current_idx = match current { config::ForwardMode::Nat => 0, config::ForwardMode::Relay => 1 };

        let theme = dialoguer::theme::ColorfulTheme::default();
        let idx = dialoguer::Select::with_theme(&theme)
            .with_prompt(format!(
                "转发模式（当前: {}）",
                if current_idx == 0 { "nat" } else { "relay" }
            ))
            .items(&[
                "nat    — nftables DNAT，内核直转，性能最优（推荐）",
                "relay  — tokio + splice 零拷贝，无需 root，支持复杂场景",
            ])
            .default(current_idx)
            .interact()
            .unwrap_or(current_idx);

        let new_mode = if idx == 0 { config::ForwardMode::Nat } else { config::ForwardMode::Relay };
        if new_mode == current {
            println!("模式未变更");
            return;
        }
        db::set_forward_mode(&pool, &new_mode).await.expect("写入模式失败");
        println!("已切换 → {}", if idx == 0 { "nat" } else { "relay" });
    });
    if ctl::confirm("立即重启服务使变更生效？[Y/n]", true) {
        systemctl_quiet("restart");
    }
}

/// 初始化面板：生成 Ed25519 主控密钥对并保存到配置（登录走 Discourse Connect）
fn panel_init(config_path: &str) {
    use base64::Engine as _;

    let mut cfg = config::load(config_path).unwrap_or_default();

    let panel = cfg.panel.get_or_insert_with(|| config::PanelConfig {
        mode: config::PanelMode::Master,
        listen: "0.0.0.0:9090".to_string(),
        secret: String::new(),
        private_key: None,
        master_pubkey: None,
        tls_cert: None,
        tls_key: None,
        nodes: Vec::new(),
        database_url: None,
    });

    if panel.private_key.is_some() {
        println!("已存在 Ed25519 主控密钥，跳过生成。");
    } else {
        println!("生成 Ed25519 主控密钥对...");
        let key_pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_ED25519)
            .unwrap_or_else(|e| { eprintln!("生成密钥对失败: {}", e); std::process::exit(1); });
        let priv_pem = key_pair.serialize_pem();
        let pub_der = key_pair.public_key_der();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&pub_der);
        let lines: Vec<&str> = b64.as_bytes().chunks(64)
            .map(|c| std::str::from_utf8(c).unwrap_or(""))
            .collect();
        let pub_pem = format!(
            "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
            lines.join("\n")
        );
        panel.private_key = Some(priv_pem);
        println!("\n节点配置中的 master_pubkey：\n{}", pub_pem);
    }

    if panel.secret.is_empty() {
        eprintln!("⚠️  panel.secret 为空，请手动填写一个随机字符串用于签发面板登录 JWT。");
    }
    println!("💡 Discourse 登录配置现在在面板 UI 的「设置」页中填写（首次访问 panel 无需登录）。");

    match config::save(&cfg, config_path) {
        Ok(_) => println!("面板配置已保存：{}", config_path),
        Err(e) => { eprintln!("保存失败: {}", e); std::process::exit(1); }
    }
}

/// 主控模式初始化：生成 secret 和 Ed25519 密钥对并写入 PostgreSQL
fn panel_init_db(db_url: &str) {
    use base64::Engine as _;
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let pool = db::connect(db_url).await.expect("DB连接失败");
        db::ensure_schema(&pool).await.expect("建表失败");

        // secret
        let secret = db::get_or_create_secret(&pool).await.expect("生成 secret 失败");
        println!("JWT secret 已就绪（{}...）", &secret[..8.min(secret.len())]);

        // Ed25519 key
        if db::get_private_key(&pool).await.unwrap_or(None).is_some() {
            println!("已存在 Ed25519 主控密钥，跳过生成。");
        } else {
            let key_pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_ED25519)
                .unwrap_or_else(|e| { eprintln!("生成密钥对失败: {}", e); std::process::exit(1); });
            let priv_pem = key_pair.serialize_pem();
            let pub_der = key_pair.public_key_der();
            let b64 = base64::engine::general_purpose::STANDARD.encode(&pub_der);
            let lines: Vec<&str> = b64.as_bytes().chunks(64)
                .map(|c| std::str::from_utf8(c).unwrap_or(""))
                .collect();
            let pub_pem = format!(
                "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
                lines.join("\n")
            );
            db::set_private_key(&pool, &priv_pem).await.expect("写入私钥失败");
            println!("\n节点配置中的 master_pubkey：\n{}", pub_pem);
        }
        println!("面板初始化完成（数据已存入 PostgreSQL）");
    });
}

/// 主控模式：清除 DB 中的 Discourse 登录配置，恢复开放访问模式
fn panel_reset_auth_db(db_url: &str) {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let pool = match db::connect(db_url).await {
            Ok(p) => p,
            Err(e) => { eprintln!("连接数据库失败: {}", e); std::process::exit(1); }
        };
        match db::delete_setting(&pool, "discourse").await {
            Ok(true)  => println!("已清除 Discourse 登录配置，panel 将回到开放模式（无需重启）。"),
            Ok(false) => println!("未配置 Discourse，无需清除。"),
            Err(e)    => { eprintln!("清除失败: {}", e); std::process::exit(1); }
        }
    });
}

/// 清除 DB 中的 Discourse 登录配置，恢复开放访问模式
fn panel_reset_auth(config_path: &str) {
    let cfg = match config::load(config_path) {
        Ok(c) => c,
        Err(e) => { eprintln!("读取配置失败: {}", e); std::process::exit(1); }
    };
    let panel = match cfg.panel {
        Some(p) => p,
        None => { eprintln!("配置中无 [panel] 段"); std::process::exit(1); }
    };
    let db_url = match &panel.database_url {
        Some(u) if !u.is_empty() => u.clone(),
        _ => { eprintln!("panel.database_url 为空"); std::process::exit(1); }
    };

    let rt = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => { eprintln!("创建 tokio runtime 失败: {}", e); std::process::exit(1); }
    };

    rt.block_on(async move {
        let pool = match db::connect(&db_url).await {
            Ok(p) => p,
            Err(e) => { eprintln!("连接数据库失败: {}", e); std::process::exit(1); }
        };
        match db::delete_setting(&pool, "discourse").await {
            Ok(true) => println!("✅ 已清除 Discourse 登录配置，panel 将回到开放模式（无需重启）。"),
            Ok(false) => println!("ℹ️  未配置 Discourse，无需清除。"),
            Err(e) => { eprintln!("清除失败: {}", e); std::process::exit(1); }
        }
    });
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
            Ok(ref cfg) => ctl::list(&cfg.forward, &cfg.block),
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
                            ctl::list(&cfg.forward, &cfg.block);
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
                    Ok(cfg) => { let _ = ctl::check(&cfg.forward); }
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
                let is_relay = config::load(config)
                    .map(|c| c.mode == config::ForwardMode::Relay)
                    .unwrap_or(false);
                ctl::stats(is_relay);
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
        .args([action, service_name()])
        .status();
}

fn systemctl(action: &str) {
    let status = std::process::Command::new("systemctl")
        .args([action, service_name()])
        .status()
        .unwrap_or_else(|e| { eprintln!("执行 systemctl 失败: {}", e); std::process::exit(1); });
    std::process::exit(status.code().unwrap_or(1));
}

fn journalctl() {
    let status = std::process::Command::new("journalctl")
        .args(["-u", service_name(), "-f"])
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
    // 若 nat 模式配置了面板，在独立线程中启动 tokio runtime 运行面板
    if let Ok(cfg) = config::load(config_path) {
        if let Some(pcfg) = cfg.panel {
            let cp = config_path.to_string();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("无法创建 panel runtime");
                rt.block_on(panel::run(pcfg, cp));
            });
        }
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

        for _ in 0..next_sleep {
            if reload.load(Ordering::Relaxed) { break; }
            sleep(Duration::from_secs(1));
        }
    }
}

fn run_relay_daemon(config_path: &str, reload: Arc<AtomicBool>) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("无法创建 proxy runtime");

    // 若配置了面板，则并发启动
    let panel_cfg = config::load(config_path).ok().and_then(|c| c.panel);
    let config_path_owned = config_path.to_string();

    rt.block_on(async move {
        if let Some(pcfg) = panel_cfg {
            tokio::spawn(panel::run(pcfg, config_path_owned.clone()));
        }
        proxy::run(&config_path_owned, reload).await;
    });
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

// ── 主控守护进程 ──────────────────────────────────────────────────

fn run_master_daemon(db_url: &str, listen: &str, interval: u64) {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("无法创建 runtime");

    let db_url = db_url.to_string();
    let listen = listen.to_string();

    rt.block_on(async move {
        let pool = db::connect(&db_url).await.expect("DB连接失败");
        db::ensure_schema(&pool).await.expect("建表失败");

        // 从 DB 构建 PanelConfig（首次启动时自动生成密钥）
        let secret = db::get_or_create_secret(&pool).await.expect("获取 secret 失败");
        let private_key = match db::get_private_key(&pool).await.unwrap_or(None) {
            Some(pem) => Some(pem),
            None => {
                log::info!("首次启动，生成 Ed25519 主控密钥...");
                match rcgen::KeyPair::generate_for(&rcgen::PKCS_ED25519) {
                    Ok(kp) => {
                        let pem = kp.serialize_pem();
                        if let Err(e) = db::set_private_key(&pool, &pem).await {
                            log::error!("存储主控私钥失败: {}", e);
                        }
                        Some(pem)
                    }
                    Err(e) => { log::error!("生成 Ed25519 密钥失败: {}", e); None }
                }
            }
        };
        let mode = db::get_forward_mode(&pool).await.unwrap_or_default();

        let panel_cfg = config::PanelConfig {
            mode: config::PanelMode::Master,
            listen: listen.clone(),
            secret,
            private_key,
            master_pubkey: None,
            tls_cert: None,
            tls_key: None,
            nodes: vec![],
            database_url: Some(db_url.clone()),
        };

        log::info!("relay-rs 主控启动，面板监听 {}，转发模式: {:?}", listen, mode);

        // 注册 SIGHUP
        let reload = Arc::new(AtomicBool::new(false));
        let reload_tx = Arc::clone(&reload);
        match signal_hook::iterator::Signals::new([signal_hook::consts::SIGHUP]) {
            Ok(mut signals) => {
                std::thread::spawn(move || {
                    for _ in signals.forever() { reload_tx.store(true, Ordering::Relaxed); }
                });
                log::info!("已注册 SIGHUP 热重载");
            }
            Err(e) => log::warn!("无法注册 SIGHUP: {}", e),
        }

        // 启动面板（异步任务）
        let db_url_panel = db_url.clone();
        tokio::spawn(panel::run(panel_cfg, db_url_panel));

        // 根据模式运行 NAT 或 relay daemon
        match mode {
            config::ForwardMode::Nat => {
                enable_ip_forwarding();
                run_master_nat_loop(&pool, interval, reload).await;
            }
            config::ForwardMode::Relay => {
                nft::clear_tables();
                // TODO: relay 模式下从 DB 读规则暂不支持，使用 TODO 占位
                log::warn!("主控 relay 模式暂未完整支持，将回退到 NAT 模式循环");
                run_master_nat_loop(&pool, interval, reload).await;
            }
        }
    });
}

async fn run_master_nat_loop(pool: &sqlx::PgPool, interval: u64, reload: Arc<AtomicBool>) {
    let mut last_script = String::new();
    let mut next_sleep = interval.clamp(MIN_INTERVAL, MAX_INTERVAL);
    loop {
        if reload.swap(false, Ordering::Relaxed) {
            log::info!("收到 SIGHUP，重新加载规则");
        }
        match tick_from_db(&mut last_script, pool).await {
            Ok((true, ttl))  => { log::info!("规则已更新并应用"); next_sleep = calc_interval(ttl, interval); }
            Ok((false, ttl)) => { log::debug!("规则无变化，跳过"); next_sleep = calc_interval(ttl, interval); }
            Err(e)           => log::error!("{}", e),
        }
        log::debug!("下次检查: {}s 后", next_sleep);

        for _ in 0..next_sleep {
            if reload.load(Ordering::Relaxed) { break; }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

async fn tick_from_db(
    last_script: &mut String,
    pool: &sqlx::PgPool,
) -> Result<(bool, Option<u64>), Box<dyn std::error::Error>> {
    let forward_rows = db::list_forward_rules(pool).await?;
    let block_rows = db::list_block_rules(pool).await?;

    let forward_rules: Vec<config::ForwardRule> = forward_rows.into_iter().map(|r| r.rule).collect();
    let block_rules: Vec<config::BlockRule> = block_rows.into_iter().map(|r| r.rule).collect();

    if forward_rules.is_empty() && block_rules.is_empty() {
        log::warn!("数据库中没有任何规则");
    }

    let mut min_ttl: Option<u64> = None;
    let mut forwards: Vec<ResolvedForward> = Vec::new();

    'rule: for rule in forward_rules {
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

    let script = nft::build_script(&forwards, &block_rules);
    if script == *last_script {
        return Ok((false, min_ttl));
    }

    nft::apply(&forwards, &block_rules)?;
    *last_script = script;
    Ok((true, min_ttl))
}
