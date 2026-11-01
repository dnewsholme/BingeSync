pub mod admin;
pub mod api_keys;
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
pub mod tmdb;
pub mod tvdb;
pub mod user;
pub mod user_settings;
pub mod user_sync;
pub mod tautulli_sync;
pub mod backup;
pub mod db_maintenance;
pub mod setup;
pub mod webhooks;
pub mod outbound_webhooks;
pub mod overseerr;
pub mod media_writeback;
pub mod ssrf;
pub mod rate_limit;

use axum::{
    routing::{delete, get, post, put},
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
    pub db_backup_notify: Arc<tokio::sync::Notify>,
    pub db_maintenance_notify: Arc<tokio::sync::Notify>,
    pub tautulli_sync_notify: Arc<tokio::sync::Notify>,
    pub media_sync_notify: Arc<tokio::sync::Notify>,
    pub http_client: reqwest::Client,
}

impl AppState {
    pub fn new(pool: SqlitePool, config: Arc<Config>, oidc_client: Option<Arc<OidcClient>>) -> Self {
        Self {
            pool,
            config,
            oidc_client,
            image_notify: Arc::new(tokio::sync::Notify::new()),
            metadata_notify: Arc::new(tokio::sync::Notify::new()),
            user_sync_notify: Arc::new(tokio::sync::Notify::new()),
            log_buffer: Arc::new(logs::LogBuffer::new(500)),
            task_tracker: Arc::new(jobs::BackgroundTaskTracker::new()),
            db_backup_notify: Arc::new(tokio::sync::Notify::new()),
            db_maintenance_notify: Arc::new(tokio::sync::Notify::new()),
            tautulli_sync_notify: Arc::new(tokio::sync::Notify::new()),
            media_sync_notify: Arc::new(tokio::sync::Notify::new()),
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .danger_accept_invalid_certs(true)
                .pool_max_idle_per_host(10)
                .build()
                .unwrap_or_default(),
        }
    }

    pub async fn is_oidc_enabled(&self) -> bool {
        if let Some(val) = settings::get_db_setting(&self.pool, "oidc_enabled").await {
            return val == "true" || val == "1";
        }
        self.config.oidc_enabled
    }

    pub async fn get_effective_oidc_admin_group(&self) -> Option<String> {
        if let Some(val) = settings::get_db_setting(&self.pool, "oidc_admin_group").await {
            if !val.trim().is_empty() {
                return Some(val);
            }
        }
        self.config.oidc_admin_group.clone()
    }

    pub async fn get_effective_oidc_user_group(&self) -> Option<String> {
        if let Some(val) = settings::get_db_setting(&self.pool, "oidc_user_group").await {
            if !val.trim().is_empty() {
                return Some(val);
            }
        }
        self.config.oidc_user_group.clone()
    }

    pub async fn get_effective_oidc_client(&self) -> Option<Arc<OidcClient>> {
        let is_enabled = self.is_oidc_enabled().await;
        if !is_enabled {
            return None;
        }

        let issuer = match settings::get_db_setting(&self.pool, "oidc_issuer_url").await {
            Some(v) if !v.trim().is_empty() => Some(v),
            _ => self.config.oidc_issuer_url.clone(),
        };

        let client_id = match settings::get_db_setting(&self.pool, "oidc_client_id").await {
            Some(v) if !v.trim().is_empty() => Some(v),
            _ => self.config.oidc_client_id.clone(),
        };

        let client_secret = match settings::get_db_setting(&self.pool, "oidc_client_secret").await {
            Some(v) if !v.trim().is_empty() => Some(v),
            _ => self.config.oidc_client_secret.clone(),
        };

        let redirect_uri = match settings::get_db_setting(&self.pool, "oidc_redirect_url").await {
            Some(v) if !v.trim().is_empty() => v,
            _ => self.config.oidc_redirect_url.clone().unwrap_or_else(|| {
                format!("{}/api/v1/auth/callback", self.config.app_url.trim_end_matches('/'))
            }),
        };

        if let (Some(issuer), Some(client_id)) = (issuer, client_id) {
            if let Some(existing) = &self.oidc_client {
                if existing.issuer_url == issuer
                    && existing.client_id == client_id
                    && existing.client_secret == client_secret
                    && existing.redirect_uri == redirect_uri
                {
                    return Some(existing.clone());
                }
            }
            Some(Arc::new(OidcClient::new(
                issuer,
                client_id,
                client_secret,
                redirect_uri,
            )))
        } else {
            None
        }
    }

    pub async fn get_effective_metadata_source(&self) -> String {
        if let Some(src) = settings::get_db_setting(&self.pool, "metadata_source").await {
            let s = src.trim().to_lowercase();
            if s == "tmdb" || s == "themoviedb" {
                return "tmdb".to_string();
            } else if s == "tvdb" || s == "thetvdb" {
                return "tvdb".to_string();
            }
        }
        self.config.metadata_source.clone()
    }

    pub async fn get_effective_tmdb_api_key(&self) -> Option<String> {
        if let Some(key) = settings::get_db_setting(&self.pool, "tmdb_api_key").await {
            if !key.trim().is_empty() {
                return Some(key);
            }
        }
        self.config.tmdb_api_key.clone()
    }

    pub async fn get_effective_tmdb_base_url(&self) -> String {
        if let Some(url) = settings::get_db_setting(&self.pool, "tmdb_base_url").await {
            if !url.trim().is_empty() {
                return url;
            }
        }
        self.config.tmdb_base_url.clone()
    }

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
        let (key, default_hours) = match task_id {
            "image_cache" | "images" => ("task_image_cache_interval_hours", 4),
            "tvdb_metadata" | "tvdb" => ("task_tvdb_metadata_interval_hours", 4),
            "user_sync" | "users" | "plex_users" => ("task_user_sync_interval_hours", 4),
            "db_backup" | "backup" => ("task_db_backup_interval_hours", 24),
            "db_maintenance" | "maintenance" => ("task_db_maintenance_interval_hours", 168),
            "tautulli_sync" | "tautulli" => ("task_tautulli_sync_interval_hours", 1),
            _ => return 4,
        };
        if let Some(val) = settings::get_db_setting(&self.pool, key).await {
            if let Ok(hours) = val.trim().parse::<u64>() {
                if (1..=720).contains(&hours) {
                    return hours;
                }
            }
        }
        default_hours
    }

    pub async fn set_task_interval_hours(&self, task_id: &str, hours: u64) -> Result<(), sqlx::Error> {
        let key = match task_id {
            "image_cache" | "images" => "task_image_cache_interval_hours",
            "tvdb_metadata" | "tvdb" => "task_tvdb_metadata_interval_hours",
            "user_sync" | "users" | "plex_users" => "task_user_sync_interval_hours",
            "db_backup" | "backup" => "task_db_backup_interval_hours",
            "db_maintenance" | "maintenance" => "task_db_maintenance_interval_hours",
            "tautulli_sync" | "tautulli" => "task_tautulli_sync_interval_hours",
            _ => return Ok(()),
        };
        let clamped = hours.clamp(1, 720);
        settings::set_db_setting(&self.pool, key, &clamped.to_string()).await
    }
}

async fn security_headers_middleware(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let is_https = req
        .headers()
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.eq_ignore_ascii_case("https"))
        .unwrap_or(false);

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
    headers.insert(
        axum::http::header::HeaderName::from_static("permissions-policy"),
        axum::http::HeaderValue::from_static("camera=(), microphone=(), geolocation=(), payment=(), usb=()"),
    );
    headers.insert(
        axum::http::header::HeaderName::from_static("cross-origin-opener-policy"),
        axum::http::HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        axum::http::header::HeaderName::from_static("content-security-policy"),
        axum::http::HeaderValue::from_static(
            "default-src 'self'; script-src 'self' 'unsafe-inline' https://cdn.jsdelivr.net https://cdnjs.cloudflare.com https://unpkg.com; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com https://cdn.jsdelivr.net https://cdnjs.cloudflare.com https://unpkg.com; font-src 'self' https://fonts.gstatic.com data:; img-src 'self' data: blob: https: http:; connect-src 'self' https://api.github.com https://cdn.jsdelivr.net https://cdnjs.cloudflare.com https://unpkg.com; object-src 'none'; frame-ancestors 'self'; base-uri 'self';"
        ),
    );

    if is_https {
        headers.insert(
            axum::http::header::HeaderName::from_static("strict-transport-security"),
            axum::http::HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        );
    }

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
        .route("/emby", post(webhooks::handle_emby_webhook_query))
        .route("/emby/{token}", post(webhooks::handle_emby_webhook_path))
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
        // Initial First-Run Setup Flow
        .route("/api/v1/setup/status", get(setup::get_setup_status_handler))
        .route("/api/v1/setup/test-oidc", post(setup::test_oidc_discovery_handler))
        .route("/api/v1/setup/save", post(setup::save_setup_handler))
        // Auth with rate limiting
        .nest("/api/v1/auth", auth_routes)
        // Webhooks with rate limiting
        .nest("/api/v1/webhooks", webhook_routes)
        // Imports with rate limiting
        .nest("/api/v1/import", import_routes)
        // History & Scrobble
        .route("/api/v1/history", get(history::list_history).post(history::create_scrobble))
        .route("/api/v1/history/by-show", delete(history::delete_by_show))
        .route("/api/v1/history/by-media", delete(history::delete_by_media))
        .route("/api/v1/history/delete-batch", post(history::delete_batch))
        .route("/api/v1/history/by-date", delete(history::delete_by_date))
        .route("/api/v1/history/duplicates", get(history::get_duplicates_handler))
        .route("/api/v1/history/duplicates/clean", post(history::clean_duplicates_handler))
        .route("/api/v1/history/{id}/watches", get(history::get_item_watches))
        .route("/api/v1/history/{id}/rate", post(history::rate_scrobble).put(history::rate_scrobble))
        .route("/api/v1/history/{id}/user-data", put(history::update_scrobble_user_data).patch(history::update_scrobble_user_data))
        .route("/api/v1/history/{id}/refresh-metadata", post(history::refresh_scrobble_metadata))
        .route("/api/v1/history/{id}/fix-match", post(history::fix_match_handler).put(history::fix_match_handler))
        .route("/api/v1/history/{id}/match-search", get(history::search_match_handler))
        .route("/api/v1/history/{id}", delete(history::delete_scrobble))
        // Timeline & Stats
        .route("/api/v1/timeline", get(timeline::get_timeline))
        .route("/api/v1/timeline/periods", get(timeline::get_timeline_periods))
        .route("/api/v1/stats", get(stats::get_user_stats))
        .route("/api/v1/stats/wrapped", get(stats::get_user_wrapped))
        .route("/api/v1/now-playing", get(now_playing::get_now_playing))
        // User settings & data export
        .route("/api/v1/user/webhook", get(user::get_webhook_info))
        .route("/api/v1/user/regenerate-token", post(user::regenerate_webhook_token))
        .route("/api/v1/user/export", get(user::export_user_data))
        .route("/api/v1/user/api-keys", get(api_keys::list_api_keys_handler).post(api_keys::create_api_key_handler))
        .route("/api/v1/user/api-keys/{id}", delete(api_keys::delete_api_key_handler))
        .route("/api/v1/user/settings", get(user_settings::get_user_settings_handler).post(user_settings::update_user_settings_handler).put(user_settings::update_user_settings_handler))
        .route("/api/v1/user/settings/test-outbound-webhook", post(outbound_webhooks::test_outbound_webhook_handler))
        .route("/api/v1/user/settings/test-plex", post(media_writeback::test_plex_connection_handler))
        .route("/api/v1/user/settings/test-jellyfin", post(media_writeback::test_jellyfin_connection_handler))
        .route("/api/v1/user/settings/test-emby", post(media_writeback::test_emby_connection_handler))
        .route("/api/v1/user/settings/plex/users", post(media_writeback::fetch_plex_users_post_handler).get(media_writeback::fetch_plex_users_get_handler))
        .route("/api/v1/user/settings/jellyfin/users", post(media_writeback::fetch_jellyfin_users_post_handler).get(media_writeback::fetch_jellyfin_users_get_handler))
        .route("/api/v1/user/settings/emby/users", post(media_writeback::fetch_emby_users_post_handler).get(media_writeback::fetch_emby_users_get_handler))
        .route("/api/v1/user/sync/writeback", post(media_writeback::trigger_writeback_handler))
        // Image cache (serves cached posters from disk)
        .route("/api/v1/images/{filename}", get(images::get_image_handler))
        // Radarr / Sonarr Integrations (Cleanup after watching)
        .route("/api/v1/integrations/status", get(integrations::get_integrations_status))
        .route("/api/v1/integrations/check", get(integrations::check_media_in_library))
        .route("/api/v1/integrations/cleanup", get(integrations::get_cleanup_items))
        .route("/api/v1/integrations/radarr/delete", post(integrations::delete_from_radarr))
        .route("/api/v1/integrations/sonarr/delete-series", post(integrations::delete_from_sonarr))
        .route("/api/v1/integrations/overseerr/status", get(overseerr::get_overseerr_status_handler))
        .route("/api/v1/integrations/overseerr/request", post(overseerr::request_overseerr_media_handler))
        // Admin management: Users, Settings, Logs, and Tasks
        .route("/api/v1/admin/users", get(admin::list_users))
        .route("/api/v1/admin/users/{id}", delete(admin::delete_user))
        .route("/api/v1/admin/settings", get(settings::get_admin_settings_handler).post(settings::update_admin_settings_handler).put(settings::update_admin_settings_handler))
        .route("/api/v1/admin/version", get(settings::get_admin_version_handler))
        .route("/api/v1/admin/version/check", post(settings::get_admin_version_handler))
        .route("/api/v1/admin/system-info", get(settings::get_admin_version_handler))
        .route("/api/v1/admin/logs", get(logs::get_admin_logs_handler).delete(logs::clear_admin_logs_handler))
        .route("/api/v1/admin/tasks", get(jobs::get_admin_tasks_handler))
        .route("/api/v1/admin/tasks/{id}/run", post(jobs::trigger_admin_task_handler))
        .route("/api/v1/admin/tasks/{id}/schedule", post(jobs::update_admin_task_schedule_handler))
        .route("/api/v1/admin/metadata/force-refresh", post(jobs::force_refresh_all_metadata_handler))
        .route("/api/v1/admin/images/force-refresh", post(jobs::force_refresh_all_images_handler))
        .route("/api/v1/admin/users-cache/force-refresh", post(jobs::force_refresh_user_cache_handler))
        // Database Backups & Restore (Admin Only)
        .route("/api/v1/admin/backups", get(backup::get_admin_backups_handler))
        .route("/api/v1/admin/backups/create", post(backup::create_admin_backup_handler))
        .route("/api/v1/admin/backups/restore", post(backup::restore_admin_backup_handler))
        .route("/api/v1/admin/backups/{filename}", delete(backup::delete_admin_backup_handler))
        // OpenAPI specification and Swagger UI
        .route("/api/v1/openapi.json", get(openapi::get_openapi_json))
        .route("/docs", get(openapi::get_swagger_ui))
        .route("/swagger", get(openapi::get_swagger_ui))
        .layer(tower_http::compression::CompressionLayer::new())
        .layer(axum::middleware::from_fn(security_headers_middleware))
        .layer(cors)
        .layer(Extension(state.clone()))
        .with_state(state)
}
