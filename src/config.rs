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

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Chain {
    Input,
    Forward,
}

/// 一条规则，通过 `type` 字段区分类型
#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Rule {
    /// 单端口转发：本机 sport → target:dport
    Single {
        sport: u16,
        dport: u16,
        target: String,
        #[serde(default = "default_protocol")]
        protocol: Protocol,
        #[serde(default = "default_ip_version")]
        ip_version: IpVersion,
        comment: Option<String>,
    },
    /// 端口段转发：本机 sport_start-sport_end → target:dport_start-dport_end
    Range {
        sport_start: u16,
        sport_end: u16,
        /// 目标起始端口，省略则与 sport_start 相同
        #[serde(default)]
        dport_start: Option<u16>,
        target: String,
        #[serde(default = "default_protocol")]
        protocol: Protocol,
        #[serde(default = "default_ip_version")]
        ip_version: IpVersion,
        comment: Option<String>,
    },
    /// 防火墙丢包：按 IP / 端口 / 协议匹配后 drop
    Drop {
        /// 作用链：input（入站）或 forward（转发）
        #[serde(default = "default_chain")]
        chain: Chain,
        /// 源 IP 或 CIDR，如 "1.2.3.4" 或 "192.168.0.0/24"
        src_ip: Option<String>,
        /// 目标 IP 或 CIDR
        dst_ip: Option<String>,
        /// 源端口
        src_port: Option<u16>,
        /// 目标端口
        dst_port: Option<u16>,
        #[serde(default = "default_protocol")]
        protocol: Protocol,
        #[serde(default = "default_ip_version")]
        ip_version: IpVersion,
        comment: Option<String>,
    },
}

impl Rule {
    /// 返回转发规则的目标地址（Drop 规则无目标）
    pub fn target(&self) -> Option<&str> {
        match self {
            Rule::Single { target, .. } => Some(target),
            Rule::Range { target, .. } => Some(target),
            Rule::Drop { .. } => None,
        }
    }

    pub fn ip_version(&self) -> &IpVersion {
        match self {
            Rule::Single { ip_version, .. } => ip_version,
            Rule::Range { ip_version, .. } => ip_version,
            Rule::Drop { ip_version, .. } => ip_version,
        }
    }
}

fn default_protocol() -> Protocol { Protocol::All }
fn default_ip_version() -> IpVersion { IpVersion::Ipv4 }
fn default_chain() -> Chain { Chain::Input }

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
