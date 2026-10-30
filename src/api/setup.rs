use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    api::{settings::{get_db_setting, set_db_setting}, ssrf::validate_integration_url, AppState},
};

#[derive(Debug, Serialize)]
pub struct SetupStatusResponse {
    pub needs_setup: bool,
    pub completed: bool,
    pub user_count: i64,
    pub oidc_configured: bool,
    pub app_url: String,
    pub redirect_url: String,
    pub backchannel_logout_url: String,
    pub oidc_issuer_url: Option<String>,
    pub oidc_client_id: Option<String>,
    pub oidc_admin_group: Option<String>,
    pub oidc_user_group: Option<String>,
    pub dev_login_enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct TestOidcRequest {
    pub issuer_url: String,
}

#[derive(Debug, Serialize)]
pub struct TestOidcResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub userinfo_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwks_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backchannel_logout_supported: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SaveSetupRequest {
    pub oidc_issuer_url: String,
    pub oidc_client_id: String,
    pub oidc_client_secret: Option<String>,
    pub oidc_redirect_url: Option<String>,
    pub oidc_admin_group: Option<String>,
    pub oidc_user_group: Option<String>,
}

pub async fn get_setup_status_handler(
    State(state): State<AppState>,
) -> Result<Json<SetupStatusResponse>, (StatusCode, Json<serde_json::Value>)> {
    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    let setup_completed = get_db_setting(&state.pool, "setup_completed")
        .await
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    let base_app = state.config.app_url.trim_end_matches('/');
    let redirect_url = format!("{}/api/v1/auth/callback", base_app);
    let backchannel_logout_url = format!("{}/api/v1/auth/backchannel-logout", base_app);

    let oidc_client = state.get_effective_oidc_client().await;
    let oidc_configured = oidc_client.is_some();

    // If there are users in the database, setup is already completed
    let needs_setup = user_count == 0 && (!setup_completed || !oidc_configured);

    let oidc_issuer_url = match get_db_setting(&state.pool, "oidc_issuer_url").await {
        Some(v) if !v.trim().is_empty() => Some(v),
        _ => state.config.oidc_issuer_url.clone(),
    };

    let oidc_client_id = match get_db_setting(&state.pool, "oidc_client_id").await {
        Some(v) if !v.trim().is_empty() => Some(v),
        _ => state.config.oidc_client_id.clone(),
    };

    let oidc_admin_group = match get_db_setting(&state.pool, "oidc_admin_group").await {
        Some(v) if !v.trim().is_empty() => Some(v),
        _ => state.config.oidc_admin_group.clone(),
    };

    let oidc_user_group = match get_db_setting(&state.pool, "oidc_user_group").await {
        Some(v) if !v.trim().is_empty() => Some(v),
        _ => state.config.oidc_user_group.clone(),
    };

    Ok(Json(SetupStatusResponse {
        needs_setup,
        completed: !needs_setup,
        user_count,
        oidc_configured,
        app_url: state.config.app_url.clone(),
        redirect_url,
        backchannel_logout_url,
        oidc_issuer_url,
        oidc_client_id,
        oidc_admin_group,
        oidc_user_group,
        dev_login_enabled: state.config.dev_login_enabled,
    }))
}

pub async fn test_oidc_discovery_handler(
    Json(payload): Json<TestOidcRequest>,
) -> impl IntoResponse {
    let trimmed = payload.issuer_url.trim();
    if trimmed.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(TestOidcResponse {
                success: false,
                issuer: None,
                authorization_endpoint: None,
                token_endpoint: None,
                userinfo_endpoint: None,
                jwks_uri: None,
                backchannel_logout_supported: None,
                error: Some("Issuer URL cannot be empty".to_string()),
            }),
        );
    }

    if let Err(e) = validate_integration_url(trimmed) {
        return (
            StatusCode::BAD_REQUEST,
            Json(TestOidcResponse {
                success: false,
                issuer: None,
                authorization_endpoint: None,
                token_endpoint: None,
                userinfo_endpoint: None,
                jwks_uri: None,
                backchannel_logout_supported: None,
                error: Some(format!("Invalid Issuer URL: {}", e)),
            }),
        );
    }

    let well_known = format!("{}/.well-known/openid-configuration", trimmed.trim_end_matches('/'));

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(TestOidcResponse {
                    success: false,
                    issuer: None,
                    authorization_endpoint: None,
                    token_endpoint: None,
                    userinfo_endpoint: None,
                    jwks_uri: None,
                    backchannel_logout_supported: None,
                    error: Some(format!("Failed to build HTTP client: {}", e)),
                }),
            );
        }
    };

    let res = match client.get(&well_known).send().await {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(TestOidcResponse {
                    success: false,
                    issuer: None,
                    authorization_endpoint: None,
                    token_endpoint: None,
                    userinfo_endpoint: None,
                    jwks_uri: None,
                    backchannel_logout_supported: None,
                    error: Some(format!("Failed to connect to {}: {}", well_known, e)),
                }),
            );
        }
    };

    if !res.status().is_success() {
        return (
            StatusCode::BAD_GATEWAY,
            Json(TestOidcResponse {
                success: false,
                issuer: None,
                authorization_endpoint: None,
                token_endpoint: None,
                userinfo_endpoint: None,
                jwks_uri: None,
                backchannel_logout_supported: None,
                error: Some(format!("Discovery endpoint returned HTTP status {}", res.status())),
            }),
        );
    }

    let disc_value: serde_json::Value = match res.json().await {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(TestOidcResponse {
                    success: false,
                    issuer: None,
                    authorization_endpoint: None,
                    token_endpoint: None,
                    userinfo_endpoint: None,
                    jwks_uri: None,
                    backchannel_logout_supported: None,
                    error: Some(format!("Failed to parse discovery JSON from {}: {}", well_known, e)),
                }),
            );
        }
    };

    let issuer = disc_value.get("issuer").and_then(|v| v.as_str()).map(|s| s.to_string());
    let authorization_endpoint = disc_value.get("authorization_endpoint").and_then(|v| v.as_str()).map(|s| s.to_string());
    let token_endpoint = disc_value.get("token_endpoint").and_then(|v| v.as_str()).map(|s| s.to_string());
    let userinfo_endpoint = disc_value.get("userinfo_endpoint").and_then(|v| v.as_str()).map(|s| s.to_string());
    let jwks_uri = disc_value.get("jwks_uri").and_then(|v| v.as_str()).map(|s| s.to_string());
    let backchannel_logout_supported = disc_value.get("backchannel_logout_supported").and_then(|v| v.as_bool());

    if authorization_endpoint.is_none() || token_endpoint.is_none() {
        return (
            StatusCode::BAD_GATEWAY,
            Json(TestOidcResponse {
                success: false,
                issuer,
                authorization_endpoint,
                token_endpoint,
                userinfo_endpoint,
                jwks_uri,
                backchannel_logout_supported,
                error: Some("Discovery document missing authorization_endpoint or token_endpoint".to_string()),
            }),
        );
    }

    (
        StatusCode::OK,
        Json(TestOidcResponse {
            success: true,
            issuer,
            authorization_endpoint,
            token_endpoint,
            userinfo_endpoint,
            jwks_uri,
            backchannel_logout_supported,
            error: None,
        }),
    )
}

pub async fn save_setup_handler(
    State(state): State<AppState>,
    Json(payload): Json<SaveSetupRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    let setup_completed = get_db_setting(&state.pool, "setup_completed")
        .await
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    // If users already exist and setup was completed, forbid modifications via setup endpoint
    if user_count > 0 && setup_completed {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "Initial setup has already been completed. Further changes must be made in Admin Settings."
            })),
        ));
    }

    let issuer = payload.oidc_issuer_url.trim();
    if issuer.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "OIDC Issuer URL is required" })),
        ));
    }

    let client_id = payload.oidc_client_id.trim();
    if client_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "OIDC Client ID is required" })),
        ));
    }

    if let Err(e) = validate_integration_url(issuer) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("Invalid OIDC Issuer URL: {}", e) })),
        ));
    }

    // Persist settings in SQLite
    let pool = &state.pool;
    set_db_setting(pool, "oidc_enabled", "true")
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))))?;
    set_db_setting(pool, "oidc_issuer_url", issuer)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))))?;
    set_db_setting(pool, "oidc_client_id", client_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))))?;

    if let Some(secret) = &payload.oidc_client_secret {
        let trimmed = secret.trim();
        if !trimmed.is_empty() {
            set_db_setting(pool, "oidc_client_secret", trimmed)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))))?;
        }
    }

    if let Some(redirect) = &payload.oidc_redirect_url {
        let trimmed = redirect.trim();
        if !trimmed.is_empty() {
            set_db_setting(pool, "oidc_redirect_url", trimmed)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))))?;
        }
    }

    if let Some(group) = &payload.oidc_admin_group {
        let trimmed = group.trim();
        if !trimmed.is_empty() {
            set_db_setting(pool, "oidc_admin_group", trimmed)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))))?;
        }
    }

    if let Some(group) = &payload.oidc_user_group {
        let trimmed = group.trim();
        if !trimmed.is_empty() {
            set_db_setting(pool, "oidc_user_group", trimmed)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))))?;
        }
    }

    set_db_setting(pool, "setup_completed", "true")
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))))?;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Initial OIDC setup completed successfully! You can now log in.",
        "login_url": "/api/v1/auth/login"
    })))
}

