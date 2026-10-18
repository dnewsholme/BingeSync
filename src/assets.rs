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
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, HeaderValue::from_str(mime.as_ref()).unwrap())
                .header("X-Content-Type-Options", "nosniff")
                .header("X-Frame-Options", "SAMEORIGIN")
                .header("Referrer-Policy", "strict-origin-when-cross-origin")
                .body(Body::from(content.data.into_owned()))
                .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Error").into_response())
        }
        None => {
            // Fallback to index.html for SPA client-side routing
            if let Some(content) = WebAssets::get("index.html") {
                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"))
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
