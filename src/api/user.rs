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
    let jellyfin_webhook_url = format!("{}/api/v1/webhooks/jellyfin/{}", trimmed, user.webhook_token);
    let emby_webhook_url = format!("{}/api/v1/webhooks/emby/{}", trimmed, user.webhook_token);

    Ok(Json(serde_json::json!({
        "webhook_token": user.webhook_token,
        "webhook_url": webhook_url,
        "plex_webhook_url": webhook_url,
        "jellyfin_webhook_url": jellyfin_webhook_url,
        "emby_webhook_url": emby_webhook_url,
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
    let jellyfin_webhook_url = format!("{}/api/v1/webhooks/jellyfin/{}", trimmed, new_token);
    let emby_webhook_url = format!("{}/api/v1/webhooks/emby/{}", trimmed, new_token);

    Ok(Json(serde_json::json!({
        "webhook_token": new_token,
        "webhook_url": webhook_url,
        "plex_webhook_url": webhook_url,
        "jellyfin_webhook_url": jellyfin_webhook_url,
        "emby_webhook_url": emby_webhook_url,
        "message": "Webhook token successfully regenerated"
    })))
}

fn escape_csv(val: &str) -> String {
    if val.contains(',') || val.contains('"') || val.contains('\n') || val.contains('\r') {
        format!("\"{}\"", val.replace('"', "\"\""))
    } else {
        val.to_string()
    }
}

pub async fn export_user_data(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl axum::response::IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    use std::io::{Cursor, Write};
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

    let scrobbles: Vec<crate::models::Scrobble> = sqlx::query_as(
        "SELECT * FROM scrobbles WHERE user_id = ? ORDER BY watched_at DESC"
    )
    .bind(&auth.user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to query scrobbles: {}", e) })),
        )
    })?;

    let scrobbles_json = serde_json::to_vec_pretty(&scrobbles).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to serialize scrobbles: {}", e) })),
        )
    })?;

    // Build scrobbles.csv
    let mut scrobbles_csv = String::from("id,title,media_type,grandparent_title,parent_title,season_number,episode_number,year,user_rating,watch_status,progress_percent,duration_ms,watched_at,created_at,genres,director,cinematographer,actors,tvdb_id,tags,notes\n");
    for s in &scrobbles {
        scrobbles_csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            escape_csv(&s.id),
            escape_csv(&s.title),
            escape_csv(&s.media_type),
            escape_csv(s.grandparent_title.as_deref().unwrap_or("")),
            escape_csv(s.parent_title.as_deref().unwrap_or("")),
            s.season_number.map(|n| n.to_string()).unwrap_or_default(),
            s.episode_number.map(|n| n.to_string()).unwrap_or_default(),
            s.year.map(|n| n.to_string()).unwrap_or_default(),
            s.user_rating.map(|n| n.to_string()).unwrap_or_default(),
            escape_csv(s.watch_status.as_deref().unwrap_or("")),
            s.progress_percent.map(|n| n.to_string()).unwrap_or_default(),
            s.duration_ms.map(|n| n.to_string()).unwrap_or_default(),
            escape_csv(&s.watched_at),
            escape_csv(&s.created_at),
            escape_csv(s.genres.as_deref().unwrap_or("")),
            escape_csv(s.director.as_deref().unwrap_or("")),
            escape_csv(s.cinematographer.as_deref().unwrap_or("")),
            escape_csv(s.actors.as_deref().unwrap_or("")),
            escape_csv(s.tvdb_id.as_deref().unwrap_or("")),
            escape_csv(s.tags.as_deref().unwrap_or("")),
            escape_csv(s.notes.as_deref().unwrap_or("")),
        ));
    }

    // Build ratings.json & ratings.csv
    let rated_items: Vec<_> = scrobbles
        .iter()
        .filter(|s| s.user_rating.unwrap_or(0) > 0)
        .collect();

    let ratings_json = serde_json::to_vec_pretty(&rated_items).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to serialize ratings: {}", e) })),
        )
    })?;

    let mut ratings_csv = String::from("title,media_type,year,grandparent_title,season_number,episode_number,rating,rated_at\n");
    for s in &rated_items {
        ratings_csv.push_str(&format!(
            "{},{},{},{},{},{},{},{}\n",
            escape_csv(&s.title),
            escape_csv(&s.media_type),
            s.year.map(|n| n.to_string()).unwrap_or_default(),
            escape_csv(s.grandparent_title.as_deref().unwrap_or("")),
            s.season_number.map(|n| n.to_string()).unwrap_or_default(),
            s.episode_number.map(|n| n.to_string()).unwrap_or_default(),
            s.user_rating.unwrap_or_default(),
            escape_csv(&s.watched_at),
        ));
    }

    // Zip archive creation
    let mut zip_buffer = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut zip_buffer);
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated);

        zip.start_file("scrobbles.json", options).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to create scrobbles.json in zip: {}", e) })),
            )
        })?;
        zip.write_all(&scrobbles_json).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to write scrobbles.json: {}", e) })),
            )
        })?;

        zip.start_file("scrobbles.csv", options).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to create scrobbles.csv in zip: {}", e) })),
            )
        })?;
        zip.write_all(scrobbles_csv.as_bytes()).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to write scrobbles.csv: {}", e) })),
            )
        })?;

        zip.start_file("ratings.json", options).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to create ratings.json in zip: {}", e) })),
            )
        })?;
        zip.write_all(&ratings_json).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to write ratings.json: {}", e) })),
            )
        })?;

        zip.start_file("ratings.csv", options).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to create ratings.csv in zip: {}", e) })),
            )
        })?;
        zip.write_all(ratings_csv.as_bytes()).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to write ratings.csv: {}", e) })),
            )
        })?;

        zip.finish().map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to finalize zip archive: {}", e) })),
            )
        })?;
    }

    let zip_bytes = zip_buffer.into_inner();
    let filename = format!("bingesync_export_{}.zip", chrono::Utc::now().format("%Y%m%d_%H%M%S"));

    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "application/zip".to_string()),
            (
                axum::http::header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        zip_bytes,
    ))
}
