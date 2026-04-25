//! master CA 管理：首次启动自动生成自签 CA，后续 Register/Renew 用它签发 node cert。

use anyhow::{Context, Result};
use rcgen::{
    CertificateParams, DistinguishedName, DnType, IsCa, KeyPair, KeyUsagePurpose,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use time::{Duration, OffsetDateTime};

/// CA 的默认有效期：10 年
const CA_VALIDITY_YEARS: i64 = 10;
/// node cert 默认有效期：365 天
const NODE_CERT_VALIDITY_DAYS: i64 = 365;

pub struct Ca {
    pub cert: rcgen::Certificate,
    pub cert_pem: String,
    pub key_pair: KeyPair,
    /// CA bundle 版本（后续轮换用）
    pub bundle_version: u32,
}

impl Ca {
    /// 从目录加载，若不存在则生成一份新 CA 并写入
    pub fn load_or_create(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir).with_context(|| format!("创建 CA 目录 {:?} 失败", dir))?;
        let cert_path = dir.join("ca.pem");
        let key_path = dir.join("ca.key");
        let ver_path = dir.join("ca.bundle_version");

        if cert_path.exists() && key_path.exists() {
            let cert_pem = fs::read_to_string(&cert_path)?;
            let key_pem = fs::read_to_string(&key_path)?;
            let key_pair = KeyPair::from_pem(&key_pem).context("加载 CA 私钥失败")?;
            // 从 PEM 重建 CertificateParams，再 self_signed 得到可用于签发的 Certificate
            let params = CertificateParams::from_ca_cert_pem(&cert_pem)
                .context("解析 CA cert 为 params")?;
            let cert = params.self_signed(&key_pair).context("重建 CA Certificate")?;
            let bundle_version = fs::read_to_string(&ver_path)
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(1u32);
            log::info!("加载现有 CA {:?}（bundle v{}）", cert_path, bundle_version);
            return Ok(Self { cert, cert_pem, key_pair, bundle_version });
        }

        log::warn!("CA 不存在，生成新的自签 CA 到 {:?}", dir);
        let key_pair = KeyPair::generate().context("生成 CA 密钥失败")?;
        let mut params = CertificateParams::new(vec![]).context("CertificateParams::new")?;
        params.is_ca = IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, "relay-master-ca");
        dn.push(DnType::OrganizationName, "relay-rs");
        params.distinguished_name = dn;
        params.not_before = OffsetDateTime::now_utc();
        params.not_after = OffsetDateTime::now_utc() + Duration::days(365 * CA_VALIDITY_YEARS);

        let cert = params.self_signed(&key_pair).context("自签 CA 失败")?;
        let cert_pem = cert.pem();
        let key_pem = key_pair.serialize_pem();

        write_secure(&cert_path, &cert_pem)?;
        write_secure(&key_path, &key_pem)?;
        fs::write(&ver_path, "1\n")?;

        Ok(Self {
            cert,
            cert_pem,
            key_pair,
            bundle_version: 1,
        })
    }

    /// 用本 CA 签一张 node cert
    pub fn sign_node_csr(
        &self,
        csr_pem: &str,
        node_id: &str,
        node_name: &str,
    ) -> Result<String> {
        use rcgen::{CertificateSigningRequestParams, ExtendedKeyUsagePurpose};

        let mut csr = CertificateSigningRequestParams::from_pem(csr_pem)
            .context("解析 CSR 失败")?;

        // 强制覆盖 CSR 的 Subject / SAN：node_id 作为 CN（权威身份源）
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, node_id);
        dn.push(DnType::OrganizationName, "relay-rs-node");
        dn.push(DnType::OrganizationalUnitName, node_name);
        csr.params.distinguished_name = dn;

        csr.params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ClientAuth,
            ExtendedKeyUsagePurpose::ServerAuth,
        ];
        csr.params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyEncipherment,
        ];
        csr.params.is_ca = IsCa::NoCa;
        csr.params.not_before = OffsetDateTime::now_utc();
        csr.params.not_after =
            OffsetDateTime::now_utc() + Duration::days(NODE_CERT_VALIDITY_DAYS);

        let cert = csr
            .signed_by(&self.cert, &self.key_pair)
            .context("签发 node cert 失败")?;
        Ok(cert.pem())
    }

    /// 为 master 自己签发一张 gRPC server cert（首次启动时）。
    /// 返回 `(cert_pem, key_pem)`；调用方负责持久化。
    pub fn issue_server_cert(&self, sans: &[String]) -> Result<(String, String)> {
        use rcgen::{ExtendedKeyUsagePurpose, SanType};

        let key_pair = KeyPair::generate().context("生成 server 密钥失败")?;
        let mut params =
            CertificateParams::new(sans.to_vec()).context("server CertificateParams")?;

        // 额外把 IP SAN 也加进去
        for s in sans {
            if let Ok(ip) = s.parse::<std::net::IpAddr>() {
                params.subject_alt_names.push(SanType::IpAddress(ip));
            }
        }

        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, "relay-master");
        dn.push(DnType::OrganizationName, "relay-rs");
        params.distinguished_name = dn;
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyEncipherment,
        ];
        params.is_ca = IsCa::NoCa;
        params.not_before = OffsetDateTime::now_utc();
        params.not_after = OffsetDateTime::now_utc() + Duration::days(NODE_CERT_VALIDITY_DAYS);

        let cert = params
            .signed_by(&key_pair, &self.cert, &self.key_pair)
            .context("签发 server cert 失败")?;
        Ok((cert.pem(), key_pair.serialize_pem()))
    }
}

fn write_secure(path: &Path, content: &str) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true).mode(0o600);
    let mut f = opts.open(path).with_context(|| format!("打开 {:?}", path))?;
    use std::io::Write;
    f.write_all(content.as_bytes())?;
    f.sync_all()?;
    Ok(())
}

pub fn default_ca_dir() -> PathBuf {
    std::env::var("RELAY_MASTER_CA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/etc/relay-master"))
}

// 供 tests 用
#[allow(dead_code)]
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
