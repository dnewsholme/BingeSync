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

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: String,
    pub email: String,
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

        // 1. Try Authorization header: Bearer <token>
        let mut token: Option<String> = None;
        if let Some(auth_header) = parts.headers.get(header::AUTHORIZATION) {
            if let Ok(auth_str) = auth_header.to_str() {
                if let Some(bearer) = auth_str.strip_prefix("Bearer ") {
                    token = Some(bearer.trim().to_string());
                }
            }
        }

        // 2. Try Cookie if header not found
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
                    Json(serde_json::json!({ "error": "Authentication required" })),
                )
                    .into_response());
            }
        };

        match verify_jwt(&token, &app_state.config.jwt_secret) {
            Ok(claims) => Ok(AuthUser {
                user_id: claims.sub,
                email: claims.email,
            }),
            Err(_) => Err((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "Invalid or expired authentication token" })),
            )
                .into_response()),
        }
    }
}
