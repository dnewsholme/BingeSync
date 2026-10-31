use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::{api::AppState, auth::AuthUser};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemSettingsView {
    pub metadata_source: String,
    pub tvdb_api_key_configured: bool,
    pub tvdb_api_key_masked: Option<String>,
    pub tvdb_api_key: Option<String>,
    pub tvdb_pin: Option<String>,
    pub tvdb_base_url: Option<String>,

    pub tmdb_api_key_configured: bool,
    pub tmdb_api_key_masked: Option<String>,
    pub tmdb_api_key: Option<String>,
    pub tmdb_base_url: Option<String>,

    pub oidc_enabled: bool,
    pub oidc_issuer_url: Option<String>,
    pub oidc_client_id: Option<String>,
    pub oidc_client_secret_configured: bool,
    pub oidc_client_secret_masked: Option<String>,
    pub oidc_client_secret: Option<String>,
    pub oidc_redirect_url: Option<String>,
    pub oidc_admin_group: Option<String>,
    pub oidc_user_group: Option<String>,
    pub scrobble_threshold_percent: i64,
    pub task_image_interval_hours: u64,
    pub task_tvdb_interval_hours: u64,

    pub app_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub release_url: Option<String>,
    pub release_notes: Option<String>,
    #[serde(default)]
    pub docker_base_image: Option<String>,
    #[serde(default)]
    pub sqlite_version: Option<String>,
    #[serde(default)]
    pub rust_version: Option<String>,
    #[serde(default)]
    pub components: Option<Vec<SystemComponentInfo>>,
}

pub const APP_VERSION: &str = match option_env!("BINGESYNC_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

pub const RUSTC_VERSION: &str = match option_env!("RUSTC_VERSION") {
    Some(v) => v,
    None => "rustc 1.99+",
};

pub const AXUM_VERSION: &str = match option_env!("AXUM_VERSION") {
    Some(v) => v,
    None => "0.8",
};

pub const TOKIO_VERSION: &str = match option_env!("TOKIO_VERSION") {
    Some(v) => v,
    None => "1.40",
};

pub const VITE_VERSION: &str = match option_env!("VITE_VERSION") {
    Some(v) => v,
    None => "8.3.5",
};

pub fn get_os_pretty_name() -> String {
    if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
        for line in content.lines() {
            if let Some(val) = line.strip_prefix("PRETTY_NAME=") {
                let unquoted = val.trim_matches('"').trim_matches('\'').trim();
                if !unquoted.is_empty() {
                    return unquoted.to_string();
                }
            }
        }
    }
    format!("{} ({})", std::env::consts::OS, std::env::consts::ARCH)
}

pub fn get_docker_base_image() -> String {
    if let Ok(val) = std::env::var("DOCKER_BASE_IMAGE") {
        let clean = val.trim();
        if !clean.is_empty() {
            return clean.to_string();
        }
    }
    "debian:trixie-slim".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemComponentInfo {
    pub name: String,
    pub version: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionCheckResponse {
    pub current_version: String,
    pub app_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub release_url: Option<String>,
    pub release_notes: Option<String>,
    pub published_at: Option<String>,

    pub docker_base_image: String,
    pub os_name: String,
    pub sqlite_version: String,
    pub rust_version: String,
    pub target_arch: String,
    pub target_os: String,
    pub components: Vec<SystemComponentInfo>,
}

pub async fn build_system_components(
    pool: Option<&sqlx::SqlitePool>,
) -> (String, String, String, String, Vec<SystemComponentInfo>) {
    let sqlite_ver = if let Some(p) = pool {
        sqlx::query_scalar("SELECT sqlite_version()")
            .fetch_one(p)
            .await
            .unwrap_or_else(|_| "3.45+".to_string())
    } else {
        "3.45+".to_string()
    };

    let docker_base = get_docker_base_image();
    let os_name = get_os_pretty_name();
    let rustc_ver = RUSTC_VERSION.to_string();

    let components = vec![
        SystemComponentInfo {
            name: "BingeSync".to_string(),
            version: format!("v{}", APP_VERSION),
            description: "Self-hosted media tracker and scrobbler service".to_string(),
        },
        SystemComponentInfo {
            name: "Docker Base Image".to_string(),
            version: docker_base.clone(),
            description: "Official minimal Debian slim container runtime".to_string(),
        },
        SystemComponentInfo {
            name: "Operating System".to_string(),
            version: os_name.clone(),
            description: format!(
                "Host/container runtime environment ({})",
                std::env::consts::ARCH
            ),
        },
        SystemComponentInfo {
            name: "SQLite Engine".to_string(),
            version: sqlite_ver.clone(),
            description: "Embedded SQL database engine (WAL journal mode)".to_string(),
        },
        SystemComponentInfo {
            name: "Rust Toolchain".to_string(),
            version: rustc_ver.clone(),
            description: "Binary compiler and build environment".to_string(),
        },
        SystemComponentInfo {
            name: "Axum Web Framework".to_string(),
            version: format!("v{}", AXUM_VERSION.trim_start_matches('v')),
            description: "Asynchronous HTTP routing and middleware framework".to_string(),
        },
        SystemComponentInfo {
            name: "Tokio Async Runtime".to_string(),
            version: format!("v{}", TOKIO_VERSION.trim_start_matches('v')),
            description: "Multi-threaded asynchronous task runtime".to_string(),
        },
        SystemComponentInfo {
            name: "Web Interface".to_string(),
            version: format!("Vite {} / SPA", VITE_VERSION.trim_start_matches('v')),
            description: "Embedded reactive single-page frontend".to_string(),
        },
    ];

    (docker_base, os_name, sqlite_ver, rustc_ver, components)
}

pub fn is_version_newer(latest: &str, current: &str) -> bool {
    let parse_parts = |s: &str| -> Vec<u64> {
        s.trim_start_matches('v')
            .split('.')
            .filter_map(|p| p.parse::<u64>().ok())
            .collect()
    };
    let l_parts = parse_parts(latest);
    let c_parts = parse_parts(current);
    if !l_parts.is_empty() && !c_parts.is_empty() {
        l_parts > c_parts
    } else {
        let l_clean = latest.trim_start_matches('v');
        let c_clean = current.trim_start_matches('v');
        l_clean != c_clean && !l_clean.is_empty()
    }
}

pub async fn check_latest_github_release(pool: Option<&sqlx::SqlitePool>) -> VersionCheckResponse {
    let current = APP_VERSION.to_string();
    let (docker_base_image, os_name, sqlite_version, rust_version, components) =
        build_system_components(pool).await;

    let client = reqwest::Client::builder()
        .user_agent("BingeSync-Server")
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    let res = client
        .get("https://api.github.com/repos/dnewsholme/BingeSync/releases/latest")
        .send()
        .await;

    if let Ok(resp) = res {
        if resp.status().is_success() {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if let Some(tag) = json.get("tag_name").and_then(|v| v.as_str()) {
                    let latest = tag.trim_start_matches('v').to_string();
                    let release_url = json
                        .get("html_url")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let release_notes = json
                        .get("body")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let published_at = json
                        .get("published_at")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let update_available = is_version_newer(&latest, &current);

                    return VersionCheckResponse {
                        current_version: current.clone(),
                        app_version: current,
                        latest_version: Some(latest),
                        update_available,
                        release_url,
                        release_notes,
                        published_at,
                        docker_base_image,
                        os_name,
                        sqlite_version,
                        rust_version,
                        target_arch: std::env::consts::ARCH.to_string(),
                        target_os: std::env::consts::OS.to_string(),
                        components,
                    };
                }
            }
        }
    }

    VersionCheckResponse {
        current_version: current.clone(),
        app_version: current,
        latest_version: None,
        update_available: false,
        release_url: None,
        release_notes: None,
        published_at: None,
        docker_base_image,
        os_name,
        sqlite_version,
        rust_version,
        target_arch: std::env::consts::ARCH.to_string(),
        target_os: std::env::consts::OS.to_string(),
        components,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSettingsRequest {
    pub metadata_source: Option<String>,
    pub tvdb_api_key: Option<String>,
    pub tvdb_pin: Option<String>,
    pub tvdb_base_url: Option<String>,

    pub tmdb_api_key: Option<String>,
    pub tmdb_base_url: Option<String>,

    pub oidc_enabled: Option<bool>,
    pub oidc_issuer_url: Option<String>,
    pub oidc_client_id: Option<String>,
    pub oidc_client_secret: Option<String>,
    pub oidc_redirect_url: Option<String>,
    pub oidc_admin_group: Option<String>,
    pub oidc_user_group: Option<String>,
    pub scrobble_threshold_percent: Option<i64>,
    pub task_image_interval_hours: Option<u64>,
    pub task_tvdb_interval_hours: Option<u64>,
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

pub async fn get_admin_settings_handler(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if auth.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(
                serde_json::json!({ "error": "Admin privileges required to view system settings" }),
            ),
        ));
    }

    let metadata_source = match get_db_setting(&state.pool, "metadata_source").await {
        Some(v) if v.to_lowercase() == "tmdb" || v.to_lowercase() == "themoviedb" => {
            "tmdb".to_string()
        }
        Some(v) if v.to_lowercase() == "tvdb" || v.to_lowercase() == "thetvdb" => {
            "tvdb".to_string()
        }
        _ => state.config.metadata_source.clone(),
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

    let tmdb_api_key = match get_db_setting(&state.pool, "tmdb_api_key").await {
        Some(v) if !v.trim().is_empty() => Some(v),
        _ => state.config.tmdb_api_key.clone(),
    };
    let tmdb_base_url = match get_db_setting(&state.pool, "tmdb_base_url").await {
        Some(v) if !v.trim().is_empty() => Some(v),
        _ => Some(state.config.tmdb_base_url.clone()),
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
    let oidc_admin_group = match get_db_setting(&state.pool, "oidc_admin_group").await {
        Some(v) if !v.trim().is_empty() => Some(v),
        _ => state.config.oidc_admin_group.clone(),
    };
    let oidc_user_group = match get_db_setting(&state.pool, "oidc_user_group").await {
        Some(v) if !v.trim().is_empty() => Some(v),
        _ => state.config.oidc_user_group.clone(),
    };

    let ver_info = check_latest_github_release(Some(&state.pool)).await;

    let view = SystemSettingsView {
        metadata_source,
        tvdb_api_key_configured: tvdb_api_key.is_some()
            && !tvdb_api_key.as_deref().unwrap_or("").is_empty(),
        tvdb_api_key_masked: tvdb_api_key.as_deref().map(mask_api_key),
        tvdb_api_key: None,
        tvdb_pin,
        tvdb_base_url,

        tmdb_api_key_configured: tmdb_api_key.is_some()
            && !tmdb_api_key.as_deref().unwrap_or("").is_empty(),
        tmdb_api_key_masked: tmdb_api_key.as_deref().map(mask_api_key),
        tmdb_api_key: None,
        tmdb_base_url,

        oidc_enabled,
        oidc_issuer_url,
        oidc_client_id,
        oidc_client_secret_configured: oidc_client_secret.is_some()
            && !oidc_client_secret.as_deref().unwrap_or("").is_empty(),
        oidc_client_secret_masked: oidc_client_secret.as_deref().map(mask_api_key),
        oidc_client_secret: None,
        oidc_redirect_url,
        oidc_admin_group,
        oidc_user_group,
        scrobble_threshold_percent: state.get_effective_scrobble_threshold().await,
        task_image_interval_hours: state.get_task_interval_hours("image_cache").await,
        task_tvdb_interval_hours: state.get_task_interval_hours("tvdb_metadata").await,

        app_version: ver_info.current_version,
        latest_version: ver_info.latest_version,
        update_available: ver_info.update_available,
        release_url: ver_info.release_url,
        release_notes: ver_info.release_notes,
        docker_base_image: Some(ver_info.docker_base_image),
        sqlite_version: Some(ver_info.sqlite_version),
        rust_version: Some(ver_info.rust_version),
        components: Some(ver_info.components),
    };

    Ok(Json(view))
}

pub async fn get_admin_version_handler(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if auth.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Admin privileges required" })),
        ));
    }
    let info = check_latest_github_release(Some(&state.pool)).await;
    Ok(Json(serde_json::to_value(info).unwrap()))
}

pub async fn update_admin_settings_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(payload): Json<UpdateSettingsRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if auth.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(
                serde_json::json!({ "error": "Admin privileges required to update system settings" }),
            ),
        ));
    }

    let mut metadata_updated = false;

    if let Some(ref source) = payload.metadata_source {
        let clean_src = source.trim().to_lowercase();
        if clean_src == "tmdb" || clean_src == "themoviedb" {
            let _ = set_db_setting(&state.pool, "metadata_source", "tmdb").await;
            metadata_updated = true;
        } else if clean_src == "tvdb" || clean_src == "thetvdb" {
            let _ = set_db_setting(&state.pool, "metadata_source", "tvdb").await;
            metadata_updated = true;
        }
    }

    if let Some(ref val) = payload.tvdb_api_key {
        if !val.trim().is_empty() && !val.contains('•') {
            let _ = set_db_setting(&state.pool, "tvdb_api_key", val.trim()).await;
            metadata_updated = true;
        }
    }
    if let Some(ref val) = payload.tvdb_pin {
        let _ = set_db_setting(&state.pool, "tvdb_pin", val.trim()).await;
        metadata_updated = true;
    }
    if let Some(ref val) = payload.tvdb_base_url {
        let _ = set_db_setting(&state.pool, "tvdb_base_url", val.trim()).await;
        metadata_updated = true;
    }

    if let Some(ref val) = payload.tmdb_api_key {
        if !val.trim().is_empty() && !val.contains('•') {
            let _ = set_db_setting(&state.pool, "tmdb_api_key", val.trim()).await;
            metadata_updated = true;
        }
    }
    if let Some(ref val) = payload.tmdb_base_url {
        let _ = set_db_setting(&state.pool, "tmdb_base_url", val.trim()).await;
        metadata_updated = true;
    }

    if let Some(enabled) = payload.oidc_enabled {
        let _ = set_db_setting(
            &state.pool,
            "oidc_enabled",
            if enabled { "true" } else { "false" },
        )
        .await;
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
    if let Some(ref val) = payload.oidc_admin_group {
        let _ = set_db_setting(&state.pool, "oidc_admin_group", val.trim()).await;
    }
    if let Some(ref val) = payload.oidc_user_group {
        let _ = set_db_setting(&state.pool, "oidc_user_group", val.trim()).await;
    }

    if let Some(pct) = payload.scrobble_threshold_percent {
        let clamped = pct.clamp(1, 100);
        let _ = set_db_setting(
            &state.pool,
            "scrobble_threshold_percent",
            &clamped.to_string(),
        )
        .await;
    }

    if let Some(hours) = payload.task_image_interval_hours {
        let _ = state.set_task_interval_hours("image_cache", hours).await;
        let next_run = (chrono::Utc::now() + chrono::Duration::hours(hours as i64)).to_rfc3339();
        state.task_tracker.set_image_next_run(Some(next_run)).await;
    }
    if let Some(hours) = payload.task_tvdb_interval_hours {
        let _ = state.set_task_interval_hours("tvdb_metadata", hours).await;
        let next_run = (chrono::Utc::now() + chrono::Duration::hours(hours as i64)).to_rfc3339();
        state.task_tracker.set_tvdb_next_run(Some(next_run)).await;
    }

    tracing::info!(
        admin_id = %auth.user_id,
        metadata_source = ?payload.metadata_source,
        metadata_updated = metadata_updated,
        scrobble_threshold_percent = ?payload.scrobble_threshold_percent,
        task_image_interval_hours = ?payload.task_image_interval_hours,
        task_tvdb_interval_hours = ?payload.task_tvdb_interval_hours,
        oidc_enabled = ?payload.oidc_enabled,
        "System settings updated by admin"
    );

    if metadata_updated {
        state.metadata_notify.notify_waiters();
        state.metadata_notify.notify_one();
    }

    Ok(Json(serde_json::json!({
        "status": "success",
        "message": "System settings saved successfully"
    })))
}
