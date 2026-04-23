use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fs;

// ── 枚举 ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Proto {
    Tcp,
    Udp,
    #[default]
    All,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Chain {
    #[default]
    Input,
    Forward,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Balance {
    #[default]
    RoundRobin,
    Random,
}

// ── 配置结构 ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ForwardRule {
    /// 本机监听端口，单端口 "10000" 或端口段 "10000-10100"
    pub listen: String,
    /// 目标地址，单个 "host:port" 或多个 ["host1:port", "host2:port"]
    #[serde(
        deserialize_with = "deser_one_or_many",
        serialize_with = "ser_one_or_many"
    )]
    pub to: Vec<String>,
    #[serde(default)]
    pub proto: Proto,
    /// 强制使用 IPv6 解析（默认 false，自动优先 IPv4）
    #[serde(default)]
    pub ipv6: bool,
    /// 多目标负载均衡策略（默认 round-robin）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub balance: Option<Balance>,
    /// 带宽限速，单位 Mbps，如 200 表示 200 Mbps
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<u32>,
    pub comment: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BlockRule {
    /// 源 IP 或 CIDR，如 "1.2.3.4" 或 "10.0.0.0/8"
    pub src: Option<String>,
    /// 目标 IP 或 CIDR
    pub dst: Option<String>,
    /// 目标端口
    pub port: Option<u16>,
    #[serde(default)]
    pub proto: Proto,
    /// 作用链：input（入站）或 forward（转发），默认 input
    #[serde(default)]
    pub chain: Chain,
    /// 是否匹配 IPv6（默认 false）
    #[serde(default)]
    pub ipv6: bool,
    pub comment: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ForwardMode {
    /// nftables DNAT，内核直转（默认）
    #[default]
    Nat,
    /// tokio 异步代理 + splice 零拷贝
    Relay,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct Config {
    /// 转发模式（默认 nat，nat 时不写入配置文件）
    #[serde(default, skip_serializing_if = "ForwardMode::is_nat")]
    pub mode: ForwardMode,
    #[serde(default)]
    pub forward: Vec<ForwardRule>,
    #[serde(default)]
    pub block: Vec<BlockRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel: Option<PanelConfig>,
}

// ── 面板配置 ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum PanelMode {
    #[default]
    Node,
    Master,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct NodeEntry {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DiscourseConfig {
    /// Discourse 站点 URL，如 "https://forum.example.com"
    pub url: String,
    /// 与 Discourse 后台 "SSO Provider" 共享的 secret
    pub secret: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PanelConfig {
    #[serde(default)]
    pub mode: PanelMode,
    /// 监听地址，如 "0.0.0.0:9090"
    #[serde(default = "default_panel_listen")]
    pub listen: String,
    /// master 模式：面板 Web 登录的 JWT 签名密钥（HMAC）
    #[serde(default)]
    pub secret: String,
    /// master 模式：Ed25519 私钥 PEM（PKCS8），用于签署对 node 的 API 调用
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
    /// node 模式：主控的 Ed25519 公钥 PEM，用于验证主控请求
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub master_pubkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls_cert: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls_key: Option<String>,
    /// Discourse Connect（SSO Provider）配置，master 模式下的唯一登录方式
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discourse: Option<DiscourseConfig>,
    /// master 管理的 node 列表（有 database_url 时由数据库管理，此字段忽略）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<NodeEntry>,
    /// PostgreSQL 连接字符串（master 模式必填）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database_url: Option<String>,
}

fn default_panel_listen() -> String { "0.0.0.0:9090".to_string() }

impl ForwardMode {
    fn is_nat(&self) -> bool { *self == ForwardMode::Nat }
}

// ── 解析后的端口信息 ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Listen {
    Single(u16),
    Range(u16, u16),
}

#[derive(Debug, Clone)]
pub struct Target {
    pub host: String,
    pub port_start: u16,
}

impl Listen {
    pub fn parse(s: &str) -> Result<Self, String> {
        if let Some((a, b)) = s.split_once('-') {
            let start = a.trim().parse::<u16>().map_err(|_| format!("无效端口: {}", a))?;
            let end   = b.trim().parse::<u16>().map_err(|_| format!("无效端口: {}", b))?;
            if start >= end {
                return Err(format!("端口段无效: {} >= {}", start, end));
            }
            Ok(Listen::Range(start, end))
        } else {
            let port = s.trim().parse::<u16>().map_err(|_| format!("无效端口: {}", s))?;
            Ok(Listen::Single(port))
        }
    }

    pub fn size(&self) -> u16 {
        match self {
            Listen::Single(_) => 1,
            Listen::Range(s, e) => e - s,
        }
    }
}

impl Target {
    pub fn parse(s: &str) -> Result<Self, String> {
        // 从右边分割最后一个 ':'，支持 IPv6 地址
        let (host, port_str) = s.rsplit_once(':')
            .ok_or_else(|| format!("目标格式应为 host:port，实际为: {}", s))?;
        let port_start = port_str.trim().parse::<u16>()
            .map_err(|_| format!("无效端口: {}", port_str))?;
        Ok(Target { host: host.to_string(), port_start })
    }
}

// ── 自定义 serde：单字符串或字符串数组 ───────────────────────────

fn deser_one_or_many<'de, D>(d: D) -> Result<Vec<String>, D::Error>
where D: Deserializer<'de>
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany { One(String), Many(Vec<String>) }
    match OneOrMany::deserialize(d)? {
        OneOrMany::One(s)  => Ok(vec![s]),
        OneOrMany::Many(v) => Ok(v),
    }
}

fn ser_one_or_many<S>(v: &[String], s: S) -> Result<S::Ok, S::Error>
where S: Serializer
{
    if v.len() == 1 {
        s.serialize_str(&v[0])
    } else {
        v.serialize(s)
    }
}

// ── 加载 ──────────────────────────────────────────────────────────

pub fn load(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("读取配置文件 {} 失败: {}", path, e))?;
    let config: Config = toml::from_str(&content)
        .map_err(|e| format!("解析配置文件失败: {}", e))?;
    Ok(config)
}

pub fn save(config: &Config, path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let content = toml::to_string_pretty(config)
        .map_err(|e| format!("序列化配置失败: {}", e))?;
    fs::write(path, content)
        .map_err(|e| format!("写入配置文件 {} 失败: {}", path, e))?;
    Ok(())
}
