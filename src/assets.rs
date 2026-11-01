use axum::{
    body::Body,
    http::{header, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "web/dist/"]
pub struct WebAssets;

pub async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // If path is empty, serve index.html
    let file_path = if path.is_empty() {
        "index.html"
    } else {
        path
    };

    match WebAssets::get(file_path) {
        Some(content) => {
            let mime = mime_guess::from_path(file_path).first_or_octet_stream();
            let mut builder = Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, HeaderValue::from_str(mime.as_ref()).unwrap())
                .header("X-Content-Type-Options", "nosniff")
                .header("X-Frame-Options", "SAMEORIGIN")
                .header("Referrer-Policy", "strict-origin-when-cross-origin");

            if file_path == "sw.js" || file_path.ends_with("/sw.js") {
                builder = builder
                    .header("Service-Worker-Allowed", "/")
                    .header(header::CACHE_CONTROL, "no-cache, no-store, must-revalidate");
            } else if file_path.starts_with("assets/") {
                // Vite content-hashed assets can be cached immutably for 1 year
                builder = builder.header(header::CACHE_CONTROL, "public, max-age=31536000, immutable");
            } else if file_path == "index.html" {
                // SPA entry point must always be revalidated to detect new deploys immediately
                builder = builder.header(header::CACHE_CONTROL, "no-cache");
            } else {
                // Other static assets (favicons, manifests, etc.) cached for 1 day
                builder = builder.header(header::CACHE_CONTROL, "public, max-age=86400");
            }

            builder
                .body(Body::from(content.data.into_owned()))
                .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Error").into_response())
        }
        None => {
            // Fallback to index.html for SPA client-side routing
            if let Some(content) = WebAssets::get("index.html") {
                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"))
                    .header(header::CACHE_CONTROL, "no-cache")
                    .header("X-Content-Type-Options", "nosniff")
                    .header("X-Frame-Options", "SAMEORIGIN")
                    .header("Referrer-Policy", "strict-origin-when-cross-origin")
                    .body(Body::from(content.data.into_owned()))
                    .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Error").into_response())
            } else {
                (
                    StatusCode::OK,
                    [
                        (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                        (header::HeaderName::from_static("x-content-type-options"), "nosniff"),
                        (header::HeaderName::from_static("x-frame-options"), "SAMEORIGIN"),
                        (header::HeaderName::from_static("referrer-policy"), "strict-origin-when-cross-origin"),
                    ],
                    r#"<!DOCTYPE html><html><head><title>BingeSync</title></head><body style="font-family:sans-serif;background:#0f172a;color:#f8fafc;padding:40px;"><h1>BingeSync</h1><p>Frontend assets are building or not yet bundled.</p></body></html>"#,
                ).into_response()
            }
        }
    }
}
