pub mod admin;
pub mod auth;
pub mod history;
pub mod images;
pub mod integrations;
pub mod now_playing;
pub mod stats;
pub mod timeline;
pub mod trakt_import;
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
        // Image cache (serves cached posters from disk)
        .route("/api/v1/images/{filename}", get(images::get_image_handler))
        // Radarr / Sonarr Integrations (Cleanup after watching)
        .route("/api/v1/integrations/status", get(integrations::get_integrations_status))
        .route("/api/v1/integrations/check", get(integrations::check_media_in_library))
        .route("/api/v1/integrations/radarr/delete", post(integrations::delete_from_radarr))
        .route("/api/v1/integrations/sonarr/delete-series", post(integrations::delete_from_sonarr))
        // Admin user management
        .route("/api/v1/admin/users", get(admin::list_users))
        .route("/api/v1/admin/users/{id}", delete(admin::delete_user))
        .layer(cors)
        .layer(Extension(state.clone()))
        .with_state(state)
}
