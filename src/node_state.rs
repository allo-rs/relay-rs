//! node 端运行时状态持久化
//!
//! 规则由 master 通过 HTTP API 推送下来，落到 `/var/lib/relay-rs/state.json`。
//! 重启后 node 先从 state.json 加载上一次的规则，等 master 推下一轮更新再覆盖。

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::Path;

use crate::config::{BlockRule, ForwardRule};

pub const DEFAULT_STATE_PATH: &str = "/var/lib/relay-rs/state.json";

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct NodeState {
    #[serde(default)]
    pub forward: Vec<ForwardRule>,
    #[serde(default)]
    pub block: Vec<BlockRule>,
    /// 单调递增版本号，便于后续与 master 对齐（v0.x 暂不使用）
    #[serde(default)]
    pub revision: u64,
}

pub fn load(path: &str) -> Result<NodeState, Box<dyn std::error::Error>> {
    match fs::read_to_string(path) {
        Ok(content) => {
            let state: NodeState = serde_json::from_str(&content)
                .map_err(|e| format!("解析 {} 失败: {}", path, e))?;
            Ok(state)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(NodeState::default()),
        Err(e) => Err(format!("读取 {} 失败: {}", path, e).into()),
    }
}

/// 原子写入：tmp + fsync + rename + dir fsync
pub fn save(path: &str, state: &NodeState) -> Result<(), Box<dyn std::error::Error>> {
    let p = Path::new(path);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("创建目录 {:?} 失败: {}", parent, e))?;
    }

    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|e| format!("序列化状态失败: {}", e))?;

    let tmp = format!("{}.tmp", path);
    {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)
            .map_err(|e| format!("打开临时文件 {} 失败: {}", tmp, e))?;
        f.write_all(&bytes)
            .map_err(|e| format!("写入临时文件失败: {}", e))?;
        f.sync_all()
            .map_err(|e| format!("fsync 临时文件失败: {}", e))?;
    }

    fs::rename(&tmp, path)
        .map_err(|e| format!("原子替换 {} 失败: {}", path, e))?;

    // 目录 fsync（best-effort，失败不致命）
    if let Some(parent) = p.parent() {
        if let Ok(d) = fs::File::open(parent) {
            let _ = d.sync_all();
        }
    }

    Ok(())
}

/// 首次启动自动迁移：若 state.json 不存在但老 TOML 存在，把 forward/block 导入
/// 并将老 TOML 重命名为 `.migrated-bak`，避免下次再次触发
pub fn migrate_from_toml_if_needed(state_path: &str, toml_path: &str) {
    if Path::new(state_path).exists() {
        return;
    }
    if !Path::new(toml_path).exists() {
        return;
    }

    match crate::config::load(toml_path) {
        Ok(cfg) => {
            let state = NodeState {
                forward: cfg.forward,
                block: cfg.block,
                revision: 0,
            };
            match save(state_path, &state) {
                Ok(()) => {
                    log::info!(
                        "已将旧配置 {} 的 forward/block 规则迁移到 {}",
                        toml_path,
                        state_path
                    );
                    let backup = format!("{}.migrated-bak", toml_path);
                    if let Err(e) = fs::rename(toml_path, &backup) {
                        log::warn!("重命名旧配置 {} → {} 失败: {}", toml_path, backup, e);
                    } else {
                        log::info!("旧配置已备份为 {}", backup);
                    }
                }
                Err(e) => log::error!("迁移规则失败: {}", e),
            }
        }
        Err(e) => log::warn!("无法加载旧配置 {}: {}，跳过迁移", toml_path, e),
    }
}
