use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    api::AppState,
    models::{PlexWebhookPayload, User},
};

#[derive(Debug, Deserialize)]
pub struct WebhookQuery {
    pub token: Option<String>,
}

pub async fn handle_plex_webhook_path(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    multipart_or_body: axum::body::Bytes,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    process_webhook(state, token, multipart_or_body, headers).await
}

pub async fn handle_plex_webhook_query(
    State(state): State<AppState>,
    Query(query): Query<WebhookQuery>,
    headers: HeaderMap,
    multipart_or_body: axum::body::Bytes,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let token = query.token.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Missing webhook token in query parameter 'token'" })),
        )
    })?;

    process_webhook(state, token, multipart_or_body, headers).await
}

async fn process_webhook(
    state: AppState,
    token: String,
    body_bytes: axum::body::Bytes,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    // 1. Authenticate user by webhook_token
    let user: User = sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE webhook_token = ?"
    )
    .bind(&token)
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
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Invalid webhook token" })),
        )
    })?;

    // 2. Parse payload from multipart or application/json
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let payload: PlexWebhookPayload = if content_type.starts_with("multipart/form-data") {
        parse_multipart_payload(content_type, body_bytes).await.map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Failed to parse multipart Plex payload: {}", e) })),
            )
        })?
    } else {
        serde_json::from_slice(&body_bytes).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Failed to parse JSON Plex payload: {}", e) })),
            )
        })?
    };

    tracing::info!(
        user_id = %user.id,
        event = %payload.event,
        title = ?payload.metadata.as_ref().and_then(|m| m.title.as_deref()),
        "Received Plex webhook"
    );

    // Filter by Plex account username if present
    let account_title = payload
        .account
        .as_ref()
        .and_then(|a| a.title.as_deref())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    if let Some(account_title) = account_title {
        let configured_plex_user =
            crate::api::user_settings::get_user_setting(&state.pool, &user.id, "plex_username").await;

        let allowed_usernames: Vec<String> = if let Some(ref cfg) = configured_plex_user {
            let list: Vec<String> = cfg
                .split(',')
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect();
            if list.is_empty() {
                get_default_user_candidates(&user)
            } else {
                list
            }
        } else {
            get_default_user_candidates(&user)
        };

        let matches = allowed_usernames
            .iter()
            .any(|allowed| allowed == &account_title.to_lowercase());

        if !matches {
            tracing::info!(
                user_id = %user.id,
                account_title = %account_title,
                allowed = ?allowed_usernames,
                "Ignored Plex webhook: username does not match allowed Plex username(s)"
            );
            return Ok((
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "ignored",
                    "reason": "username_mismatch",
                    "account": account_title
                })),
            ));
        }
    }

    let metadata = match payload.metadata {
        Some(m) => m,
        None => {
            return Ok((
                StatusCode::OK,
                Json(serde_json::json!({ "status": "ignored", "reason": "no metadata" })),
            ));
        }
    };

    let title = metadata.title.unwrap_or_else(|| "Unknown Title".to_string());
    let media_type = metadata.media_type.unwrap_or_else(|| "movie".to_string());
    let grandparent_title = metadata.grandparent_title;
    let parent_title = metadata.parent_title;
    let season_number = metadata.parent_index;
    let episode_number = metadata.index;
    let year = metadata.year;
    let overview = metadata.summary;
    let raw_thumb = metadata.thumb
        .or(metadata.grandparent_thumb)
        .or(metadata.parent_thumb);
    let thumb_url = raw_thumb.filter(|u| {
        u.starts_with("http://") || u.starts_with("https://") || u.starts_with("/api/v1/images/")
    });
    let rating_key = metadata.rating_key;
    let guid = metadata.guid;
    let duration_ms = metadata.duration;
    let genres = metadata.genre.as_ref().and_then(|list| {
        let tags: Vec<String> = list.iter().filter_map(|t| t.tag.clone()).collect();
        if tags.is_empty() { None } else { Some(tags.join(", ")) }
    });
    let directors = metadata.director.as_ref().and_then(|list| {
        let tags: Vec<String> = list.iter().filter_map(|t| t.tag.clone()).collect();
        if tags.is_empty() { None } else { Some(tags.join(", ")) }
    });
    let actors = metadata.role.as_ref().and_then(|list| {
        let tags: Vec<String> = list.iter().filter_map(|t| t.tag.clone()).collect();
        if tags.is_empty() { None } else { Some(tags.join(", ")) }
    });
    let now_str = Utc::now().to_rfc3339();

    let threshold = state.get_effective_scrobble_threshold().await;
    let view_offset_val = metadata.view_offset.unwrap_or(0);
    let duration_val = duration_ms.unwrap_or(0);

    let is_scrobble_event = payload.event == "media.scrobble";
    let calculated_pct = if duration_val > 0 {
        ((view_offset_val as f64 / duration_val as f64) * 100.0).round() as i64
    } else {
        0
    };

    let progress_percent = if is_scrobble_event {
        100
    } else {
        calculated_pct.clamp(0, 100)
    };

    let is_completed = is_scrobble_event || progress_percent >= threshold;
    let watch_status = if is_completed { "completed" } else { "in_progress" };

    // Same-day de-duplication check (prefix match YYYY-MM-DD)
    let today_prefix = if now_str.len() >= 10 { &now_str[..10] } else { "" };

    #[derive(sqlx::FromRow)]
    struct ScrobbleMatch {
        id: String,
        watch_status: Option<String>,
        progress_percent: Option<i64>,
        view_offset_ms: Option<i64>,
    }

    let existing: Option<ScrobbleMatch> = if media_type == "episode" {
        sqlx::query_as::<_, ScrobbleMatch>(
            r#"
            SELECT id, watch_status, progress_percent, view_offset_ms
            FROM scrobbles
            WHERE user_id = ?
              AND media_type = 'episode'
              AND (
                  (grandparent_title = ? AND season_number = ? AND episode_number = ?)
                  OR (guid IS NOT NULL AND guid = ?)
                  OR (rating_key IS NOT NULL AND rating_key = ?)
              )
              AND substr(watched_at, 1, 10) = ?
            ORDER BY watched_at DESC
            LIMIT 1
            "#
        )
        .bind(&user.id)
        .bind(&grandparent_title)
        .bind(season_number)
        .bind(episode_number)
        .bind(&guid)
        .bind(&rating_key)
        .bind(today_prefix)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None)
    } else {
        sqlx::query_as::<_, ScrobbleMatch>(
            r#"
            SELECT id, watch_status, progress_percent, view_offset_ms
            FROM scrobbles
            WHERE user_id = ?
              AND media_type = 'movie'
              AND (
                  title = ?
                  OR (guid IS NOT NULL AND guid = ?)
                  OR (rating_key IS NOT NULL AND rating_key = ?)
              )
              AND substr(watched_at, 1, 10) = ?
            ORDER BY watched_at DESC
            LIMIT 1
            "#
        )
        .bind(&user.id)
        .bind(&title)
        .bind(&guid)
        .bind(&rating_key)
        .bind(today_prefix)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None)
    };

    let scrobble_id = if let Some(existing) = existing {
        let final_status = if existing.watch_status.as_deref() == Some("completed") || is_completed {
            "completed"
        } else {
            "in_progress"
        };
        let final_progress = if final_status == "completed" {
            100.max(progress_percent).max(existing.progress_percent.unwrap_or(0))
        } else {
            progress_percent.max(existing.progress_percent.unwrap_or(0))
        };
        let final_offset = view_offset_val.max(existing.view_offset_ms.unwrap_or(0));

        sqlx::query(
            r#"
            UPDATE scrobbles
            SET watch_status = ?,
                progress_percent = ?,
                view_offset_ms = ?,
                watched_at = ?,
                thumb_url = COALESCE(thumb_url, ?),
                genres = COALESCE(genres, ?),
                directors = COALESCE(directors, ?),
                actors = COALESCE(actors, ?)
            WHERE id = ?
            "#
        )
        .bind(final_status)
        .bind(final_progress)
        .bind(final_offset)
        .bind(&now_str)
        .bind(&thumb_url)
        .bind(&genres)
        .bind(&directors)
        .bind(&actors)
        .bind(&existing.id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to update scrobble: {}", e) })),
            )
        })?;

        existing.id
    } else {
        let new_id = Uuid::new_v4().to_string();
        let has_full_meta = genres.as_deref().map(|g| !g.is_empty()).unwrap_or(false)
            && overview.as_deref().map(|o| !o.is_empty()).unwrap_or(false);
        let meta_updated = if has_full_meta { Some(now_str.clone()) } else { None };

        sqlx::query(
            r#"
            INSERT INTO scrobbles (
                id, user_id, title, media_type, grandparent_title, parent_title,
                season_number, episode_number, year, overview, thumb_url,
                rating_key, guid, duration_ms, user_rating,
                genres, directors, actors, watch_status, progress_percent, view_offset_ms,
                metadata_updated_at, watched_at, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&new_id)
        .bind(&user.id)
        .bind(&title)
        .bind(&media_type)
        .bind(&grandparent_title)
        .bind(&parent_title)
        .bind(season_number)
        .bind(episode_number)
        .bind(year)
        .bind(&overview)
        .bind(&thumb_url)
        .bind(&rating_key)
        .bind(&guid)
        .bind(duration_ms)
        .bind(None::<i32>)
        .bind(&genres)
        .bind(&directors)
        .bind(&actors)
        .bind(watch_status)
        .bind(progress_percent)
        .bind(view_offset_val)
        .bind(meta_updated)
        .bind(&now_str)
        .bind(&now_str)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to record scrobble: {}", e) })),
            )
        })?;

        if thumb_url.is_none() {
            state.image_notify.notify_one();
        }
        if !has_full_meta {
            state.metadata_notify.notify_one();
        }

        new_id
    };

    // 3. Handle playback state and active sessions
    match payload.event.as_str() {
        "media.scrobble" => {
            // Clear any active session for this user
            let _ = sqlx::query("DELETE FROM active_sessions WHERE user_id = ?")
                .bind(&user.id)
                .execute(&state.pool)
                .await;

            Ok((
                StatusCode::CREATED,
                Json(serde_json::json!({
                    "status": "scrobbled",
                    "id": scrobble_id,
                    "title": title,
                    "media_type": media_type,
                    "watched_at": now_str,
                    "watch_status": "completed",
                    "progress_percent": 100,
                })),
            ))
        }

        "media.play" | "media.resume" | "media.pause" => {
            let state_str = if payload.event == "media.pause" {
                "paused"
            } else {
                "playing"
            };

            let session_id = Uuid::new_v4().to_string();
            let player_title = payload.player.as_ref().and_then(|p| p.title.clone());

            // Upsert active session
            let _ = sqlx::query("DELETE FROM active_sessions WHERE user_id = ?")
                .bind(&user.id)
                .execute(&state.pool)
                .await;

            sqlx::query(
                r#"
                INSERT INTO active_sessions (
                    id, user_id, player_title, title, media_type, grandparent_title,
                    season_number, episode_number, year, thumb_url, state,
                    view_offset_ms, duration_ms, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&session_id)
            .bind(&user.id)
            .bind(&player_title)
            .bind(&title)
            .bind(&media_type)
            .bind(&grandparent_title)
            .bind(season_number)
            .bind(episode_number)
            .bind(year)
            .bind(&thumb_url)
            .bind(state_str)
            .bind(view_offset_val)
            .bind(duration_val)
            .bind(&now_str)
            .execute(&state.pool)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("Failed to update session: {}", e) })),
                )
            })?;

            Ok((
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "session_updated",
                    "id": scrobble_id,
                    "state": state_str,
                    "title": title,
                    "watch_status": watch_status,
                    "progress_percent": progress_percent,
                })),
            ))
        }

        "media.stop" => {
            let _ = sqlx::query("DELETE FROM active_sessions WHERE user_id = ?")
                .bind(&user.id)
                .execute(&state.pool)
                .await;

            Ok((
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "session_stopped",
                    "id": scrobble_id,
                    "title": title,
                    "watch_status": watch_status,
                    "progress_percent": progress_percent,
                })),
            ))
        }

        _ => Ok((
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "ignored",
                "event": payload.event,
            })),
        )),
    }
}

async fn parse_multipart_payload(
    content_type: &str,
    bytes: axum::body::Bytes,
) -> Result<PlexWebhookPayload, String> {
    let boundary = multer::parse_boundary(content_type)
        .map_err(|e| format!("Failed to parse boundary: {}", e))?;

    let cursor = std::io::Cursor::new(bytes);
    let stream = tokio_util::io::ReaderStream::new(cursor);
    let mut multipart = multer::Multipart::new(stream, boundary);

    let mut payload_opt: Option<String> = None;
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("payload") {
            if let Ok(text) = field.text().await {
                payload_opt = Some(text);
                break;
            }
        }
    }

    let payload_str = payload_opt.ok_or_else(|| "No 'payload' field found in multipart form data".to_string())?;
    serde_json::from_str(&payload_str)
        .map_err(|e| format!("Failed to deserialize payload JSON: {}", e))
}

fn get_default_user_candidates(user: &User) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(ref pref) = user.preferred_username {
        let trimmed = pref.trim().to_lowercase();
        if !trimmed.is_empty() && !candidates.contains(&trimmed) {
            candidates.push(trimmed);
        }
    }
    let display_trimmed = user.display_name.trim().to_lowercase();
    if !display_trimmed.is_empty() && !candidates.contains(&display_trimmed) {
        candidates.push(display_trimmed.clone());
    }
    for word in display_trimmed.split_whitespace() {
        let w = word.trim().to_lowercase();
        if !w.is_empty() && !candidates.contains(&w) {
            candidates.push(w);
        }
    }
    if let Some(email_prefix) = user.email.split('@').next() {
        let prefix_trimmed = email_prefix.trim().to_lowercase();
        if !prefix_trimmed.is_empty() && !candidates.contains(&prefix_trimmed) {
            candidates.push(prefix_trimmed);
        }
    }
    let email_trimmed = user.email.trim().to_lowercase();
    if !email_trimmed.is_empty() && !candidates.contains(&email_trimmed) {
        candidates.push(email_trimmed);
    }
    candidates
}
