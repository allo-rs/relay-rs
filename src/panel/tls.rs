use rcgen::{CertificateParams, DistinguishedName, SanType, KeyPair};
use std::fs;
use std::path::Path;

pub struct TlsFiles {
    pub cert_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
}

/// 若证书文件已存在则读取，否则用 rcgen 生成自签名证书并写入磁盘
pub fn load_or_generate(
    cert_path: &str,
    key_path: &str,
) -> Result<TlsFiles, Box<dyn std::error::Error + Send + Sync>> {
    if Path::new(cert_path).exists() && Path::new(key_path).exists() {
        let cert_pem = fs::read(cert_path)?;
        let key_pem = fs::read(key_path)?;
        log::info!("加载已有 TLS 证书: {}", cert_path);
        return Ok(TlsFiles { cert_pem, key_pem });
    }

    log::info!("未找到 TLS 证书，自动生成自签名证书");

    let mut params = CertificateParams::default();
    params.distinguished_name = DistinguishedName::new();
    params.subject_alt_names = vec![
        SanType::DnsName("relay-rs".try_into()?),
        SanType::DnsName("localhost".try_into()?),
    ];

    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;

    let cert_pem = cert.pem().into_bytes();
    let key_pem = key_pair.serialize_pem().into_bytes();

    // 确保父目录存在
    if let Some(parent) = Path::new(cert_path).parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(cert_path, &cert_pem)?;
    fs::write(key_path, &key_pem)?;
    log::info!("自签名证书已写入: {}", cert_path);

    Ok(TlsFiles { cert_pem, key_pem })
}
