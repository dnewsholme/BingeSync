use axum::{
    extract::State,
    http::StatusCode,
    Json,
};

use crate::{
    api::AppState,
    auth::AuthUser,
    models::ActiveSession,
};

pub async fn get_now_playing(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let session = sqlx::query_as::<_, ActiveSession>(
        "SELECT * FROM active_sessions WHERE user_id = ? ORDER BY updated_at DESC LIMIT 1"
    )
    .bind(&auth.user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Database error: {}", e) })),
        )
    })?;

    Ok(Json(serde_json::json!({
        "now_playing": session
    })))
}
