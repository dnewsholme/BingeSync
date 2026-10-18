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
pub mod user_settings;
pub mod user_sync;
pub mod webhooks;
pub mod ssrf;
pub mod rate_limit;

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
    pub user_sync_notify: Arc<tokio::sync::Notify>,
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

    pub async fn get_task_interval_hours(&self, task_id: &str) -> u64 {
        let key = match task_id {
            "image_cache" | "images" => "task_image_cache_interval_hours",
            "tvdb_metadata" | "tvdb" => "task_tvdb_metadata_interval_hours",
            "user_sync" | "users" | "plex_users" => "task_user_sync_interval_hours",
            _ => return 4,
        };
        if let Some(val) = settings::get_db_setting(&self.pool, key).await {
            if let Ok(hours) = val.trim().parse::<u64>() {
                if (1..=168).contains(&hours) {
                    return hours;
                }
            }
        }
        4
    }

    pub async fn set_task_interval_hours(&self, task_id: &str, hours: u64) -> Result<(), sqlx::Error> {
        let key = match task_id {
            "image_cache" | "images" => "task_image_cache_interval_hours",
            "tvdb_metadata" | "tvdb" => "task_tvdb_metadata_interval_hours",
            "user_sync" | "users" | "plex_users" => "task_user_sync_interval_hours",
            _ => return Ok(()),
        };
        let clamped = hours.clamp(1, 168);
        settings::set_db_setting(&self.pool, key, &clamped.to_string()).await
    }
}

async fn security_headers_middleware(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut resp = next.run(req).await;
    let headers = resp.headers_mut();
    headers.insert(
        axum::http::header::HeaderName::from_static("x-content-type-options"),
        axum::http::HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        axum::http::header::HeaderName::from_static("x-frame-options"),
        axum::http::HeaderValue::from_static("SAMEORIGIN"),
    );
    headers.insert(
        axum::http::header::HeaderName::from_static("referrer-policy"),
        axum::http::HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    resp
}

pub fn create_api_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::DELETE,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::AUTHORIZATION,
            axum::http::header::CONTENT_TYPE,
            axum::http::header::ACCEPT,
            axum::http::header::HeaderName::from_static("x-api-key"),
        ]);

    let auth_routes = Router::new()
        .route("/status", get(auth::get_auth_status))
        .route("/login", get(auth::login_handler))
        .route("/callback", get(auth::callback_handler))
        .route("/dev-login", post(auth::dev_login_handler))
        .route("/me", get(auth::get_me_handler))
        .route("/logout", post(auth::logout_handler))
        .route("/backchannel-logout", post(auth::backchannel_logout_handler))
        .route("/backchannel_logout", post(auth::backchannel_logout_handler))
        .layer(axum::middleware::from_fn(rate_limit::auth_rate_limit_middleware));

    let webhook_routes = Router::new()
        .route("/plex", post(webhooks::handle_plex_webhook_query))
        .route("/plex/{token}", post(webhooks::handle_plex_webhook_path))
        .route("/jellyfin", post(webhooks::handle_jellyfin_webhook_query))
        .route("/jellyfin/{token}", post(webhooks::handle_jellyfin_webhook_path))
        .layer(axum::middleware::from_fn(rate_limit::webhook_rate_limit_middleware));

    let import_routes = Router::new()
        .route(
            "/trakt",
            post(trakt_import::import_trakt_handler)
                .layer(axum::extract::DefaultBodyLimit::max(100 * 1024 * 1024)),
        )
        .route(
            "/tautulli",
            post(tautulli_import::import_tautulli_handler)
                .layer(axum::extract::DefaultBodyLimit::max(100 * 1024 * 1024)),
        )
        .route("/tautulli/users", get(tautulli_import::get_tautulli_users_handler))
        .layer(axum::middleware::from_fn(rate_limit::import_rate_limit_middleware));

    Router::new()
        // Auth with rate limiting
        .nest("/api/v1/auth", auth_routes)
        // Webhooks with rate limiting
        .nest("/api/v1/webhooks", webhook_routes)
        // Imports with rate limiting
        .nest("/api/v1/import", import_routes)
        // History & Scrobble
        .route("/api/v1/history", get(history::list_history).post(history::create_scrobble))
        .route("/api/v1/history/by-show", delete(history::delete_by_show))
        .route("/api/v1/history/delete-batch", post(history::delete_batch))
        .route("/api/v1/history/by-date", delete(history::delete_by_date))
        .route("/api/v1/history/{id}/watches", get(history::get_item_watches))
        .route("/api/v1/history/{id}/rate", post(history::rate_scrobble).put(history::rate_scrobble))
        .route("/api/v1/history/{id}/refresh-metadata", post(history::refresh_scrobble_metadata))
        .route("/api/v1/history/{id}/fix-match", post(history::fix_match_handler).put(history::fix_match_handler))
        .route("/api/v1/history/{id}", delete(history::delete_scrobble))
        // Timeline & Stats
        .route("/api/v1/timeline", get(timeline::get_timeline))
        .route("/api/v1/timeline/periods", get(timeline::get_timeline_periods))
        .route("/api/v1/stats", get(stats::get_user_stats))
        .route("/api/v1/now-playing", get(now_playing::get_now_playing))
        // User settings
        .route("/api/v1/user/webhook", get(user::get_webhook_info))
        .route("/api/v1/user/regenerate-token", post(user::regenerate_webhook_token))
        .route("/api/v1/user/settings", get(user_settings::get_user_settings_handler).post(user_settings::update_user_settings_handler).put(user_settings::update_user_settings_handler))
        // Image cache (serves cached posters from disk)
        .route("/api/v1/images/{filename}", get(images::get_image_handler))
        // Radarr / Sonarr Integrations (Cleanup after watching)
        .route("/api/v1/integrations/status", get(integrations::get_integrations_status))
        .route("/api/v1/integrations/check", get(integrations::check_media_in_library))
        .route("/api/v1/integrations/cleanup", get(integrations::get_cleanup_items))
        .route("/api/v1/integrations/radarr/delete", post(integrations::delete_from_radarr))
        .route("/api/v1/integrations/sonarr/delete-series", post(integrations::delete_from_sonarr))
        // Admin management: Users, Settings, Logs, and Tasks
        .route("/api/v1/admin/users", get(admin::list_users))
        .route("/api/v1/admin/users/{id}", delete(admin::delete_user))
        .route("/api/v1/admin/settings", get(settings::get_admin_settings_handler).post(settings::update_admin_settings_handler).put(settings::update_admin_settings_handler))
        .route("/api/v1/admin/logs", get(logs::get_admin_logs_handler).delete(logs::clear_admin_logs_handler))
        .route("/api/v1/admin/tasks", get(jobs::get_admin_tasks_handler))
        .route("/api/v1/admin/tasks/{id}/run", post(jobs::trigger_admin_task_handler))
        .route("/api/v1/admin/tasks/{id}/schedule", post(jobs::update_admin_task_schedule_handler))
        .route("/api/v1/admin/metadata/force-refresh", post(jobs::force_refresh_all_metadata_handler))
        .route("/api/v1/admin/images/force-refresh", post(jobs::force_refresh_all_images_handler))
        .route("/api/v1/admin/users-cache/force-refresh", post(jobs::force_refresh_user_cache_handler))
        // OpenAPI specification and Swagger UI
        .route("/api/v1/openapi.json", get(openapi::get_openapi_json))
        .route("/docs", get(openapi::get_swagger_ui))
        .route("/swagger", get(openapi::get_swagger_ui))
        .layer(axum::middleware::from_fn(security_headers_middleware))
        .layer(cors)
        .layer(Extension(state.clone()))
        .with_state(state)
}
