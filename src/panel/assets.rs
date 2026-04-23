use axum::{
    body::Body,
    http::{Uri, header, Response},
    response::IntoResponse,
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "panel/dist/"]
struct PanelAssets;

/// 提供嵌入的前端静态资源，不含 '.' 的路径统一回退到 index.html（SPA 路由）
pub async fn serve_asset(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() || !path.contains('.') {
        "index.html"
    } else {
        path
    };

    match PanelAssets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            Response::builder()
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(content.data))
                .unwrap()
        }
        None => {
            // SPA fallback：不存在的资源也返回 index.html
            match PanelAssets::get("index.html") {
                Some(content) => Response::builder()
                    .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                    .body(Body::from(content.data))
                    .unwrap(),
                None => Response::builder()
                    .status(404)
                    .body(Body::from("Not Found"))
                    .unwrap(),
            }
        }
    }
}
