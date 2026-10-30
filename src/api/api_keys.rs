use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Utc;
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    api::AppState,
    auth::AuthUser,
    models::{CreateApiKeyResponse, UserApiKey},
};

#[derive(Debug, Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: Option<String>,
}

pub async fn list_api_keys_handler(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<UserApiKey>>, (StatusCode, Json<serde_json::Value>)> {
    let keys = sqlx::query_as::<_, UserApiKey>(
        "SELECT id, user_id, name, key_prefix, created_at, last_used_at FROM user_api_keys WHERE user_id = ? ORDER BY created_at DESC"
    )
    .bind(&auth.user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Database error fetching API keys: {}", e) })),
        )
    })?;

    Ok(Json(keys))
}

pub async fn create_api_key_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(payload): Json<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<CreateApiKeyResponse>), (StatusCode, Json<serde_json::Value>)> {
    let raw_name = payload.name.unwrap_or_default();
    let name = raw_name.trim();
    let final_name = if name.is_empty() {
        "Default API Key".to_string()
    } else {
        name.to_string()
    };

    // Generate 32 bytes of cryptographic randomness
    let mut rand_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut rand_bytes);
    let secret_str = URL_SAFE_NO_PAD.encode(rand_bytes);
    let full_api_key = format!("bs_live_{}", secret_str);

    // Compute key prefix for UI masking (e.g. bs_live_a1b2...)
    let key_prefix = if full_api_key.len() >= 14 {
        format!("{}...", &full_api_key[..14])
    } else {
        format!("{}...", &full_api_key[..full_api_key.len().min(8)])
    };

    // Compute SHA-256 hash for secure database persistence
    let mut hasher = Sha256::new();
    hasher.update(full_api_key.as_bytes());
    let key_hash = format!("{:x}", hasher.finalize());

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        r#"
        INSERT INTO user_api_keys (id, user_id, name, key_hash, key_prefix, created_at)
        VALUES (?, ?, ?, ?, ?, ?)
        "#
    )
    .bind(&id)
    .bind(&auth.user_id)
    .bind(&final_name)
    .bind(&key_hash)
    .bind(&key_prefix)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Database error creating API key: {}", e) })),
        )
    })?;

    Ok((
        StatusCode::CREATED,
        Json(CreateApiKeyResponse {
            id,
            name: final_name,
            key_prefix,
            api_key: full_api_key,
            created_at: now,
        }),
    ))
}

pub async fn delete_api_key_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let result = sqlx::query("DELETE FROM user_api_keys WHERE id = ? AND user_id = ?")
        .bind(&id)
        .bind(&auth.user_id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Database error deleting API key: {}", e) })),
            )
        })?;

    if result.rows_affected() == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "API key not found" })),
        ));
    }

    Ok(Json(serde_json::json!({
        "deleted": true,
        "id": id,
    })))
}

