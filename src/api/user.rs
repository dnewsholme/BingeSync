use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use uuid::Uuid;

use crate::{
    api::AppState,
    auth::AuthUser,
    models::User,
};

pub async fn get_webhook_info(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
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

    let trimmed = state.config.app_url.trim_end_matches('/');
    let webhook_url = format!("{}/api/v1/webhooks/plex/{}", trimmed, user.webhook_token);

    Ok(Json(serde_json::json!({
        "webhook_token": user.webhook_token,
        "webhook_url": webhook_url,
    })))
}

pub async fn regenerate_webhook_token(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let new_token = Uuid::new_v4().to_string().replace('-', "");

    sqlx::query("UPDATE users SET webhook_token = ?, updated_at = datetime('now') WHERE id = ?")
        .bind(&new_token)
        .bind(&auth.user_id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Database error: {}", e) })),
            )
        })?;

    let trimmed = state.config.app_url.trim_end_matches('/');
    let webhook_url = format!("{}/api/v1/webhooks/plex/{}", trimmed, new_token);

    Ok(Json(serde_json::json!({
        "webhook_token": new_token,
        "webhook_url": webhook_url,
        "message": "Webhook token successfully regenerated"
    })))
}
