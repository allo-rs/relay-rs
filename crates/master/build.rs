//! 编译期确保 `panel/dist/` 目录存在（rust-embed 要求）。
//!
//! CI / fresh checkout 下 panel/dist 可能不存在，rust-embed 会编译失败。
//! 这里兜底创建占位；运行时 assets.rs 发现 dist 空会走 placeholder HTML。

use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dist_dir = manifest_dir.join("../../panel/dist");
    if let Err(e) = fs::create_dir_all(&dist_dir) {
        println!("cargo:warning=无法创建 {}: {}", dist_dir.display(), e);
        return;
    }
    let placeholder = dist_dir.join(".gitkeep");
    if !placeholder.exists() {
        let _ = fs::write(&placeholder, b"");
    }
    println!("cargo:rerun-if-changed=../../panel/dist");
}
