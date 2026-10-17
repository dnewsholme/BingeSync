use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};

use crate::{
    api::AppState,
    auth::AuthUser,
    models::{AdminUserItem, User},
};

pub async fn list_users(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<AdminUserItem>>, (StatusCode, Json<serde_json::Value>)> {
    // 1. Verify caller has admin role
    if auth.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Forbidden: Admin privileges required" })),
        ));
    }

    // 2. Fetch all users with scrobble counts
    let users = sqlx::query_as::<_, AdminUserItem>(
        r#"
        SELECT 
            u.id,
            u.email,
            u.display_name,
            u.role,
            u.preferred_username,
            u.webhook_token,
            u.avatar_url,
            u.created_at,
            u.updated_at,
            COALESCE(COUNT(s.id), 0) AS scrobble_count
        FROM users u
        LEFT JOIN scrobbles s ON s.user_id = u.id
        GROUP BY u.id
        ORDER BY u.created_at ASC
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))))?;

    Ok(Json(users))
}

pub async fn delete_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(target_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // 1. Verify caller has admin role
    if auth.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Forbidden: Admin privileges required" })),
        ));
    }

    // 2. Prevent self-deletion
    if target_id == auth.user_id {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Cannot delete your own admin account" })),
        ));
    }

    // 3. Verify target user exists
    let target = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
        .bind(&target_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "User not found" }))))?;

    // 4. Delete user and ALL associated data inside a transaction
    let mut tx = state.pool.begin().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))))?;

    sqlx::query("DELETE FROM active_sessions WHERE user_id = ?")
        .bind(&target_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))))?;

    sqlx::query("DELETE FROM scrobbles WHERE user_id = ?")
        .bind(&target_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))))?;

    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(&target_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))))?;

    tx.commit().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))))?;

    tracing::info!(
        admin_id = %auth.user_id,
        deleted_user_id = %target_id,
        deleted_email = %target.email,
        "Admin deleted user and all associated data"
    );

    Ok(Json(serde_json::json!({
        "deleted": true,
        "user_id": target_id,
        "email": target.email,
        "message": format!("User {} and all associated watch history and sessions deleted successfully", target.email),
    })))
}
