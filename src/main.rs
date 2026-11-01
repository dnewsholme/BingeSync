use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use bingesync::{
    api::{create_api_router, AppState},
    assets,
    auth::OidcClient,
    config::Config,
    db,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. In-memory logging buffer and background task tracker
    let log_buffer = Arc::new(bingesync::api::logs::LogBuffer::new(5000));
    let task_tracker = Arc::new(bingesync::api::jobs::BackgroundTaskTracker::new());

    // 2. Logging setup with in-memory buffer capture
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "bingesync=debug,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .with(bingesync::api::logs::TracingLogLayer::new(log_buffer.clone()))
        .init();

    // 3. Load config
    let config = Arc::new(Config::from_env());
    tracing::info!(
        app_url = %config.app_url,
        database_url = %config.database_url,
        oidc_enabled = config.oidc_enabled,
        dev_login_enabled = config.dev_login_enabled,
        "Starting BingeSync..."
    );

    // 4. Initialize SQLite DB
    let pool = db::init_db(&config.database_url).await?;
    tracing::info!("SQLite database initialized and migrations applied successfully");

    // 5. Initialize OIDC client if enabled
    let oidc_client = if config.oidc_enabled {
        if let (Some(issuer), Some(client_id)) = (&config.oidc_issuer_url, &config.oidc_client_id) {
            let redirect_uri = config
                .oidc_redirect_url
                .clone()
                .unwrap_or_else(|| format!("{}/api/v1/auth/callback", config.app_url.trim_end_matches('/')));

            tracing::info!(
                issuer = %issuer,
                client_id = %client_id,
                redirect_uri = %redirect_uri,
                "Configuring OIDC provider client"
            );

            Some(Arc::new(OidcClient::new(
                issuer.clone(),
                client_id.clone(),
                config.oidc_client_secret.clone(),
                redirect_uri,
            )))
        } else {
            tracing::warn!("OIDC is enabled but OIDC_ISSUER_URL or OIDC_CLIENT_ID is missing");
            None
        }
    } else {
        None
    };

    // 6. Build application state
    let state = AppState {
        pool,
        config: config.clone(),
        oidc_client,
        image_notify: Arc::new(tokio::sync::Notify::new()),
        metadata_notify: Arc::new(tokio::sync::Notify::new()),
        user_sync_notify: Arc::new(tokio::sync::Notify::new()),
        log_buffer,
        task_tracker,
        db_backup_notify: Arc::new(tokio::sync::Notify::new()),
        db_maintenance_notify: Arc::new(tokio::sync::Notify::new()),
        tautulli_sync_notify: Arc::new(tokio::sync::Notify::new()),
        media_sync_notify: Arc::new(tokio::sync::Notify::new()),
        session_reaper_notify: Arc::new(tokio::sync::Notify::new()),
        image_prune_notify: Arc::new(tokio::sync::Notify::new()),
        duplicate_pruner_notify: Arc::new(tokio::sync::Notify::new()),
        metadata_backfill_notify: Arc::new(tokio::sync::Notify::new()),
        version_check_notify: Arc::new(tokio::sync::Notify::new()),
        library_audit_notify: Arc::new(tokio::sync::Notify::new()),
        http_client: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .danger_accept_invalid_certs(true)
            .pool_max_idle_per_host(10)
            .build()
            .unwrap_or_default(),
    };

    // 6. Start background workers
    let _image_worker = bingesync::api::images::start_image_cache_worker(state.clone());
    let _metadata_worker = bingesync::api::tvdb::start_metadata_worker(state.clone());
    let _user_sync_worker = bingesync::api::user_sync::start_user_cache_worker(state.clone());
    let _db_backup_worker = bingesync::api::backup::start_db_backup_worker(state.clone());
    let _db_maintenance_worker = bingesync::api::db_maintenance::start_db_maintenance_worker(state.clone());
    let _tautulli_sync_worker = bingesync::api::tautulli_sync::start_tautulli_sync_worker(state.clone());
    let _media_sync_worker = bingesync::api::media_writeback::start_media_sync_worker(state.clone());
    let _session_reaper_worker = bingesync::api::maintenance_tasks::start_session_reaper_worker(state.clone());
    let _image_prune_worker = bingesync::api::maintenance_tasks::start_image_prune_worker(state.clone());
    let _duplicate_pruner_worker = bingesync::api::maintenance_tasks::start_duplicate_pruner_worker(state.clone());
    let _metadata_backfill_worker = bingesync::api::maintenance_tasks::start_metadata_backfill_worker(state.clone());
    let _version_check_worker = bingesync::api::maintenance_tasks::start_version_check_worker(state.clone());
    let _library_audit_worker = bingesync::api::maintenance_tasks::start_library_audit_worker(state.clone());

    // 7. Router assembly
    let app = create_api_router(state)
        .fallback(assets::static_handler);

    // Support configurable reverse proxy subpath (e.g. /bingesync)
    let app = if !config.base_path.is_empty() {
        let base = config.base_path.trim_end_matches('/').to_string();
        let redirect_target = format!("{}/", base);
        tracing::info!(base_path = %base, "Configuring reverse proxy subpath mount with trailing slash redirect");
        axum::Router::new()
            .route(
                &base,
                axum::routing::get({
                    let redirect_target = redirect_target.clone();
                    move || async move { axum::response::Redirect::permanent(&redirect_target) }
                }),
            )
            .nest(&base, app.clone())
            .merge(app)
    } else {
        app
    }
    .layer(tower_http::compression::CompressionLayer::new());

    // 7. Start server
    let bind_addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!("BingeSync server running on http://{}", bind_addr);

    axum::serve(listener, app).await?;

    Ok(())
}
