use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Redirect},
    Json,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::Utc;
use serde::Deserialize;
use url::form_urlencoded;
use uuid::Uuid;

use crate::{
    api::{settings::get_db_setting, AppState},
    auth::{create_jwt, AuthUser},
    models::User,
};

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DevLoginRequest {
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub role: Option<String>,
}

pub async fn get_auth_status(State(state): State<AppState>) -> impl IntoResponse {
    let base_app = state.config.app_url.trim_end_matches('/');
    let backchannel_logout_url = format!("{}/api/v1/auth/backchannel-logout", base_app);
    let oidc_enabled = state.is_oidc_enabled().await;

    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    let setup_completed = get_db_setting(&state.pool, "setup_completed")
        .await
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    let oidc_client = state.get_effective_oidc_client().await;
    let oidc_configured = oidc_client.is_some();

    let needs_setup = user_count == 0 && (!setup_completed || !oidc_configured);

    Json(serde_json::json!({
        "oidc_enabled": oidc_enabled,
        "oidc_configured": oidc_configured,
        "dev_login_enabled": state.config.dev_login_enabled,
        "app_url": state.config.app_url,
        "backchannel_logout_url": backchannel_logout_url,
        "needs_setup": needs_setup,
        "user_count": user_count,
    }))
}

pub async fn login_handler(State(state): State<AppState>) -> Result<Redirect, (StatusCode, Json<serde_json::Value>)> {
    let oidc = state.get_effective_oidc_client().await.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "OpenID Connect is not enabled or configured on this instance. Please complete the setup flow." })),
        )
    })?;

    let auth_url = oidc
        .generate_auth_url(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to generate auth URL: {}", e) })),
            )
        })?;

    Ok(Redirect::temporary(&auth_url))
}

pub async fn callback_handler(
    State(state): State<AppState>,
    Query(query): Query<CallbackQuery>,
    jar: CookieJar,
) -> Result<(CookieJar, Redirect), (StatusCode, Json<serde_json::Value>)> {
    if let Some(err) = query.error {
        let desc = query.error_description.unwrap_or_default();
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("OIDC provider returned error: {} ({})", err, desc) })),
        ));
    }

    let code = query.code.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Missing 'code' query parameter" })),
        )
    })?;

    let state_param = query.state.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Missing 'state' query parameter" })),
        )
    })?;

    let oidc = state.get_effective_oidc_client().await.ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "OIDC client is not initialized or configured" })),
        )
    })?;

    let user_info = oidc
        .handle_callback(&state.pool, &code, &state_param)
        .await
        .map_err(|e| {
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": format!("OIDC authentication failed: {}", e) })),
            )
        })?;

    let email = user_info
        .email
        .clone()
        .unwrap_or_else(|| format!("{}@oidc.local", user_info.sub));

    let custom_admin_group = state.get_effective_oidc_admin_group().await;
    let is_admin = user_info.is_admin(custom_admin_group.as_deref());

    let custom_user_group = state.get_effective_oidc_user_group().await;
    if !is_admin {
        if let Some(ref req_group) = custom_user_group.filter(|g| !g.trim().is_empty()) {
            if !user_info.has_group(Some(req_group)) {
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({
                        "error": format!("Access denied: user is not a member of the required group '{}'", req_group)
                    })),
                ));
            }
        }
    }

    let display_name = user_info
        .name
        .clone()
        .or_else(|| user_info.preferred_username.clone())
        .unwrap_or_else(|| email.clone());

    let user = get_or_create_user_with_sid(
        &state.pool,
        &email,
        &display_name,
        Some(&user_info.sub),
        user_info.sid.as_deref(),
        user_info.picture.as_deref(),
        user_info.preferred_username.as_deref(),
        is_admin,
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Database error creating user: {}", e) })),
        )
    })?;

    let token = create_jwt(&user.id, &user.email, &state.config.jwt_secret).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Token creation failed: {}", e) })),
        )
    })?;

    let cookie = Cookie::build(("bingesync_token", token))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::days(30))
        .build();

    let redirect_url = if !state.config.base_path.is_empty() {
        format!("{}/", state.config.base_path.trim_end_matches('/'))
    } else {
        "/".to_string()
    };

    Ok((jar.add(cookie), Redirect::temporary(&redirect_url)))
}

pub async fn dev_login_handler(
    State(state): State<AppState>,
    jar: CookieJar,
    body: Option<Json<DevLoginRequest>>,
) -> Result<(CookieJar, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    if !state.config.dev_login_enabled {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Dev login is disabled" })),
        ));
    }

    let req = body.map(|b| b.0).unwrap_or(DevLoginRequest {
        email: None,
        display_name: None,
        role: None,
    });

    let email = req
        .email
        .unwrap_or_else(|| "admin@bingesync.local".to_string());
    let display_name = req
        .display_name
        .unwrap_or_else(|| "Demo User".to_string());

    let is_admin = if let Some(ref r) = req.role {
        r.to_lowercase() == "admin"
    } else {
        email.to_lowercase().contains("admin")
    };

    let user = get_or_create_user(
        &state.pool,
        &email,
        &display_name,
        None,
        None,
        is_admin,
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Database error: {}", e) })),
        )
    })?;

    let token = create_jwt(&user.id, &user.email, &state.config.jwt_secret).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Token creation failed: {}", e) })),
        )
    })?;

    let cookie = Cookie::build(("bingesync_token", token.clone()))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::days(30))
        .build();

    let user_resp = user.to_response(&state.config.app_url);

    Ok((
        jar.add(cookie),
        Json(serde_json::json!({
            "token": token,
            "user": user_resp,
        })),
    ))
}

pub async fn get_me_handler(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<crate::models::UserResponse>, (StatusCode, Json<serde_json::Value>)> {
    let user: User = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
        .bind(&auth.user_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Database error: {}", e) })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "User not found" })),
            )
        })?;

    Ok(Json(user.to_response(&state.config.app_url)))
}

pub async fn logout_handler(jar: CookieJar) -> (CookieJar, Json<serde_json::Value>) {
    let removal_cookie = Cookie::build(("bingesync_token", ""))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::seconds(0))
        .build();

    (jar.add(removal_cookie), Json(serde_json::json!({ "status": "logged_out" })))
}

pub async fn get_or_create_user(
    pool: &sqlx::SqlitePool,
    email: &str,
    display_name: &str,
    oidc_sub: Option<&str>,
    avatar_url: Option<&str>,
    is_admin: bool,
) -> Result<User, sqlx::Error> {
    let pref = email.split('@').next();
    get_or_create_user_with_sid(pool, email, display_name, oidc_sub, None, avatar_url, pref, is_admin).await
}

pub async fn get_or_create_user_with_sid(
    pool: &sqlx::SqlitePool,
    email: &str,
    display_name: &str,
    oidc_sub: Option<&str>,
    oidc_sid: Option<&str>,
    avatar_url: Option<&str>,
    preferred_username: Option<&str>,
    is_admin: bool,
) -> Result<User, sqlx::Error> {
    let existing_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    let is_admin = is_admin || existing_count == 0;
    let desired_role = if is_admin { "admin" } else { "user" };

    // 1. Try finding by oidc_sub if provided
    if let Some(sub) = oidc_sub {
        if let Some(user) = sqlx::query_as::<_, User>("SELECT * FROM users WHERE oidc_sub = ?")
            .bind(sub)
            .fetch_optional(pool)
            .await?
        {
            // Sync role, oidc_sid, preferred_username, and reset sessions_revoked_at on fresh login
            sqlx::query(
                "UPDATE users SET role = ?, oidc_sid = COALESCE(?, oidc_sid), preferred_username = COALESCE(?, preferred_username), sessions_revoked_at = NULL, updated_at = datetime('now') WHERE id = ?"
            )
            .bind(desired_role)
            .bind(oidc_sid)
            .bind(preferred_username)
            .bind(&user.id)
            .execute(pool)
            .await?;

            return sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
                .bind(&user.id)
                .fetch_one(pool)
                .await;
        }
    }

    // 2. Try finding by email
    if let Some(user) = sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = ?")
        .bind(email)
        .fetch_optional(pool)
        .await?
    {
        let role_to_set = if is_admin { "admin" } else { &user.role };
        sqlx::query(
            "UPDATE users SET oidc_sub = COALESCE(oidc_sub, ?), oidc_sid = COALESCE(?, oidc_sid), preferred_username = COALESCE(?, preferred_username), role = ?, sessions_revoked_at = NULL, updated_at = datetime('now') WHERE id = ?"
        )
        .bind(oidc_sub)
        .bind(oidc_sid)
        .bind(preferred_username)
        .bind(role_to_set)
        .bind(&user.id)
        .execute(pool)
        .await?;

        return sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
            .bind(&user.id)
            .fetch_one(pool)
            .await;
    }

    // 3. Create new user with unique webhook_token, preferred_username, and role
    let user_id = Uuid::new_v4().to_string();
    let webhook_token = Uuid::new_v4().to_string().replace('-', "");
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        r#"
        INSERT INTO users (id, email, display_name, role, oidc_sub, oidc_sid, preferred_username, webhook_token, avatar_url, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&user_id)
    .bind(email)
    .bind(display_name)
    .bind(desired_role)
    .bind(oidc_sub)
    .bind(oidc_sid)
    .bind(preferred_username)
    .bind(&webhook_token)
    .bind(avatar_url)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;

    let user: User = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
        .bind(&user_id)
        .fetch_one(pool)
        .await?;

    Ok(user)
}

pub async fn backchannel_logout_handler(
    State(state): State<AppState>,
    _headers: HeaderMap,
    body: axum::body::Bytes,
) -> (StatusCode, [(header::HeaderName, &'static str); 2], Json<serde_json::Value>) {
    let response_headers = [
        (header::CACHE_CONTROL, "no-cache, no-store"),
        (header::PRAGMA, "no-cache"),
    ];

    let oidc = match state.get_effective_oidc_client().await {
        Some(client) => client,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                response_headers,
                Json(serde_json::json!({
                    "error": "invalid_request",
                    "error_description": "OIDC is disabled or not configured on this instance"
                })),
            );
        }
    };

    // Extract logout_token parameter
    let mut logout_token: Option<String> = None;

    // Check application/x-www-form-urlencoded
    for (k, v) in form_urlencoded::parse(&body) {
        if k == "logout_token" {
            logout_token = Some(v.into_owned());
            break;
        }
    }

    // Check application/json
    if logout_token.is_none() {
        if let Ok(json_body) = serde_json::from_slice::<serde_json::Value>(&body) {
            if let Some(tok) = json_body.get("logout_token").and_then(|v| v.as_str()) {
                logout_token = Some(tok.to_string());
            }
        }
    }

    // Fallback: raw token in body
    if logout_token.is_none() {
        let text = String::from_utf8_lossy(&body).trim().to_string();
        if text.starts_with("eyJ") {
            logout_token = Some(text);
        }
    }

    let token = match logout_token {
        Some(t) if !t.trim().is_empty() => t.trim().to_string(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                response_headers,
                Json(serde_json::json!({
                    "error": "invalid_request",
                    "error_description": "Missing 'logout_token' parameter in request"
                })),
            );
        }
    };

    let claims = match oidc.validate_logout_token(&token).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "Invalid OIDC backchannel logout token received");
            return (
                StatusCode::BAD_REQUEST,
                response_headers,
                Json(serde_json::json!({
                    "error": "invalid_request",
                    "error_description": e
                })),
            );
        }
    };

    let now = Utc::now().timestamp();
    let mut revoked_count = 0;

    // Invalidate sessions matching sub
    if let Some(ref sub) = claims.sub {
        let users: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, email FROM users WHERE oidc_sub = ? OR email = ? OR id = ?"
        )
        .bind(sub)
        .bind(sub)
        .bind(sub)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

        for (user_id, email) in users {
            revoked_count += 1;
            let _ = sqlx::query("DELETE FROM active_sessions WHERE user_id = ?")
                .bind(&user_id)
                .execute(&state.pool)
                .await;

            let _ = sqlx::query("UPDATE users SET sessions_revoked_at = ?, updated_at = datetime('now') WHERE id = ?")
                .bind(now)
                .bind(&user_id)
                .execute(&state.pool)
                .await;

            tracing::info!(user_id = %user_id, email = %email, oidc_sub = %sub, "OIDC backchannel logout: user access revoked and sessions terminated");
        }
    }

    // Invalidate sessions matching sid
    if let Some(ref sid) = claims.sid {
        let users: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, email FROM users WHERE oidc_sid = ?"
        )
        .bind(sid)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

        for (user_id, email) in users {
            revoked_count += 1;
            let _ = sqlx::query("DELETE FROM active_sessions WHERE user_id = ?")
                .bind(&user_id)
                .execute(&state.pool)
                .await;

            let _ = sqlx::query("UPDATE users SET sessions_revoked_at = ?, updated_at = datetime('now') WHERE id = ?")
                .bind(now)
                .bind(&user_id)
                .execute(&state.pool)
                .await;

            tracing::info!(user_id = %user_id, email = %email, oidc_sid = %sid, "OIDC backchannel logout: session terminated by sid");
        }
    }

    // Per OIDC Back-Channel Logout 1.0 section 2.7:
    // "If the Logout Token is valid, the RP MUST respond with HTTP 200 OK and Cache-Control: no-cache, no-store, Pragma: no-cache."
    (
        StatusCode::OK,
        response_headers,
        Json(serde_json::json!({
            "status": "ok",
            "revoked": revoked_count > 0,
            "revoked_count": revoked_count
        })),
    )
}
