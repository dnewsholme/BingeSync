pub mod admin;
pub mod auth;
pub mod history;
pub mod images;
pub mod integrations;
pub mod jobs;
pub mod logs;
pub mod now_playing;
pub mod settings;
pub mod stats;
pub mod timeline;
pub mod trakt_import;
pub mod tautulli_import;
pub mod openapi;
pub mod tvdb;
pub mod user;
pub mod webhooks;

use axum::{
    routing::{delete, get, post},
    Extension, Router,
};
use sqlx::SqlitePool;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

use crate::{auth::OidcClient, config::Config};

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub config: Arc<Config>,
    pub oidc_client: Option<Arc<OidcClient>>,
    pub image_notify: Arc<tokio::sync::Notify>,
    pub metadata_notify: Arc<tokio::sync::Notify>,
    pub log_buffer: Arc<logs::LogBuffer>,
    pub task_tracker: Arc<jobs::BackgroundTaskTracker>,
}

impl AppState {
    pub async fn get_effective_tvdb_api_key(&self) -> Option<String> {
        if let Some(key) = settings::get_db_setting(&self.pool, "tvdb_api_key").await {
            if !key.trim().is_empty() {
                return Some(key);
            }
        }
        self.config.tvdb_api_key.clone()
    }

    pub async fn get_effective_tvdb_pin(&self) -> Option<String> {
        if let Some(pin) = settings::get_db_setting(&self.pool, "tvdb_pin").await {
            if !pin.trim().is_empty() {
                return Some(pin);
            }
        }
        self.config.tvdb_pin.clone()
    }

    pub async fn get_effective_tvdb_base_url(&self) -> String {
        if let Some(url) = settings::get_db_setting(&self.pool, "tvdb_base_url").await {
            if !url.trim().is_empty() {
                return url;
            }
        }
        self.config.tvdb_base_url.clone()
    }

    pub async fn get_effective_radarr_url(&self) -> Option<String> {
        if let Some(url) = settings::get_db_setting(&self.pool, "radarr_url").await {
            if !url.trim().is_empty() {
                return Some(url);
            }
        }
        self.config.radarr_url.clone()
    }

    pub async fn get_effective_radarr_api_key(&self) -> Option<String> {
        if let Some(key) = settings::get_db_setting(&self.pool, "radarr_api_key").await {
            if !key.trim().is_empty() {
                return Some(key);
            }
        }
        self.config.radarr_api_key.clone()
    }

    pub async fn get_effective_sonarr_url(&self) -> Option<String> {
        if let Some(url) = settings::get_db_setting(&self.pool, "sonarr_url").await {
            if !url.trim().is_empty() {
                return Some(url);
            }
        }
        self.config.sonarr_url.clone()
    }

    pub async fn get_effective_sonarr_api_key(&self) -> Option<String> {
        if let Some(key) = settings::get_db_setting(&self.pool, "sonarr_api_key").await {
            if !key.trim().is_empty() {
                return Some(key);
            }
        }
        self.config.sonarr_api_key.clone()
    }

    pub async fn get_effective_tautulli_url(&self) -> Option<String> {
        if let Some(url) = settings::get_db_setting(&self.pool, "tautulli_url").await {
            if !url.trim().is_empty() {
                return Some(url);
            }
        }
        self.config.tautulli_url.clone()
    }

    pub async fn get_effective_tautulli_api_key(&self) -> Option<String> {
        if let Some(key) = settings::get_db_setting(&self.pool, "tautulli_api_key").await {
            if !key.trim().is_empty() {
                return Some(key);
            }
        }
        self.config.tautulli_api_key.clone()
    }

    pub async fn get_effective_scrobble_threshold(&self) -> i64 {
        if let Some(val) = settings::get_db_setting(&self.pool, "scrobble_threshold_percent").await {
            if let Ok(pct) = val.trim().parse::<i64>() {
                if (1..=100).contains(&pct) {
                    return pct;
                }
            }
        }
        self.config.scrobble_threshold_percent
    }
}

pub fn create_api_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // Auth
        .route("/api/v1/auth/status", get(auth::get_auth_status))
        .route("/api/v1/auth/login", get(auth::login_handler))
        .route("/api/v1/auth/callback", get(auth::callback_handler))
        .route("/api/v1/auth/dev-login", post(auth::dev_login_handler))
        .route("/api/v1/auth/me", get(auth::get_me_handler))
        .route("/api/v1/auth/logout", post(auth::logout_handler))
        .route("/api/v1/auth/backchannel-logout", post(auth::backchannel_logout_handler))
        .route("/api/v1/auth/backchannel_logout", post(auth::backchannel_logout_handler))
        // Webhooks (Per-user Plex endpoints)
        .route("/api/v1/webhooks/plex", post(webhooks::handle_plex_webhook_query))
        .route("/api/v1/webhooks/plex/{token}", post(webhooks::handle_plex_webhook_path))
        // History & Scrobble
        .route("/api/v1/history", get(history::list_history).post(history::create_scrobble))
        .route("/api/v1/history/by-show", delete(history::delete_by_show))
        .route("/api/v1/history/delete-batch", post(history::delete_batch))
        .route("/api/v1/history/by-date", delete(history::delete_by_date))
        .route("/api/v1/history/{id}/watches", get(history::get_item_watches))
        .route("/api/v1/history/{id}/rate", post(history::rate_scrobble).put(history::rate_scrobble))
        .route("/api/v1/history/{id}", delete(history::delete_scrobble))
        // Timeline & Stats
        .route("/api/v1/timeline", get(timeline::get_timeline))
        .route("/api/v1/stats", get(stats::get_user_stats))
        .route("/api/v1/now-playing", get(now_playing::get_now_playing))
        // User settings
        .route("/api/v1/user/webhook", get(user::get_webhook_info))
        .route("/api/v1/user/regenerate-token", post(user::regenerate_webhook_token))
        // Trakt import (supports up to 100MB zip exports)
        .route(
            "/api/v1/import/trakt",
            post(trakt_import::import_trakt_handler)
                .layer(axum::extract::DefaultBodyLimit::max(100 * 1024 * 1024)),
        )
        // Tautulli import & users
        .route(
            "/api/v1/import/tautulli",
            post(tautulli_import::import_tautulli_handler)
                .layer(axum::extract::DefaultBodyLimit::max(100 * 1024 * 1024)),
        )
        .route("/api/v1/import/tautulli/users", get(tautulli_import::get_tautulli_users_handler))
        // Image cache (serves cached posters from disk)
        .route("/api/v1/images/{filename}", get(images::get_image_handler))
        // Radarr / Sonarr Integrations (Cleanup after watching)
        .route("/api/v1/integrations/status", get(integrations::get_integrations_status))
        .route("/api/v1/integrations/check", get(integrations::check_media_in_library))
        .route("/api/v1/integrations/radarr/delete", post(integrations::delete_from_radarr))
        .route("/api/v1/integrations/sonarr/delete-series", post(integrations::delete_from_sonarr))
        // Admin management: Users, Settings, Logs, and Tasks
        .route("/api/v1/admin/users", get(admin::list_users))
        .route("/api/v1/admin/users/{id}", delete(admin::delete_user))
        .route("/api/v1/admin/settings", get(settings::get_admin_settings_handler).post(settings::update_admin_settings_handler).put(settings::update_admin_settings_handler))
        .route("/api/v1/admin/logs", get(logs::get_admin_logs_handler).delete(logs::clear_admin_logs_handler))
        .route("/api/v1/admin/tasks", get(jobs::get_admin_tasks_handler))
        .route("/api/v1/admin/tasks/{id}/run", post(jobs::trigger_admin_task_handler))
        // OpenAPI specification and Swagger UI
        .route("/api/v1/openapi.json", get(openapi::get_openapi_json))
        .route("/docs", get(openapi::get_swagger_ui))
        .route("/swagger", get(openapi::get_swagger_ui))
        .layer(cors)
        .layer(Extension(state.clone()))
        .with_state(state)
}
