use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Tcp,
    Udp,
    All,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum IpVersion {
    Ipv4,
    Ipv6,
    All,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Rule {
    /// 本机监听端口
    pub sport: u16,
    /// 目标端口
    pub dport: u16,
    /// 目标域名或 IP
    pub target: String,
    #[serde(default = "default_protocol")]
    pub protocol: Protocol,
    #[serde(default = "default_ip_version")]
    pub ip_version: IpVersion,
    pub comment: Option<String>,
}

fn default_protocol() -> Protocol {
    Protocol::All
}

fn default_ip_version() -> IpVersion {
    IpVersion::Ipv4
}

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub rules: Vec<Rule>,
}

pub fn load(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("读取配置文件 {} 失败: {}", path, e))?;
    let config: Config = toml::from_str(&content)
        .map_err(|e| format!("解析配置文件失败: {}", e))?;
    Ok(config)
}
