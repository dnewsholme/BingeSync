use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
    Json,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    api::AppState,
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
}

pub async fn get_auth_status(State(state): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({
        "oidc_enabled": state.config.oidc_enabled,
        "dev_login_enabled": state.config.dev_login_enabled,
        "app_url": state.config.app_url,
    }))
}

pub async fn login_handler(State(state): State<AppState>) -> Result<Redirect, (StatusCode, Json<serde_json::Value>)> {
    if !state.config.oidc_enabled {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "OIDC is not enabled on this instance. Use dev-login or configure OIDC environment variables." })),
        ));
    }

    let oidc = state.oidc_client.as_ref().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "OIDC client is not initialized" })),
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

    let oidc = state.oidc_client.as_ref().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "OIDC client is not initialized" })),
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

    let display_name = user_info
        .name
        .or(user_info.preferred_username)
        .unwrap_or_else(|| email.clone());

    let user = get_or_create_user(
        &state.pool,
        &email,
        &display_name,
        Some(&user_info.sub),
        user_info.picture.as_deref(),
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

    Ok((jar.add(cookie), Redirect::temporary("/")))
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
    });

    let email = req
        .email
        .unwrap_or_else(|| "admin@bingesync.local".to_string());
    let display_name = req
        .display_name
        .unwrap_or_else(|| "Demo User".to_string());

    let user = get_or_create_user(&state.pool, &email, &display_name, None, None)
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
) -> Result<User, sqlx::Error> {
    // 1. Try finding by oidc_sub if provided
    if let Some(sub) = oidc_sub {
        if let Some(user) = sqlx::query_as::<_, User>("SELECT * FROM users WHERE oidc_sub = ?")
            .bind(sub)
            .fetch_optional(pool)
            .await?
        {
            return Ok(user);
        }
    }

    // 2. Try finding by email
    if let Some(user) = sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = ?")
        .bind(email)
        .fetch_optional(pool)
        .await?
    {
        // Update oidc_sub if missing
        if oidc_sub.is_some() && user.oidc_sub.is_none() {
            sqlx::query("UPDATE users SET oidc_sub = ?, updated_at = datetime('now') WHERE id = ?")
                .bind(oidc_sub)
                .bind(&user.id)
                .execute(pool)
                .await?;
        }
        return Ok(user);
    }

    // 3. Create new user with unique webhook_token
    let user_id = Uuid::new_v4().to_string();
    let webhook_token = Uuid::new_v4().to_string().replace('-', "");
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        r#"
        INSERT INTO users (id, email, display_name, oidc_sub, webhook_token, avatar_url, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&user_id)
    .bind(email)
    .bind(display_name)
    .bind(oidc_sub)
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
