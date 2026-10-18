use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::{api::AppState, auth::AuthUser};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSettingsResponse {
    pub plex_username: Option<String>,
    pub jellyfin_username: Option<String>,
    pub jellyfin_url: Option<String>,
    pub jellyfin_api_key_configured: bool,
    pub jellyfin_api_key_masked: Option<String>,
    pub jellyfin_api_key: Option<String>,

    pub radarr_url: Option<String>,
    pub radarr_api_key_configured: bool,
    pub radarr_api_key_masked: Option<String>,
    pub radarr_api_key: Option<String>,

    pub sonarr_url: Option<String>,
    pub sonarr_api_key_configured: bool,
    pub sonarr_api_key_masked: Option<String>,
    pub sonarr_api_key: Option<String>,

    pub tautulli_url: Option<String>,
    pub tautulli_api_key_configured: bool,
    pub tautulli_api_key_masked: Option<String>,
    pub tautulli_api_key: Option<String>,
}

pub type UserSettingsView = UserSettingsResponse;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateUserSettingsRequest {
    pub plex_username: Option<String>,
    pub jellyfin_username: Option<String>,
    pub jellyfin_url: Option<String>,
    pub jellyfin_api_key: Option<String>,
    pub radarr_url: Option<String>,
    pub radarr_api_key: Option<String>,
    pub sonarr_url: Option<String>,
    pub sonarr_api_key: Option<String>,
    pub tautulli_url: Option<String>,
    pub tautulli_api_key: Option<String>,
}

pub async fn get_user_setting(pool: &SqlitePool, user_id: &str, key: &str) -> Option<String> {
    sqlx::query_scalar("SELECT value FROM user_settings WHERE user_id = ? AND key = ?")
        .bind(user_id)
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
}

pub async fn set_user_setting(pool: &SqlitePool, user_id: &str, key: &str, value: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO user_settings (user_id, key, value, updated_at)
        VALUES (?, ?, ?, datetime('now'))
        ON CONFLICT(user_id, key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
        "#,
    )
    .bind(user_id)
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

fn mask_secret(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    let visible_count = if chars.len() <= 2 { chars.len() } else { 2 };
    let prefix: String = chars[..visible_count].iter().collect();
    let bullets = "•".repeat(chars.len() - visible_count);
    format!("{}{}", prefix, bullets)
}

/// GET /api/v1/user/settings
pub async fn get_user_settings_handler(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let plex_username = get_user_setting(&state.pool, &auth.user_id, "plex_username").await;
    let jellyfin_username = get_user_setting(&state.pool, &auth.user_id, "jellyfin_username").await;
    let jellyfin_url = get_user_setting(&state.pool, &auth.user_id, "jellyfin_url").await;
    let jellyfin_api_key = get_user_setting(&state.pool, &auth.user_id, "jellyfin_api_key").await;

    let radarr_url = get_user_setting(&state.pool, &auth.user_id, "radarr_url").await;
    let radarr_api_key = get_user_setting(&state.pool, &auth.user_id, "radarr_api_key").await;

    let sonarr_url = get_user_setting(&state.pool, &auth.user_id, "sonarr_url").await;
    let sonarr_api_key = get_user_setting(&state.pool, &auth.user_id, "sonarr_api_key").await;

    let tautulli_url = get_user_setting(&state.pool, &auth.user_id, "tautulli_url").await;
    let tautulli_api_key = get_user_setting(&state.pool, &auth.user_id, "tautulli_api_key").await;

    let resp = UserSettingsResponse {
        plex_username,
        jellyfin_username,
        jellyfin_url,
        jellyfin_api_key_configured: jellyfin_api_key.as_ref().map(|k| !k.trim().is_empty()).unwrap_or(false),
        jellyfin_api_key_masked: jellyfin_api_key.as_deref().map(mask_secret),
        jellyfin_api_key: None,

        radarr_url,
        radarr_api_key_configured: radarr_api_key.as_ref().map(|k| !k.trim().is_empty()).unwrap_or(false),
        radarr_api_key_masked: radarr_api_key.as_deref().map(mask_secret),
        radarr_api_key: None,

        sonarr_url,
        sonarr_api_key_configured: sonarr_api_key.as_ref().map(|k| !k.trim().is_empty()).unwrap_or(false),
        sonarr_api_key_masked: sonarr_api_key.as_deref().map(mask_secret),
        sonarr_api_key: None,

        tautulli_url,
        tautulli_api_key_configured: tautulli_api_key.as_ref().map(|k| !k.trim().is_empty()).unwrap_or(false),
        tautulli_api_key_masked: tautulli_api_key.as_deref().map(mask_secret),
        tautulli_api_key: None,
    };

    Ok(Json(resp))
}

/// POST /api/v1/user/settings
pub async fn update_user_settings_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(payload): Json<UpdateUserSettingsRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if let Some(ref val) = payload.plex_username {
        let _ = set_user_setting(&state.pool, &auth.user_id, "plex_username", val.trim()).await;
    }
    if let Some(ref val) = payload.jellyfin_username {
        let _ = set_user_setting(&state.pool, &auth.user_id, "jellyfin_username", val.trim()).await;
    }
    if let Some(ref val) = payload.jellyfin_url {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            crate::api::ssrf::validate_integration_url(trimmed).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("Invalid Jellyfin URL: {}", e) })),
                )
            })?;
        }
        let _ = set_user_setting(&state.pool, &auth.user_id, "jellyfin_url", trimmed).await;
    }
    if let Some(ref val) = payload.jellyfin_api_key {
        if !val.contains('•') {
            let _ = set_user_setting(&state.pool, &auth.user_id, "jellyfin_api_key", val.trim()).await;
        }
    }

    if let Some(ref val) = payload.radarr_url {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            crate::api::ssrf::validate_integration_url(trimmed).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("Invalid Radarr URL: {}", e) })),
                )
            })?;
        }
        let _ = set_user_setting(&state.pool, &auth.user_id, "radarr_url", trimmed).await;
    }
    if let Some(ref val) = payload.radarr_api_key {
        if !val.contains('•') {
            let _ = set_user_setting(&state.pool, &auth.user_id, "radarr_api_key", val.trim()).await;
        }
    }

    if let Some(ref val) = payload.sonarr_url {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            crate::api::ssrf::validate_integration_url(trimmed).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("Invalid Sonarr URL: {}", e) })),
                )
            })?;
        }
        let _ = set_user_setting(&state.pool, &auth.user_id, "sonarr_url", trimmed).await;
    }
    if let Some(ref val) = payload.sonarr_api_key {
        if !val.contains('•') {
            let _ = set_user_setting(&state.pool, &auth.user_id, "sonarr_api_key", val.trim()).await;
        }
    }

    if let Some(ref val) = payload.tautulli_url {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            crate::api::ssrf::validate_integration_url(trimmed).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("Invalid Tautulli URL: {}", e) })),
                )
            })?;
        }
        let _ = set_user_setting(&state.pool, &auth.user_id, "tautulli_url", trimmed).await;
    }
    if let Some(ref val) = payload.tautulli_api_key {
        if !val.contains('•') {
            let _ = set_user_setting(&state.pool, &auth.user_id, "tautulli_api_key", val.trim()).await;
        }
    }

    state.user_sync_notify.notify_one();

    Ok(Json(serde_json::json!({
        "status": "success",
        "message": "User settings updated successfully"
    })))
}
