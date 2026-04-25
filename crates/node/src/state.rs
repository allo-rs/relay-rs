//! node 持久化状态：session_epoch、applied_revision、actual_hash、apply_errors。
//!
//! 保存到 `<state_dir>/state.json`，每次 apply 后落盘。
//! 用 write-rename 保证原子性。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeState {
    pub session_epoch: u64,
    pub applied_revision: u64,
    #[serde(with = "hex::serde", default)]
    pub applied_hash: Vec<u8>,
    #[serde(with = "hex::serde", default)]
    pub actual_hash: Vec<u8>,
    /// 当前生效的 segments（id → 原 proto）
    #[serde(default)]
    pub segments: BTreeMap<String, SegmentSnapshot>,
    /// 最近一次失败（segment_id → 错误信息）
    #[serde(default)]
    pub apply_errors: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentSnapshot {
    pub listen: String,
    pub proto: i32,
    pub applied_ok: bool,
}

pub fn state_path(dir: &Path) -> PathBuf {
    dir.join("state.json")
}

pub fn load(dir: &Path) -> Result<NodeState> {
    let p = state_path(dir);
    if !p.exists() {
        return Ok(NodeState::default());
    }
    let txt = fs::read_to_string(&p).with_context(|| format!("读 {:?}", p))?;
    let mut s: NodeState = serde_json::from_str(&txt)
        .with_context(|| format!("解析 {:?} 失败，可能是旧格式", p))?;
    s.session_epoch = s.session_epoch.wrapping_add(1); // 每次启动 +1
    save(dir, &s)?;
    Ok(s)
}

pub fn save(dir: &Path, state: &NodeState) -> Result<()> {
    fs::create_dir_all(dir).ok();
    let p = state_path(dir);
    let tmp = dir.join("state.json.tmp");
    let txt = serde_json::to_string_pretty(state)?;
    fs::write(&tmp, txt)?;
    fs::rename(&tmp, &p).with_context(|| format!("rename {:?} → {:?}", tmp, p))?;
    Ok(())
}
