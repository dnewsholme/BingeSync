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
    models::{JellyfinWebhookPayload, PlexWebhookPayload, User},
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

pub async fn handle_jellyfin_webhook_path(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    body_bytes: axum::body::Bytes,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    process_jellyfin_or_emby_webhook(state, token, body_bytes, headers, "jellyfin").await
}

pub async fn handle_jellyfin_webhook_query(
    State(state): State<AppState>,
    Query(query): Query<WebhookQuery>,
    headers: HeaderMap,
    body_bytes: axum::body::Bytes,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let token = query.token.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Missing webhook token in query parameter 'token'" })),
        )
    })?;

    process_jellyfin_or_emby_webhook(state, token, body_bytes, headers, "jellyfin").await
}

pub async fn handle_emby_webhook_path(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    body_bytes: axum::body::Bytes,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    process_jellyfin_or_emby_webhook(state, token, body_bytes, headers, "emby").await
}

pub async fn handle_emby_webhook_query(
    State(state): State<AppState>,
    Query(query): Query<WebhookQuery>,
    headers: HeaderMap,
    body_bytes: axum::body::Bytes,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let token = query.token.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Missing webhook token in query parameter 'token'" })),
        )
    })?;

    process_jellyfin_or_emby_webhook(state, token, body_bytes, headers, "emby").await
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

    // Ignore non-playback events (such as library.new when new items are added to Plex, library.on.deck, media.rate, etc.)
    // Only playback-related events should be tracked as scrobbles or active sessions.
    let is_playback_event = matches!(
        payload.event.as_str(),
        "media.play" | "media.pause" | "media.resume" | "media.stop" | "media.scrobble"
    );

    if !is_playback_event {
        tracing::info!(
            user_id = %user.id,
            event = %payload.event,
            "Ignored Plex webhook: non-playback event"
        );
        return Ok((
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "ignored",
                "reason": "non_playback_event",
                "event": payload.event
            })),
        ));
    }

    // Filter by Plex account username or ID if present
    let account_title = payload
        .account
        .as_ref()
        .and_then(|a| a.title.as_deref())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let account_id = payload
        .account
        .as_ref()
        .and_then(|a| a.id.as_ref())
        .or(payload.user_id.as_ref())
        .and_then(|v| match v {
            serde_json::Value::String(s) => Some(s.trim().to_string()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .filter(|s| !s.is_empty());

    if account_title.is_some() || account_id.is_some() {
        if let (Some(ref t), Some(ref aid)) = (&account_title, &account_id) {
            let _ = sqlx::query(
                r#"
                INSERT INTO plex_user_cache (plex_id, username, friendly_name, source, updated_at)
                VALUES (?, ?, ?, 'plex_webhook', datetime('now'))
                ON CONFLICT(plex_id) DO UPDATE SET username = excluded.username, friendly_name = excluded.friendly_name, source = 'plex_webhook', updated_at = excluded.updated_at
                "#
            )
            .bind(aid)
            .bind(t)
            .bind(t)
            .execute(&state.pool)
            .await;
        }

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

        // Expand allowed list with any mapped Plex IDs from plex_user_cache
        let mut expanded_allowed = allowed_usernames.clone();
        for allowed in &allowed_usernames {
            let cached_ids: Vec<String> = sqlx::query_scalar(
                "SELECT plex_id FROM plex_user_cache WHERE LOWER(username) = LOWER(?) OR LOWER(friendly_name) = LOWER(?)"
            )
            .bind(allowed)
            .bind(allowed)
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();

            for cid in cached_ids {
                let low = cid.to_lowercase();
                if !expanded_allowed.contains(&low) {
                    expanded_allowed.push(low);
                }
            }
        }

        // Candidate identifiers from incoming webhook (title, id, and any resolved usernames)
        let mut candidate_identifiers: Vec<String> = Vec::new();
        if let Some(ref title) = account_title {
            candidate_identifiers.push(title.to_lowercase());
            if let Ok(Some(un)) = sqlx::query_scalar::<_, String>(
                "SELECT username FROM plex_user_cache WHERE LOWER(plex_id) = LOWER(?) LIMIT 1"
            )
            .bind(title)
            .fetch_optional(&state.pool)
            .await {
                candidate_identifiers.push(un.to_lowercase());
            }
        }
        if let Some(ref aid) = account_id {
            candidate_identifiers.push(aid.to_lowercase());
            if let Ok(Some(un)) = sqlx::query_scalar::<_, String>(
                "SELECT username FROM plex_user_cache WHERE LOWER(plex_id) = LOWER(?) LIMIT 1"
            )
            .bind(aid)
            .fetch_optional(&state.pool)
            .await {
                candidate_identifiers.push(un.to_lowercase());
            }
        }

        let matches = candidate_identifiers.iter().any(|c| expanded_allowed.contains(c));
        if !matches && (account_id.is_some() || account_title.is_some()) {
            state.user_sync_notify.notify_one();
        }

        if !matches {
            state.log_buffer.push(
                "INFO",
                "bingesync::plex",
                &format!("Ignored Plex webhook for {}: username '{:?}' / ID '{:?}' does not match allowed user(s)", user.email, account_title, account_id),
            );
            tracing::info!(
                user_id = %user.id,
                account_title = ?account_title,
                account_id = ?account_id,
                allowed = ?expanded_allowed,
                "Ignored Plex webhook: username/ID does not match allowed Plex user(s)"
            );
            return Ok((
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "ignored",
                    "reason": "username_mismatch",
                    "account": account_title,
                    "account_id": account_id
                })),
            ));
        }
    }

    let source_name = Some("plex".to_string());
    let resolved_source_user: Option<String> = if let Some(ref title) = account_title {
        Some(title.clone())
    } else if let Some(ref aid) = account_id {
        if let Ok(Some(un)) = sqlx::query_scalar::<_, String>(
            "SELECT username FROM plex_user_cache WHERE LOWER(plex_id) = LOWER(?) LIMIT 1"
        )
        .bind(aid)
        .fetch_optional(&state.pool)
        .await {
            Some(un)
        } else {
            Some(aid.clone())
        }
    } else {
        user.preferred_username.clone().or_else(|| Some(user.display_name.clone()))
    };

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
    let director = directors.as_ref().and_then(|d| d.split(',').next().map(|s| s.trim().to_string()));
    let cinematographer: Option<String> = None;
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
                director = COALESCE(director, ?),
                actors = COALESCE(actors, ?),
                source = COALESCE(source, ?),
                source_user = COALESCE(source_user, ?)
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
        .bind(&director)
        .bind(&actors)
        .bind(&source_name)
        .bind(&resolved_source_user)
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
                genres, directors, director, cinematographer, actors, watch_status, progress_percent, view_offset_ms,
                metadata_updated_at, source, source_user, watched_at, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
        .bind(&director)
        .bind(&cinematographer)
        .bind(&actors)
        .bind(watch_status)
        .bind(progress_percent)
        .bind(view_offset_val)
        .bind(meta_updated)
        .bind(&source_name)
        .bind(&resolved_source_user)
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

            // Dispatch bidirectional writeback & outbound notification
            crate::api::media_writeback::dispatch_media_writeback(
                state.pool.clone(),
                user.id.clone(),
                crate::api::media_writeback::WritebackItem {
                    title: title.clone(),
                    media_type: media_type.clone(),
                    grandparent_title: grandparent_title.clone(),
                    season_number,
                    episode_number,
                    year,
                    rating_key: rating_key.clone(),
                },
            );

            crate::api::outbound_webhooks::dispatch_scrobble_notification(
                state.pool.clone(),
                user.id.clone(),
                crate::api::outbound_webhooks::ScrobbleNotificationData {
                    title: title.clone(),
                    grandparent_title: grandparent_title.clone(),
                    media_type: media_type.clone(),
                    season_number,
                    episode_number,
                    year,
                    progress_percent: 100,
                    thumb_url: thumb_url.clone(),
                    user_rating: None,
                },
            );

            state.log_buffer.push(
                "INFO",
                "bingesync::plex",
                &format!("Plex scrobble recorded for user {}: '{}' ({})", user.email, title, media_type),
            );

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
                    source, source_user,
                    view_offset_ms, duration_ms, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
            .bind(&source_name)
            .bind(&resolved_source_user)
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

async fn process_jellyfin_or_emby_webhook(
    state: AppState,
    token: String,
    body_bytes: axum::body::Bytes,
    _headers: axum::http::HeaderMap,
    source: &str,
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

    // 2. Parse JSON payload
    let payload: JellyfinWebhookPayload = serde_json::from_slice(&body_bytes).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("Failed to parse JSON {} payload: {}", source, e) })),
        )
    })?;

    let event_name = payload
        .notification_type
        .clone()
        .unwrap_or_else(|| "PlaybackStop".to_string());

    tracing::info!(
        user_id = %user.id,
        source = %source,
        event = %event_name,
        name = ?payload.resolved_name(),
        series = ?payload.resolved_series_name(),
        "Received media webhook"
    );

    // 3. Filter by account username or User ID if configured or present
    let notif_user = payload.resolved_notification_username();
    let notif_id = payload.resolved_user_id();

    if notif_user.is_some() || notif_id.is_some() {
        if let (Some(ref un), Some(ref uid)) = (&notif_user, &notif_id) {
            let _ = sqlx::query(
                r#"
                INSERT INTO plex_user_cache (plex_id, username, friendly_name, source, updated_at)
                VALUES (?, ?, ?, ?, datetime('now'))
                ON CONFLICT(plex_id) DO UPDATE SET username = excluded.username, friendly_name = excluded.friendly_name, source = excluded.source, updated_at = excluded.updated_at
                "#
            )
            .bind(uid)
            .bind(un)
            .bind(un)
            .bind(source)
            .execute(&state.pool)
            .await;

            let unhyphenated = uid.replace('-', "");
            if unhyphenated != *uid {
                let _ = sqlx::query(
                    r#"
                    INSERT INTO plex_user_cache (plex_id, username, friendly_name, source, updated_at)
                    VALUES (?, ?, ?, ?, datetime('now'))
                    ON CONFLICT(plex_id) DO UPDATE SET username = excluded.username, friendly_name = excluded.friendly_name, source = excluded.source, updated_at = excluded.updated_at
                    "#
                )
                .bind(&unhyphenated)
                .bind(un)
                .bind(un)
                .bind(source)
                .execute(&state.pool)
                .await;
            }
        }

        let configured_source_user = if source == "emby" {
            crate::api::user_settings::get_user_setting(&state.pool, &user.id, "emby_username").await
        } else {
            crate::api::user_settings::get_user_setting(&state.pool, &user.id, "jellyfin_username").await
        };
        let configured_user = if let Some(s_user) = configured_source_user.filter(|s| !s.trim().is_empty()) {
            Some(s_user)
        } else {
            crate::api::user_settings::get_user_setting(&state.pool, &user.id, "plex_username").await
        };

        let allowed_usernames: Vec<String> = if let Some(ref cfg) = configured_user {
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

        // Expand allowed list with any mapped IDs from plex_user_cache
        let mut expanded_allowed = allowed_usernames.clone();
        for allowed in &allowed_usernames {
            let cached_ids: Vec<String> = sqlx::query_scalar(
                "SELECT plex_id FROM plex_user_cache WHERE LOWER(username) = LOWER(?) OR LOWER(friendly_name) = LOWER(?)"
            )
            .bind(allowed)
            .bind(allowed)
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();

            for cid in cached_ids {
                let low = cid.to_lowercase();
                if !expanded_allowed.contains(&low) {
                    expanded_allowed.push(low.clone());
                    let unhyphenated = low.replace('-', "");
                    if !expanded_allowed.contains(&unhyphenated) {
                        expanded_allowed.push(unhyphenated);
                    }
                }
            }
        }

        let mut candidate_identifiers: Vec<String> = Vec::new();
        if let Some(ref un) = notif_user {
            candidate_identifiers.push(un.to_lowercase());
            if let Ok(Some(uid)) = sqlx::query_scalar::<_, String>(
                "SELECT plex_id FROM plex_user_cache WHERE LOWER(username) = LOWER(?) OR LOWER(friendly_name) = LOWER(?) LIMIT 1"
            )
            .bind(un)
            .bind(un)
            .fetch_optional(&state.pool)
            .await {
                candidate_identifiers.push(uid.to_lowercase());
            }
        }
        if let Some(ref uid) = notif_id {
            candidate_identifiers.push(uid.to_lowercase());
            let unhyphenated = uid.replace('-', "").to_lowercase();
            if unhyphenated != uid.to_lowercase() {
                candidate_identifiers.push(unhyphenated);
            }
            if let Ok(Some(un)) = sqlx::query_scalar::<_, String>(
                "SELECT username FROM plex_user_cache WHERE LOWER(plex_id) = LOWER(?) LIMIT 1"
            )
            .bind(uid)
            .fetch_optional(&state.pool)
            .await {
                candidate_identifiers.push(un.to_lowercase());
            }
        }

        let matches = candidate_identifiers.iter().any(|c| expanded_allowed.contains(c));
        if !matches {
            state.log_buffer.push(
                "INFO",
                &format!("bingesync::{}", source),
                &format!("Ignored {} webhook for {}: username '{:?}' / ID '{:?}' does not match allowed user(s)", source, user.email, notif_user, notif_id),
            );
            tracing::info!(
                user_id = %user.id,
                source = %source,
                notification_username = ?notif_user,
                user_id_param = ?notif_id,
                allowed = ?allowed_usernames,
                "Ignored webhook: username/ID does not match allowed user(s)"
            );
            return Ok((
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "ignored",
                    "reason": "username_mismatch",
                    "account": notif_user,
                    "account_id": notif_id
                })),
            ));
        }
    }

    let source_name = Some(source.to_string());
    let resolved_source_user: Option<String> = if let Some(ref un) = notif_user {
        Some(un.clone())
    } else if let Some(ref uid) = notif_id {
        if let Ok(Some(un)) = sqlx::query_scalar::<_, String>(
            "SELECT username FROM plex_user_cache WHERE LOWER(plex_id) = LOWER(?) LIMIT 1"
        )
        .bind(uid)
        .fetch_optional(&state.pool)
        .await {
            Some(un)
        } else {
            Some(uid.clone())
        }
    } else {
        user.preferred_username.clone().or_else(|| Some(user.display_name.clone()))
    };

    // 4. Resolve media metadata
    let raw_type = payload.resolved_item_type().unwrap_or_else(|| "Movie".to_string());
    let is_episode = raw_type.eq_ignore_ascii_case("Episode");
    let media_type = if is_episode { "episode".to_string() } else { "movie".to_string() };

    let genres = payload.resolved_genres();
    let thumb_url = payload.resolved_thumb_url();
    let rating_key = payload.item_id.clone().or_else(|| payload.resolved_tvdb_id()).or_else(|| payload.resolved_tmdb_id());
    let guid = if let Some(imdb) = payload.resolved_imdb_id() {
        Some(format!("imdb://{}", imdb))
    } else if let Some(ref item_id) = payload.item_id {
        Some(format!("{}://{}", source, item_id))
    } else {
        None
    };

    let title = payload.resolved_name().unwrap_or_else(|| "Unknown Title".to_string());
    let grandparent_title = if is_episode { payload.resolved_series_name() } else { None };
    let season_number = if is_episode { payload.resolved_season_number() } else { None };
    let episode_number = if is_episode { payload.resolved_episode_number() } else { None };
    let parent_title = if is_episode {
        season_number.map(|s| format!("Season {}", s))
    } else {
        None
    };

    let year = payload.resolved_year();
    let overview = payload.resolved_overview();

    // Jellyfin / Emby ticks: 10,000 ticks = 1 ms
    let duration_ms = payload.resolved_runtime_ticks().map(|t| t / 10_000);
    let view_offset_val = payload.resolved_playback_position_ticks().map(|t| t / 10_000).unwrap_or(0);
    let duration_val = duration_ms.unwrap_or(0);

    let threshold = state.get_effective_scrobble_threshold().await;
    let calculated_pct = if duration_val > 0 {
        ((view_offset_val as f64 / duration_val as f64) * 100.0).round() as i64
    } else {
        0
    };

    let lower_event = event_name.to_lowercase();
    let is_stop = lower_event.contains("stop") || lower_event.contains("scrobble") || lower_event.contains("markplayed");
    let is_start = lower_event.contains("start") || lower_event.contains("unpause") || lower_event.contains("resume");
    let is_pause = lower_event.contains("pause");
    let is_progress = lower_event.contains("progress");
    let is_saved = lower_event.contains("userdatasaved");

    let played_flag = payload.played_to_completion == Some(true) || payload.played == Some(true) || lower_event.contains("scrobble") || lower_event.contains("markplayed");
    let is_completed = played_flag || (is_stop && calculated_pct >= threshold) || (is_saved && played_flag);

    // Ignore non-playback events (such as ItemAdded, LibraryChanged, etc.)
    let is_playback_event = is_stop || is_start || is_pause || is_progress || (is_saved && played_flag);
    if !is_playback_event {
        tracing::info!(
            user_id = %user.id,
            source = %source,
            event = %event_name,
            "Ignored webhook: non-playback event"
        );
        return Ok((
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "ignored",
                "reason": "non_playback_event",
                "event": event_name
            })),
        ));
    }

    let progress_percent = if is_completed {
        100.max(calculated_pct)
    } else {
        calculated_pct.clamp(0, 100)
    };

    let watch_status = if is_completed { "completed" } else { "in_progress" };
    let now_str = Utc::now().to_rfc3339();
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
                source = COALESCE(source, ?),
                source_user = COALESCE(source_user, ?)
            WHERE id = ?
            "#
        )
        .bind(final_status)
        .bind(final_progress)
        .bind(final_offset)
        .bind(&now_str)
        .bind(&thumb_url)
        .bind(&genres)
        .bind(&source_name)
        .bind(&resolved_source_user)
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
                genres, directors, director, cinematographer, actors, watch_status, progress_percent, view_offset_ms,
                metadata_updated_at, source, source_user, watched_at, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
        .bind(None::<String>) // directors
        .bind(None::<String>) // director
        .bind(None::<String>) // cinematographer
        .bind(None::<String>) // actors
        .bind(watch_status)
        .bind(progress_percent)
        .bind(view_offset_val)
        .bind(meta_updated)
        .bind(&source_name)
        .bind(&resolved_source_user)
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

    if is_completed {
        crate::api::media_writeback::dispatch_media_writeback(
            state.pool.clone(),
            user.id.clone(),
            crate::api::media_writeback::WritebackItem {
                title: title.clone(),
                media_type: media_type.clone(),
                grandparent_title: grandparent_title.clone(),
                season_number,
                episode_number,
                year,
                rating_key: rating_key.clone(),
            },
        );

        crate::api::outbound_webhooks::dispatch_scrobble_notification(
            state.pool.clone(),
            user.id.clone(),
            crate::api::outbound_webhooks::ScrobbleNotificationData {
                title: title.clone(),
                grandparent_title: grandparent_title.clone(),
                media_type: media_type.clone(),
                season_number,
                episode_number,
                year,
                progress_percent,
                thumb_url: thumb_url.clone(),
                user_rating: None,
            },
        );
    }

    // 5. Handle active playback session state
    if is_stop || (is_saved && is_completed) {
        let _ = sqlx::query("DELETE FROM active_sessions WHERE user_id = ?")
            .bind(&user.id)
            .execute(&state.pool)
            .await;

        let status_code = if is_completed {
            state.log_buffer.push(
                "INFO",
                &format!("bingesync::{}", source),
                &format!("{} scrobble recorded for user {}: '{}' ({})", if source == "emby" { "Emby" } else { "Jellyfin" }, user.email, title, media_type),
            );
            StatusCode::CREATED
        } else {
            StatusCode::OK
        };

        Ok((
            status_code,
            Json(serde_json::json!({
                "status": if is_completed { "scrobbled" } else { "stopped" },
                "id": scrobble_id,
                "title": title,
                "media_type": media_type,
                "watched_at": now_str,
                "watch_status": watch_status,
                "progress_percent": progress_percent,
            })),
        ))
    } else if is_start || is_progress || is_pause {
        let state_str = if is_pause { "paused" } else { "playing" };
        let session_id = Uuid::new_v4().to_string();
        let player_title = payload.device_name.clone().or(payload.client.clone());

        let _ = sqlx::query("DELETE FROM active_sessions WHERE user_id = ?")
            .bind(&user.id)
            .execute(&state.pool)
            .await;

        sqlx::query(
            r#"
            INSERT INTO active_sessions (
                id, user_id, player_title, title, media_type, grandparent_title,
                season_number, episode_number, year, thumb_url, state,
                source, source_user,
                view_offset_ms, duration_ms, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
        .bind(&source_name)
        .bind(&resolved_source_user)
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

        let status_code = if is_completed {
            state.log_buffer.push(
                "INFO",
                &format!("bingesync::{}", source),
                &format!("{} scrobble recorded for user {}: '{}' ({})", if source == "emby" { "Emby" } else { "Jellyfin" }, user.email, title, media_type),
            );
            StatusCode::CREATED
        } else {
            StatusCode::OK
        };

        Ok((
            status_code,
            Json(serde_json::json!({
                "status": if is_completed { "scrobbled" } else { "session_updated" },
                "id": scrobble_id,
                "title": title,
                "media_type": media_type,
                "watch_status": watch_status,
                "progress_percent": progress_percent,
                "session_state": state_str,
            })),
        ))
    } else {
        Ok((
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "received",
                "id": scrobble_id,
                "event": event_name,
                "watch_status": watch_status,
                "progress_percent": progress_percent
            })),
        ))
    }
}

