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
pub struct SystemSettingsView {
    pub radarr_url: Option<String>,
    pub radarr_api_key_configured: bool,
    pub radarr_api_key_masked: Option<String>,
    pub radarr_api_key: Option<String>,

    pub sonarr_url: Option<String>,
    pub sonarr_api_key_configured: bool,
    pub sonarr_api_key_masked: Option<String>,
    pub sonarr_api_key: Option<String>,

    pub tvdb_api_key_configured: bool,
    pub tvdb_api_key_masked: Option<String>,
    pub tvdb_api_key: Option<String>,
    pub tvdb_pin: Option<String>,
    pub tvdb_base_url: Option<String>,

    pub tautulli_url: Option<String>,
    pub tautulli_api_key_configured: bool,
    pub tautulli_api_key_masked: Option<String>,
    pub tautulli_api_key: Option<String>,

    pub oidc_enabled: bool,
    pub oidc_issuer_url: Option<String>,
    pub oidc_client_id: Option<String>,
    pub oidc_client_secret_configured: bool,
    pub oidc_client_secret_masked: Option<String>,
    pub oidc_client_secret: Option<String>,
    pub oidc_redirect_url: Option<String>,
    pub scrobble_threshold_percent: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSettingsRequest {
    pub radarr_url: Option<String>,
    pub radarr_api_key: Option<String>,

    pub sonarr_url: Option<String>,
    pub sonarr_api_key: Option<String>,

    pub tvdb_api_key: Option<String>,
    pub tvdb_pin: Option<String>,
    pub tvdb_base_url: Option<String>,

    pub tautulli_url: Option<String>,
    pub tautulli_api_key: Option<String>,

    pub oidc_enabled: Option<bool>,
    pub oidc_issuer_url: Option<String>,
    pub oidc_client_id: Option<String>,
    pub oidc_client_secret: Option<String>,
    pub oidc_redirect_url: Option<String>,
    pub scrobble_threshold_percent: Option<i64>,
}

pub async fn get_db_setting(pool: &SqlitePool, key: &str) -> Option<String> {
    sqlx::query_scalar("SELECT value FROM system_settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
}

pub async fn set_db_setting(pool: &SqlitePool, key: &str, value: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO system_settings (key, value, updated_at)
        VALUES (?, ?, datetime('now'))
        ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
        "#,
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_admin_settings_handler(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if auth.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Admin privileges required to view system settings" })),
        ));
    }

    // Check database settings first, then fall back to config
    let radarr_url = match get_db_setting(&state.pool, "radarr_url").await {
        Some(v) if !v.trim().is_empty() => Some(v),
        _ => state.config.radarr_url.clone(),
    };
    let radarr_api_key = match get_db_setting(&state.pool, "radarr_api_key").await {
        Some(v) if !v.trim().is_empty() => Some(v),
        _ => state.config.radarr_api_key.clone(),
    };

    let sonarr_url = match get_db_setting(&state.pool, "sonarr_url").await {
        Some(v) if !v.trim().is_empty() => Some(v),
        _ => state.config.sonarr_url.clone(),
    };
    let sonarr_api_key = match get_db_setting(&state.pool, "sonarr_api_key").await {
        Some(v) if !v.trim().is_empty() => Some(v),
        _ => state.config.sonarr_api_key.clone(),
    };

    let tvdb_api_key = match get_db_setting(&state.pool, "tvdb_api_key").await {
        Some(v) if !v.trim().is_empty() => Some(v),
        _ => state.config.tvdb_api_key.clone(),
    };
    let tvdb_pin = match get_db_setting(&state.pool, "tvdb_pin").await {
        Some(v) if !v.trim().is_empty() => Some(v),
        _ => state.config.tvdb_pin.clone(),
    };
    let tvdb_base_url = match get_db_setting(&state.pool, "tvdb_base_url").await {
        Some(v) if !v.trim().is_empty() => Some(v),
        _ => Some(state.config.tvdb_base_url.clone()),
    };

    let oidc_enabled = match get_db_setting(&state.pool, "oidc_enabled").await {
        Some(v) => v == "true" || v == "1",
        None => state.config.oidc_enabled,
    };
    let oidc_issuer_url = match get_db_setting(&state.pool, "oidc_issuer_url").await {
        Some(v) if !v.trim().is_empty() => Some(v),
        _ => state.config.oidc_issuer_url.clone(),
    };
    let oidc_client_id = match get_db_setting(&state.pool, "oidc_client_id").await {
        Some(v) if !v.trim().is_empty() => Some(v),
        _ => state.config.oidc_client_id.clone(),
    };
    let oidc_client_secret = match get_db_setting(&state.pool, "oidc_client_secret").await {
        Some(v) if !v.trim().is_empty() => Some(v),
        _ => state.config.oidc_client_secret.clone(),
    };
    let oidc_redirect_url = match get_db_setting(&state.pool, "oidc_redirect_url").await {
        Some(v) if !v.trim().is_empty() => Some(v),
        _ => state.config.oidc_redirect_url.clone(),
    };

    let tautulli_url = match get_db_setting(&state.pool, "tautulli_url").await {
        Some(v) if !v.trim().is_empty() => Some(v),
        _ => state.config.tautulli_url.clone(),
    };
    let tautulli_api_key = match get_db_setting(&state.pool, "tautulli_api_key").await {
        Some(v) if !v.trim().is_empty() => Some(v),
        _ => state.config.tautulli_api_key.clone(),
    };

fn mask_api_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    let visible_count = if chars.len() <= 2 { chars.len() } else { 2 };
    let prefix: String = chars[..visible_count].iter().collect();
    let bullets = "•".repeat(chars.len() - visible_count);
    format!("{}{}", prefix, bullets)
}

    let view = SystemSettingsView {
        radarr_url,
        radarr_api_key_configured: radarr_api_key.is_some() && !radarr_api_key.as_deref().unwrap_or("").is_empty(),
        radarr_api_key_masked: radarr_api_key.as_deref().map(mask_api_key),
        radarr_api_key: radarr_api_key.clone(),

        sonarr_url,
        sonarr_api_key_configured: sonarr_api_key.is_some() && !sonarr_api_key.as_deref().unwrap_or("").is_empty(),
        sonarr_api_key_masked: sonarr_api_key.as_deref().map(mask_api_key),
        sonarr_api_key: sonarr_api_key.clone(),

        tvdb_api_key_configured: tvdb_api_key.is_some() && !tvdb_api_key.as_deref().unwrap_or("").is_empty(),
        tvdb_api_key_masked: tvdb_api_key.as_deref().map(mask_api_key),
        tvdb_api_key: tvdb_api_key.clone(),
        tvdb_pin,
        tvdb_base_url,

        tautulli_url,
        tautulli_api_key_configured: tautulli_api_key.is_some() && !tautulli_api_key.as_deref().unwrap_or("").is_empty(),
        tautulli_api_key_masked: tautulli_api_key.as_deref().map(mask_api_key),
        tautulli_api_key: tautulli_api_key.clone(),

        oidc_enabled,
        oidc_issuer_url,
        oidc_client_id,
        oidc_client_secret_configured: oidc_client_secret.is_some() && !oidc_client_secret.as_deref().unwrap_or("").is_empty(),
        oidc_client_secret_masked: oidc_client_secret.as_deref().map(mask_api_key),
        oidc_client_secret: oidc_client_secret.clone(),
        oidc_redirect_url,
        scrobble_threshold_percent: state.get_effective_scrobble_threshold().await,
    };

    Ok(Json(view))
}

pub async fn update_admin_settings_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(payload): Json<UpdateSettingsRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if auth.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Admin privileges required to update system settings" })),
        ));
    }

    // Persist non-empty values or updates (ignoring masked strings containing bullets)
    if let Some(ref val) = payload.radarr_url {
        let _ = set_db_setting(&state.pool, "radarr_url", val.trim()).await;
    }
    if let Some(ref val) = payload.radarr_api_key {
        if !val.trim().is_empty() && !val.contains('•') {
            let _ = set_db_setting(&state.pool, "radarr_api_key", val.trim()).await;
        }
    }

    if let Some(ref val) = payload.sonarr_url {
        let _ = set_db_setting(&state.pool, "sonarr_url", val.trim()).await;
    }
    if let Some(ref val) = payload.sonarr_api_key {
        if !val.trim().is_empty() && !val.contains('•') {
            let _ = set_db_setting(&state.pool, "sonarr_api_key", val.trim()).await;
        }
    }

    let mut tvdb_updated = false;
    if let Some(ref val) = payload.tvdb_api_key {
        if !val.trim().is_empty() && !val.contains('•') {
            let _ = set_db_setting(&state.pool, "tvdb_api_key", val.trim()).await;
            tvdb_updated = true;
        }
    }
    if let Some(ref val) = payload.tvdb_pin {
        let _ = set_db_setting(&state.pool, "tvdb_pin", val.trim()).await;
        tvdb_updated = true;
    }
    if let Some(ref val) = payload.tvdb_base_url {
        let _ = set_db_setting(&state.pool, "tvdb_base_url", val.trim()).await;
        tvdb_updated = true;
    }

    if let Some(ref val) = payload.tautulli_url {
        let _ = set_db_setting(&state.pool, "tautulli_url", val.trim()).await;
    }
    if let Some(ref val) = payload.tautulli_api_key {
        if !val.trim().is_empty() && !val.contains('•') {
            let _ = set_db_setting(&state.pool, "tautulli_api_key", val.trim()).await;
        }
    }

    if let Some(enabled) = payload.oidc_enabled {
        let _ = set_db_setting(&state.pool, "oidc_enabled", if enabled { "true" } else { "false" }).await;
    }
    if let Some(ref val) = payload.oidc_issuer_url {
        let _ = set_db_setting(&state.pool, "oidc_issuer_url", val.trim()).await;
    }
    if let Some(ref val) = payload.oidc_client_id {
        let _ = set_db_setting(&state.pool, "oidc_client_id", val.trim()).await;
    }
    if let Some(ref val) = payload.oidc_client_secret {
        if !val.trim().is_empty() && !val.contains('•') {
            let _ = set_db_setting(&state.pool, "oidc_client_secret", val.trim()).await;
        }
    }
    if let Some(ref val) = payload.oidc_redirect_url {
        let _ = set_db_setting(&state.pool, "oidc_redirect_url", val.trim()).await;
    }

    if let Some(pct) = payload.scrobble_threshold_percent {
        let clamped = pct.clamp(1, 100);
        let _ = set_db_setting(&state.pool, "scrobble_threshold_percent", &clamped.to_string()).await;
    }

    state.log_buffer.push("INFO", "bingesync::settings", &format!("System settings updated by admin ({})", auth.email));

    if tvdb_updated {
        state.metadata_notify.notify_one();
    }

    Ok(Json(serde_json::json!({
        "status": "success",
        "message": "System settings successfully updated"
    })))
}
