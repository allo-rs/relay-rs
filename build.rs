use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=panel/src");
    println!("cargo:rerun-if-changed=panel/package.json");

    // 确保 panel/dist 目录存在，避免 rust-embed 编译失败
    let dist = Path::new("panel/dist");
    if !dist.exists() {
        fs::create_dir_all(dist).expect("无法创建 panel/dist 目录");
        fs::write(
            dist.join("index.html"),
            "<html><body><h2>前端未构建，请在 panel/ 目录下运行 bun install && bun run build</h2></body></html>",
        )
        .expect("无法写入占位 index.html");
    }
}
