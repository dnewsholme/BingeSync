use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use axum_extra::extract::cookie::CookieJar;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    pub email: String,
    pub exp: usize,
    pub iat: usize,
}

pub fn create_jwt(user_id: &str, email: &str, secret: &str) -> Result<String, jsonwebtoken::errors::Error> {
    let now = chrono::Utc::now().timestamp() as usize;
    let claims = Claims {
        sub: user_id.to_string(),
        email: email.to_string(),
        iat: now,
        exp: now + 30 * 24 * 3600, // 30 days
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
}

pub fn verify_jwt(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let mut validation = Validation::default();
    validation.validate_exp = true;
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )?;
    Ok(token_data.claims)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthUser {
    pub user_id: String,
    pub email: String,
    pub role: String,
}

pub async fn validate_api_key(
    pool: &sqlx::SqlitePool,
    api_key: &str,
) -> Option<AuthUser> {
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return None;
    }

    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(trimmed.as_bytes());
    let key_hash = format!("{:x}", hasher.finalize());

    let row: Option<(String, String, String, String)> = sqlx::query_as(
        r#"
        SELECT u.id, u.email, u.role, k.id as key_id
        FROM user_api_keys k
        JOIN users u ON u.id = k.user_id
        WHERE k.key_hash = ?
        "#
    )
    .bind(&key_hash)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    if let Some((user_id, email, role, key_id)) = row {
        let pool_clone = pool.clone();
        tokio::spawn(async move {
            let _ = sqlx::query("UPDATE user_api_keys SET last_used_at = datetime('now') WHERE id = ?")
                .bind(&key_id)
                .execute(&pool_clone)
                .await;
        });

        Some(AuthUser {
            user_id,
            email,
            role,
        })
    } else {
        None
    }
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = parts
            .extensions
            .get::<crate::api::AppState>()
            .cloned()
            .ok_or_else(|| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "AppState missing from request extensions" })),
                )
                    .into_response()
            })?;

        // 1. Try X-Api-Key header
        if let Some(key_header) = parts.headers.get("x-api-key") {
            if let Ok(key_str) = key_header.to_str() {
                if let Some(auth_user) = validate_api_key(&app_state.pool, key_str).await {
                    return Ok(auth_user);
                }
            }
        }

        // 2. Try Authorization header: Bearer <token> or ApiKey <token>
        let mut token: Option<String> = None;
        if let Some(auth_header) = parts.headers.get(header::AUTHORIZATION) {
            if let Ok(auth_str) = auth_header.to_str() {
                if let Some(bearer) = auth_str.strip_prefix("Bearer ") {
                    let trimmed = bearer.trim();
                    if trimmed.starts_with("bs_") {
                        if let Some(auth_user) = validate_api_key(&app_state.pool, trimmed).await {
                            return Ok(auth_user);
                        }
                    }
                    token = Some(trimmed.to_string());
                } else if let Some(api_key) = auth_str.strip_prefix("ApiKey ") {
                    if let Some(auth_user) = validate_api_key(&app_state.pool, api_key).await {
                        return Ok(auth_user);
                    }
                }
            }
        }

        // 3. Try query parameter ?api_key=<key>
        if let Some(query) = parts.uri.query() {
            for (key, val) in url::form_urlencoded::parse(query.as_bytes()) {
                if key.eq_ignore_ascii_case("api_key") {
                    if let Some(auth_user) = validate_api_key(&app_state.pool, &val).await {
                        return Ok(auth_user);
                    }
                }
            }
        }

        // 4. Try Cookie if header not found
        if token.is_none() {
            let jar = CookieJar::from_request_parts(parts, state)
                .await
                .map_err(|e| e.into_response())?;
            if let Some(cookie) = jar.get("bingesync_token") {
                token = Some(cookie.value().to_string());
            }
        }

        let token = match token {
            Some(t) => t,
            None => {
                return Err((
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({ "error": "Authentication required. Please sign in or provide a valid API key." })),
                )
                    .into_response());
            }
        };

        let claims = match verify_jwt(&token, &app_state.config.jwt_secret) {
            Ok(c) => c,
            Err(_) => {
                // If JWT verification fails, check if the token was a valid API key passed as Bearer
                if let Some(auth_user) = validate_api_key(&app_state.pool, &token).await {
                    return Ok(auth_user);
                }
                return Err((
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({ "error": "Invalid or expired authentication token" })),
                )
                    .into_response());
            }
        };

        // Query user from DB to verify user still exists and session has not been revoked
        let user_row: Option<(String, String, String, Option<i64>)> = sqlx::query_as(
            "SELECT id, email, role, sessions_revoked_at FROM users WHERE id = ?"
        )
        .bind(&claims.sub)
        .fetch_optional(&app_state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Database error validating user: {}", e) })),
            )
                .into_response()
        })?;

        let (user_id, email, role, sessions_revoked_at) = match user_row {
            Some(row) => row,
            None => {
                return Err((
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({ "error": "User account no longer exists or was deleted" })),
                )
                    .into_response());
            }
        };

        if let Some(revoked_at) = sessions_revoked_at {
            if (claims.iat as i64) <= revoked_at {
                return Err((
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({ "error": "User session has been revoked or logged out" })),
                )
                    .into_response());
            }
        }

        Ok(AuthUser {
            user_id,
            email,
            role,
        })
    }
}
