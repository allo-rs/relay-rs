use serde::Deserialize;
use std::fs;

// ── 枚举 ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Proto {
    Tcp,
    Udp,
    #[default]
    All,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Chain {
    #[default]
    Input,
    Forward,
}

// ── 配置结构 ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
pub struct ForwardRule {
    /// 本机监听端口，单端口 "10000" 或端口段 "10000-10100"
    pub listen: String,
    /// 目标地址，格式 "host:port"，如 "example.com:443"
    pub to: String,
    #[serde(default)]
    pub proto: Proto,
    /// 强制使用 IPv6 解析（默认 false，自动优先 IPv4）
    #[serde(default)]
    pub ipv6: bool,
    pub comment: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
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

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub forward: Vec<ForwardRule>,
    #[serde(default)]
    pub block: Vec<BlockRule>,
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

// ── 加载 ──────────────────────────────────────────────────────────

pub fn load(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("读取配置文件 {} 失败: {}", path, e))?;
    let config: Config = toml::from_str(&content)
        .map_err(|e| format!("解析配置文件失败: {}", e))?;
    Ok(config)
}
