use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::{api::AppState, auth::AuthUser};

#[derive(Debug, Deserialize)]
pub struct OverseerrRequestPayload {
    pub media_type: String, // "movie" or "tv"
    pub media_id: i64,      // TMDB ID
    pub seasons: Option<Vec<i32>>,
}

#[derive(Debug, Serialize)]
pub struct OverseerrStatusResponse {
    pub configured: bool,
    pub overseerr_url: Option<String>,
}

async fn get_effective_overseerr_creds(
    state: &AppState,
    user_id: &str,
) -> (Option<String>, Option<String>) {
    // 1. Check user personal settings first
    let user_url = crate::api::user_settings::get_user_setting(&state.pool, user_id, "overseerr_url").await;
    let user_key = crate::api::user_settings::get_user_setting(&state.pool, user_id, "overseerr_api_key").await;

    if let (Some(u), Some(k)) = (user_url, user_key) {
        if !u.trim().is_empty() && !k.trim().is_empty() {
            return (Some(u.trim().to_string()), Some(k.trim().to_string()));
        }
    }

    // 2. Check admin system settings as fallback
    let admin_url = crate::api::settings::get_db_setting(&state.pool, "overseerr_url").await;
    let admin_key = crate::api::settings::get_db_setting(&state.pool, "overseerr_api_key").await;

    if let (Some(u), Some(k)) = (admin_url, admin_key) {
        if !u.trim().is_empty() && !k.trim().is_empty() {
            return (Some(u.trim().to_string()), Some(k.trim().to_string()));
        }
    }

    (None, None)
}

/// GET /api/v1/integrations/overseerr/status
pub async fn get_overseerr_status_handler(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let (url, key) = get_effective_overseerr_creds(&state, &auth.user_id).await;
    let configured = url.is_some() && key.is_some();
    Ok(Json(serde_json::json!({
        "configured": configured,
        "overseerr_url": url,
    })))
}

/// POST /api/v1/integrations/overseerr/request
pub async fn request_overseerr_media_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(payload): Json<OverseerrRequestPayload>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let (url, key) = get_effective_overseerr_creds(&state, &auth.user_id).await;
    let overseerr_url = match url {
        Some(u) if !u.trim().is_empty() => u,
        _ => return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Overseerr / Jellyseerr is not configured. Please set your URL and API Key in User Settings." })),
        )),
    };
    let overseerr_key = match key {
        Some(k) if !k.trim().is_empty() => k,
        _ => return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Overseerr API Key is missing. Please set your API Key in User Settings." })),
        )),
    };

    crate::api::ssrf::validate_integration_url(&overseerr_url).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("Invalid Overseerr URL: {}", e) })),
        )
    })?;

    let is_tv = payload.media_type.eq_ignore_ascii_case("tv") || payload.media_type.eq_ignore_ascii_case("show") || payload.media_type.eq_ignore_ascii_case("episode");
    let media_type_str = if is_tv { "tv" } else { "movie" };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))))?;

    let endpoint = format!("{}/api/v1/request", overseerr_url.trim_end_matches('/'));

    let mut body = serde_json::json!({
        "mediaType": media_type_str,
        "mediaId": payload.media_id,
    });

    if is_tv {
        if let Some(seasons) = payload.seasons {
            body["seasons"] = serde_json::json!(seasons);
        } else {
            // Request all seasons by default
            body["seasons"] = serde_json::json!("all");
        }
    }

    tracing::info!(
        user_id = %auth.user_id,
        media_type = %media_type_str,
        media_id = payload.media_id,
        endpoint = %endpoint,
        "Sending media request to Overseerr / Jellyseerr"
    );

    let resp = client.post(&endpoint)
        .header("X-Api-Key", &overseerr_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "Failed to connect to Overseerr");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("Failed to connect to Overseerr: {}", e) })),
            )
        })?;

    let status = resp.status();
    let resp_text = resp.text().await.unwrap_or_default();

    if status.is_success() {
        Ok(Json(serde_json::json!({
            "status": "requested",
            "message": "Successfully submitted request to Overseerr / Jellyseerr!"
        })))
    } else if status == StatusCode::CONFLICT || resp_text.contains("already") || resp_text.contains("exists") {
        Ok(Json(serde_json::json!({
            "status": "already_requested",
            "message": "This item has already been requested or is already in your media library!"
        })))
    } else if status == StatusCode::FORBIDDEN || status == StatusCode::UNAUTHORIZED {
        Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Overseerr authentication failed. Check your API key in Settings." })),
        ))
    } else {
        Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("Overseerr returned HTTP {}: {}", status, resp_text) })),
        ))
    }
}
