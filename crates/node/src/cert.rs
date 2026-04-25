//! node 侧证书管理：生成 keypair + CSR；持久化注册结果。

use anyhow::{Context, Result};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct Paths {
    pub cert: PathBuf,
    pub key: PathBuf,
    pub ca: PathBuf,
    pub node_id: PathBuf,
    pub ca_version: PathBuf,
}

pub fn paths(dir: &Path) -> Paths {
    Paths {
        cert: dir.join("node.pem"),
        key: dir.join("node.key"),
        ca: dir.join("ca.pem"),
        node_id: dir.join("node_id"),
        ca_version: dir.join("ca.bundle_version"),
    }
}

/// 生成 keypair + CSR。返回 `(csr_pem, key_pem)`。
/// CSR 里的 DN 只是提示，master 签发时会**强制覆盖**为 node_id/node_name。
pub fn generate_csr(node_name: &str) -> Result<(String, String)> {
    let key_pair = KeyPair::generate().context("生成 node 密钥失败")?;
    let mut params = CertificateParams::new(vec![]).context("CertificateParams::new")?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, node_name);
    dn.push(DnType::OrganizationName, "relay-rs-node");
    params.distinguished_name = dn;

    let csr = params
        .serialize_request(&key_pair)
        .context("生成 CSR 失败")?;
    Ok((csr.pem()?, key_pair.serialize_pem()))
}

pub fn save(
    dir: &Path,
    node_id: &str,
    cert_pem: &[u8],
    key_pem: &[u8],
    ca_bundle: &[Vec<u8>],
    ca_bundle_version: u32,
) -> Result<()> {
    let p = paths(dir);
    write_secure(&p.cert, cert_pem)?;
    write_secure(&p.key, key_pem)?;

    // CA bundle：多份 cert concat
    let mut ca_concat = Vec::new();
    for c in ca_bundle {
        ca_concat.extend_from_slice(c);
        if !ca_concat.ends_with(b"\n") {
            ca_concat.push(b'\n');
        }
    }
    write_secure(&p.ca, &ca_concat)?;

    fs::write(&p.node_id, node_id)?;
    fs::write(&p.ca_version, ca_bundle_version.to_string())?;
    Ok(())
}

fn write_secure(path: &Path, content: &[u8]) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("打开 {:?}", path))?;
    f.write_all(content)?;
    f.sync_all()?;
    Ok(())
}
