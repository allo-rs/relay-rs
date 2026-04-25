//! 嵌入打包好的前端 SPA（`panel/dist/`）。
//!
//! 路径相对 `crates/master/Cargo.toml`，所以是 `../../panel/dist/`。
//! 若 dist 为空（CI / 开发环境未跑 `bun run build`），所有资源访问会回落到一个
//! 占位 HTML，避免编译期/运行期失败。

use axum::{
    body::Body,
    http::{Response, Uri, header},
    response::IntoResponse,
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../panel/dist/"]
#[exclude = ".gitkeep"]
struct PanelAssets;

const PLACEHOLDER_HTML: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>relay-master panel</title>
<style>body{font-family:system-ui;background:#0b0d12;color:#e6e6e6;padding:48px;line-height:1.6}
code{background:#1c1f26;padding:2px 6px;border-radius:4px}</style></head>
<body><h1>relay-master panel — frontend not built</h1>
<p>构建产物 <code>panel/dist/</code> 未嵌入。请在 build 前运行：</p>
<pre><code>cd panel &amp;&amp; bun install &amp;&amp; bun run build</code></pre>
<p>API 路径仍正常工作，例如 <code>GET /api/v1/nodes</code>。</p>
</body></html>"#;

/// 是否存在嵌入产物（非空 dist）。
fn has_assets() -> bool {
    PanelAssets::iter().next().is_some()
}

fn placeholder_response() -> Response<Body> {
    Response::builder()
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(Body::from(PLACEHOLDER_HTML))
        .unwrap()
}

/// SPA 静态资源处理：路径含 `.` 视为静态文件，缺失时 404；其他视为前端路由，
/// 回落到 `index.html`。整体策略与 v0 `src/panel/assets.rs` 一致。
pub async fn serve_asset(uri: Uri) -> impl IntoResponse {
    if !has_assets() {
        return placeholder_response();
    }

    let path = uri.path().trim_start_matches('/');
    let is_file = path.contains('.');
    let lookup = if path.is_empty() || !is_file {
        "index.html"
    } else {
        path
    };

    if let Some(content) = PanelAssets::get(lookup) {
        let mime = mime_guess::from_path(lookup).first_or_octet_stream();
        return Response::builder()
            .header(header::CONTENT_TYPE, mime.as_ref())
            .body(Body::from(content.data.into_owned()))
            .unwrap();
    }

    // 静态文件不存在：若路径像文件（含点）则 404，否则 SPA fallback 到 index.html。
    if is_file {
        return Response::builder()
            .status(404)
            .body(Body::from("Not Found"))
            .unwrap();
    }

    match PanelAssets::get("index.html") {
        Some(content) => Response::builder()
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Body::from(content.data.into_owned()))
            .unwrap(),
        None => placeholder_response(),
    }
}
