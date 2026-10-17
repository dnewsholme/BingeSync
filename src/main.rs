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
    // 1. Logging setup
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "bingesync=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // 2. Load config
    let config = Arc::new(Config::from_env());
    tracing::info!(
        app_url = %config.app_url,
        database_url = %config.database_url,
        oidc_enabled = config.oidc_enabled,
        dev_login_enabled = config.dev_login_enabled,
        "Starting BingeSync..."
    );

    // 3. Initialize SQLite DB
    let pool = db::init_db(&config.database_url).await?;
    tracing::info!("SQLite database initialized and migrations applied successfully");

    // 4. Initialize OIDC client if enabled
    let oidc_client = if config.oidc_enabled {
        if let (Some(issuer), Some(client_id)) = (&config.oidc_issuer_url, &config.oidc_client_id) {
            let redirect_uri = config
                .oidc_redirect_url
                .clone()
                .unwrap_or_else(|| format!("{}/api/v1/auth/callback", config.app_url));

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

    // 5. Build application state
    let state = AppState {
        pool,
        config: config.clone(),
        oidc_client,
    };

    // 6. Router assembly
    let app = create_api_router(state)
        .fallback(assets::static_handler);

    // Support configurable reverse proxy subpath (e.g. /bingesync)
    let app = if !config.base_path.is_empty() {
        tracing::info!(base_path = %config.base_path, "Configuring reverse proxy subpath mount");
        axum::Router::new()
            .nest(&config.base_path, app.clone())
            .merge(app)
    } else {
        app
    };

    // 7. Start server
    let bind_addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!("BingeSync server running on http://{}", bind_addr);

    axum::serve(listener, app).await?;

    Ok(())
}
