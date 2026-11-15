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

pub async fn check_is_rewatch(
    pool: &sqlx::SqlitePool,
    user_id: &str,
    media_type: &str,
    title: &str,
    grandparent_title: Option<&str>,
    season_number: Option<i32>,
    episode_number: Option<i32>,
    year: Option<i32>,
    watched_at: &str,
) -> bool {
    // Only previous completed watches qualify an item as a rewatch.
    // Incomplete or in-progress watches (< 80% or watch_status = 'in_progress') must never trigger rewatch status.
    if media_type == "episode" {
        let gp = grandparent_title.map(str::trim).unwrap_or("");
        if let (Some(s), Some(e)) = (season_number, episode_number) {
            let count: i64 = if !gp.is_empty() {
                sqlx::query_scalar(
                    r#"
                    SELECT COUNT(*) FROM scrobbles
                    WHERE user_id = ?
                      AND media_type = 'episode'
                      AND TRIM(LOWER(COALESCE(grandparent_title, ''))) = TRIM(LOWER(?))
                      AND season_number = ?
                      AND episode_number = ?
                      AND watched_at < ?
                      AND (watch_status = 'completed' OR (watch_status != 'in_progress' AND (progress_percent IS NULL OR progress_percent >= 80)))
                    "#,
                )
                .bind(user_id)
                .bind(gp)
                .bind(s)
                .bind(e)
                .bind(watched_at)
                .fetch_one(pool)
                .await
                .unwrap_or(0)
            } else {
                sqlx::query_scalar(
                    r#"
                    SELECT COUNT(*) FROM scrobbles
                    WHERE user_id = ?
                      AND media_type = 'episode'
                      AND (grandparent_title IS NULL OR TRIM(grandparent_title) = '')
                      AND TRIM(LOWER(title)) = TRIM(LOWER(?))
                      AND season_number = ?
                      AND episode_number = ?
                      AND watched_at < ?
                      AND (watch_status = 'completed' OR (watch_status != 'in_progress' AND (progress_percent IS NULL OR progress_percent >= 80)))
                    "#,
                )
                .bind(user_id)
                .bind(title)
                .bind(s)
                .bind(e)
                .bind(watched_at)
                .fetch_one(pool)
                .await
                .unwrap_or(0)
            };
            return count > 0;
        } else if !gp.is_empty() {
            let count: i64 = sqlx::query_scalar(
                r#"
                SELECT COUNT(*) FROM scrobbles
                WHERE user_id = ?
                  AND media_type = 'episode'
                  AND TRIM(LOWER(COALESCE(grandparent_title, ''))) = TRIM(LOWER(?))
                  AND TRIM(LOWER(title)) = TRIM(LOWER(?))
                  AND watched_at < ?
                  AND (watch_status = 'completed' OR (watch_status != 'in_progress' AND (progress_percent IS NULL OR progress_percent >= 80)))
                "#,
            )
            .bind(user_id)
            .bind(gp)
            .bind(title)
            .bind(watched_at)
            .fetch_one(pool)
            .await
            .unwrap_or(0);
            return count > 0;
        }
    } else if media_type == "movie" {
        let count: i64 = if let Some(y) = year {
            sqlx::query_scalar(
                r#"
                SELECT COUNT(*) FROM scrobbles
                WHERE user_id = ?
                  AND media_type = 'movie'
                  AND LOWER(TRIM(title)) = LOWER(TRIM(?))
                  AND (year = ? OR year IS NULL)
                  AND watched_at < ?
                  AND (watch_status = 'completed' OR (watch_status != 'in_progress' AND (progress_percent IS NULL OR progress_percent >= 80)))
                "#,
            )
            .bind(user_id)
            .bind(title)
            .bind(y)
            .bind(watched_at)
            .fetch_one(pool)
            .await
            .unwrap_or(0)
        } else {
            sqlx::query_scalar(
                r#"
                SELECT COUNT(*) FROM scrobbles
                WHERE user_id = ?
                  AND media_type = 'movie'
                  AND LOWER(TRIM(title)) = LOWER(TRIM(?))
                  AND watched_at < ?
                  AND (watch_status = 'completed' OR (watch_status != 'in_progress' AND (progress_percent IS NULL OR progress_percent >= 80)))
                "#,
            )
            .bind(user_id)
            .bind(title)
            .bind(watched_at)
            .fetch_one(pool)
            .await
            .unwrap_or(0)
        };
        return count > 0;
    }
    false
}

#[derive(sqlx::FromRow)]
struct ExistingScrobbleMatch {
    id: String,
    watch_status: Option<String>,
    progress_percent: Option<i64>,
    view_offset_ms: Option<i64>,
    duration_ms: Option<i64>,
}

#[derive(Default)]
struct PlexPlaybackDetails {
    duration_ms: Option<i64>,
    view_offset_ms: Option<i64>,
    resolution: Option<String>,
    hdr_format: Option<String>,
    audio_codec: Option<String>,
    audio_channels: Option<String>,
    video_codec: Option<String>,
}

fn json_value_to_string(v: Option<&serde_json::Value>) -> Option<String> {
    match v? {
        serde_json::Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
        }
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn json_value_to_pos_i64(v: Option<&serde_json::Value>) -> Option<i64> {
    match v? {
        serde_json::Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.round() as i64)),
        serde_json::Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
    .filter(|&x| x > 0)
}

fn extract_plex_entry_details(entry: &serde_json::Value, include_offset: bool) -> PlexPlaybackDetails {
    let media_streams: Option<Vec<crate::models::PlexMediaStream>> = entry
        .get("Media")
        .and_then(|m| serde_json::from_value(m.clone()).ok());

    let helper_meta = crate::models::PlexMetadata {
        library_section_type: None,
        media_type: None,
        subtype: None,
        extra_type: None,
        title: None,
        grandparent_title: None,
        parent_title: None,
        grandparent_thumb: None,
        parent_thumb: None,
        thumb: None,
        art: None,
        index: None,
        parent_index: None,
        year: None,
        summary: None,
        rating_key: None,
        guid: None,
        grandparent_guid: None,
        parent_guid: None,
        duration: json_value_to_pos_i64(entry.get("duration")),
        view_offset: json_value_to_pos_i64(entry.get("viewOffset")),
        studio: None,
        content_rating: None,
        media: media_streams,
        guids: None,
        genre: None,
        director: None,
        role: None,
    };

    let duration_ms = helper_meta.resolved_duration();
    let view_offset_ms = if include_offset { helper_meta.view_offset } else { None };
    let (resolution, hdr_format, audio_codec, audio_channels, video_codec) = helper_meta.resolved_media_specs();

    PlexPlaybackDetails {
        duration_ms,
        view_offset_ms,
        resolution,
        hdr_format,
        audio_codec,
        audio_channels,
        video_codec,
    }
}

async fn fetch_plex_playback_details(
    pool: &sqlx::SqlitePool,
    user_id: &str,
    rating_key: Option<&str>,
    account_title: Option<&str>,
    account_id: Option<&str>,
    event: &str,
) -> Option<PlexPlaybackDetails> {
    let rk = rating_key.map(str::trim).filter(|s| !s.is_empty())?;
    let plex_url = crate::api::user_settings::get_user_setting(pool, user_id, "plex_url").await?;
    let plex_token = crate::api::user_settings::get_user_setting(pool, user_id, "plex_api_key").await?;
    if plex_url.trim().is_empty() || plex_token.trim().is_empty() {
        return None;
    }
    if crate::api::ssrf::validate_integration_url(&plex_url).is_err() {
        return None;
    }

    let base_url = plex_url.trim().trim_end_matches('/');
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .ok()?;

    let mut details = PlexPlaybackDetails::default();
    let include_session_offset = event != "media.scrobble";

    // 1. Check live /status/sessions first (most accurate for live viewOffset and duration)
    if let Ok(resp) = client
        .get(format!("{}/status/sessions", base_url))
        .header("Accept", "application/json")
        .header("X-Plex-Token", plex_token.trim())
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if let Some(arr) = json
                    .get("MediaContainer")
                    .and_then(|mc| mc.get("Metadata"))
                    .and_then(|m| m.as_array())
                {
                    let mut matched_entry: Option<&serde_json::Value> = None;
                    for entry in arr {
                        let entry_rk = json_value_to_string(entry.get("ratingKey"));
                        if entry_rk.as_deref() != Some(rk) {
                            continue;
                        }
                        let user_id_match = account_id.is_some()
                            && json_value_to_string(entry.get("User").and_then(|u| u.get("id"))).as_deref() == account_id;
                        let user_title_match = account_title.is_some()
                            && json_value_to_string(entry.get("User").and_then(|u| u.get("title")))
                                .map(|t| t.eq_ignore_ascii_case(account_title.unwrap_or("")))
                                .unwrap_or(false);
                        if user_id_match || user_title_match {
                            matched_entry = Some(entry);
                            break;
                        }
                        if matched_entry.is_none() {
                            matched_entry = Some(entry);
                        }
                    }
                    if let Some(entry) = matched_entry {
                        details = extract_plex_entry_details(entry, include_session_offset);
                    }
                }
            }
        }
    }

    // 2. Fallback to /library/metadata/{rk} for duration (and stop/pause resume offset if session already ended)
    let want_library_offset = matches!(event, "media.pause" | "media.stop") && details.view_offset_ms.is_none();
    if details.duration_ms.is_none() || want_library_offset {
        let encoded_rk: String = url::form_urlencoded::byte_serialize(rk.as_bytes()).collect();
        if let Ok(resp) = client
            .get(format!("{}/library/metadata/{}", base_url, encoded_rk))
            .header("Accept", "application/json")
            .header("X-Plex-Token", plex_token.trim())
            .send()
            .await
        {
            if resp.status().is_success() {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    if let Some(entry) = json
                        .get("MediaContainer")
                        .and_then(|mc| mc.get("Metadata"))
                        .and_then(|m| m.as_array())
                        .and_then(|arr| arr.first())
                    {
                        let lib_details = extract_plex_entry_details(entry, want_library_offset);
                        if details.duration_ms.is_none() {
                            details.duration_ms = lib_details.duration_ms;
                        }
                        if details.view_offset_ms.is_none() {
                            details.view_offset_ms = lib_details.view_offset_ms;
                        }
                        if details.resolution.is_none() {
                            details.resolution = lib_details.resolution;
                        }
                        if details.hdr_format.is_none() {
                            details.hdr_format = lib_details.hdr_format;
                        }
                        if details.audio_codec.is_none() {
                            details.audio_codec = lib_details.audio_codec;
                        }
                        if details.audio_channels.is_none() {
                            details.audio_channels = lib_details.audio_channels;
                        }
                        if details.video_codec.is_none() {
                            details.video_codec = lib_details.video_codec;
                        }
                    }
                }
            }
        }
    }

    Some(details)
}

pub async fn resolve_fallback_duration_ms(
    pool: &sqlx::SqlitePool,
    user_id: &str,
    media_type: &str,
    title: &str,
    grandparent_title: Option<&str>,
    season_number: Option<i32>,
    episode_number: Option<i32>,
    rating_key: Option<&str>,
    guid: Option<&str>,
) -> Option<i64> {
    if let Ok(Some(active_dur)) = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT duration_ms FROM active_sessions
        WHERE user_id = ?
          AND media_type = ?
          AND LOWER(TRIM(title)) = LOWER(TRIM(?))
          AND duration_ms > 0
        ORDER BY updated_at DESC
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(media_type)
    .bind(title)
    .fetch_optional(pool)
    .await
    {
        return Some(active_dur);
    }

    if let Some(rk) = rating_key.filter(|s| !s.trim().is_empty()) {
        if let Ok(Some(dur)) = sqlx::query_scalar::<_, i64>(
            "SELECT duration_ms FROM scrobbles WHERE user_id = ? AND rating_key = ? AND duration_ms IS NOT NULL AND duration_ms > 0 ORDER BY watched_at DESC LIMIT 1",
        )
        .bind(user_id)
        .bind(rk)
        .fetch_optional(pool)
        .await
        {
            return Some(dur);
        }
    }

    if let Some(g) = guid.filter(|s| !s.trim().is_empty()) {
        if let Ok(Some(dur)) = sqlx::query_scalar::<_, i64>(
            "SELECT duration_ms FROM scrobbles WHERE user_id = ? AND guid = ? AND duration_ms IS NOT NULL AND duration_ms > 0 ORDER BY watched_at DESC LIMIT 1",
        )
        .bind(user_id)
        .bind(g)
        .fetch_optional(pool)
        .await
        {
            return Some(dur);
        }
    }

    if media_type == "episode" {
        if let (Some(gp), Some(s), Some(e)) = (
            grandparent_title.map(str::trim).filter(|g| !g.is_empty()),
            season_number,
            episode_number,
        ) {
            if let Ok(Some(dur)) = sqlx::query_scalar::<_, i64>(
                r#"
                SELECT duration_ms FROM scrobbles
                WHERE user_id = ?
                  AND media_type = 'episode'
                  AND LOWER(TRIM(COALESCE(grandparent_title, ''))) = LOWER(TRIM(?))
                  AND season_number = ?
                  AND episode_number = ?
                  AND duration_ms IS NOT NULL AND duration_ms > 0
                ORDER BY watched_at DESC
                LIMIT 1
                "#,
            )
            .bind(user_id)
            .bind(gp)
            .bind(s)
            .bind(e)
            .fetch_optional(pool)
            .await
            {
                return Some(dur);
            }
        }
    } else if let Ok(Some(dur)) = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT duration_ms FROM scrobbles
        WHERE user_id = ?
          AND media_type = 'movie'
          AND LOWER(TRIM(title)) = LOWER(TRIM(?))
          AND duration_ms IS NOT NULL AND duration_ms > 0
        ORDER BY watched_at DESC
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(title)
    .fetch_optional(pool)
    .await
    {
        return Some(dur);
    }

    None
}

#[derive(sqlx::FromRow, Clone, Debug)]
struct ActiveSessionSnapshot {
    state: String,
    view_offset_ms: i64,
    duration_ms: i64,
    updated_at: String,
}

async fn fetch_active_session_for_item(
    pool: &sqlx::SqlitePool,
    user_id: &str,
    media_type: &str,
    title: &str,
    grandparent_title: Option<&str>,
) -> Option<ActiveSessionSnapshot> {
    sqlx::query_as::<_, ActiveSessionSnapshot>(
        r#"
        SELECT state, view_offset_ms, duration_ms, updated_at
        FROM active_sessions
        WHERE user_id = ?
          AND media_type = ?
          AND LOWER(TRIM(title)) = LOWER(TRIM(?))
          AND (? IS NULL OR LOWER(TRIM(COALESCE(grandparent_title, ''))) = LOWER(TRIM(?)))
        ORDER BY updated_at DESC
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(media_type)
    .bind(title)
    .bind(grandparent_title)
    .bind(grandparent_title.unwrap_or(""))
    .fetch_optional(pool)
    .await
    .unwrap_or(None)
}

fn active_session_playing_elapsed_ms(sess: Option<&ActiveSessionSnapshot>, now_str: &str) -> i64 {
    let Some(sess) = sess else {
        return 0;
    };
    if sess.state != "playing" {
        return 0;
    }
    let (Ok(now_dt), Ok(prev_dt)) = (
        chrono::DateTime::parse_from_rfc3339(now_str),
        chrono::DateTime::parse_from_rfc3339(&sess.updated_at),
    ) else {
        return 0;
    };
    let elapsed = (now_dt.with_timezone(&Utc) - prev_dt.with_timezone(&Utc)).num_milliseconds();
    if elapsed > 0 {
        elapsed.min(4 * 3600 * 1000)
    } else {
        0
    }
}

fn estimate_view_offset_from_session_snapshot(
    sess: Option<&ActiveSessionSnapshot>,
    playing_elapsed_ms: i64,
    duration_val: i64,
) -> i64 {
    let Some(sess) = sess else {
        return 0;
    };
    if sess.state != "playing" || playing_elapsed_ms < 60_000 {
        return 0;
    }
    let mut est = sess.view_offset_ms.saturating_add(playing_elapsed_ms);
    let cap_dur = if duration_val > 0 { duration_val } else { sess.duration_ms };
    if cap_dur > 0 {
        est = est.min(cap_dur);
    }
    est.max(0)
}

#[allow(clippy::too_many_arguments)]
async fn fetch_last_paused_offset_ms(
    pool: &sqlx::SqlitePool,
    user_id: &str,
    media_type: &str,
    title: &str,
    grandparent_title: Option<&str>,
    season_number: Option<i32>,
    episode_number: Option<i32>,
    tmdb_id: Option<&str>,
    imdb_id: Option<&str>,
    guid: Option<&str>,
    rating_key: Option<&str>,
) -> i64 {
    if media_type == "episode" {
        sqlx::query_scalar::<_, Option<i64>>(
            r#"
            SELECT view_offset_ms
            FROM scrobbles
            WHERE user_id = ?
              AND media_type = 'episode'
              AND COALESCE(view_offset_ms, 0) > 0
              AND (
                  (LOWER(TRIM(COALESCE(grandparent_title, ''))) = LOWER(TRIM(?)) AND season_number = ? AND episode_number = ?)
                  OR (LOWER(TRIM(title)) = LOWER(TRIM(?)) AND season_number = ? AND episode_number = ?)
                  OR (guid IS NOT NULL AND guid = ?)
                  OR (rating_key IS NOT NULL AND rating_key = ?)
              )
            ORDER BY watched_at DESC
            LIMIT 1
            "#,
        )
        .bind(user_id)
        .bind(grandparent_title.unwrap_or(""))
        .bind(season_number)
        .bind(episode_number)
        .bind(title)
        .bind(season_number)
        .bind(episode_number)
        .bind(guid)
        .bind(rating_key)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .flatten()
        .unwrap_or(0)
    } else {
        sqlx::query_scalar::<_, Option<i64>>(
            r#"
            SELECT view_offset_ms
            FROM scrobbles
            WHERE user_id = ?
              AND media_type = 'movie'
              AND COALESCE(view_offset_ms, 0) > 0
              AND (
                  LOWER(TRIM(title)) = LOWER(TRIM(?))
                  OR (? IS NOT NULL AND ? != '' AND tmdb_id = ?)
                  OR (? IS NOT NULL AND ? != '' AND imdb_id = ?)
                  OR (guid IS NOT NULL AND guid = ?)
                  OR (rating_key IS NOT NULL AND rating_key = ?)
              )
            ORDER BY watched_at DESC
            LIMIT 1
            "#,
        )
        .bind(user_id)
        .bind(title)
        .bind(tmdb_id)
        .bind(tmdb_id)
        .bind(tmdb_id)
        .bind(imdb_id)
        .bind(imdb_id)
        .bind(imdb_id)
        .bind(guid)
        .bind(rating_key)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .flatten()
        .unwrap_or(0)
    }
}

/// Gate webhook scrobbling so re-opening or closing a paused item without watching >= 1 minute does not log or bump a watch.
#[allow(clippy::too_many_arguments)]
fn should_record_webhook_scrobble(
    is_explicit_complete: bool,
    is_pause_or_stop: bool,
    raw_view_offset_ms: i64,
    view_offset_val: i64,
    duration_val: i64,
    prior_session: Option<&ActiveSessionSnapshot>,
    playing_elapsed_ms: i64,
    existing_scrobble_offset_ms: Option<i64>,
    last_paused_offset_ms: i64,
) -> bool {
    if is_explicit_complete {
        return true;
    }

    let min_watch_ms = if duration_val > 0 && duration_val < 60_000 {
        1_000
    } else {
        60_000
    };

    if view_offset_val < min_watch_ms {
        return false;
    }

    if is_pause_or_stop {
        if prior_session.is_none() && raw_view_offset_ms == 0 {
            return false;
        }
        if let Some(sess) = prior_session {
            if sess.state != "playing"
                && (raw_view_offset_ms == 0 || (raw_view_offset_ms - sess.view_offset_ms).abs() < min_watch_ms)
            {
                return false;
            }
        }
    }

    if let Some(existing_off) = existing_scrobble_offset_ms {
        if existing_off > 0
            && (view_offset_val - existing_off).abs() < min_watch_ms
            && playing_elapsed_ms < min_watch_ms
        {
            return false;
        }
    } else if let Some(sess) = prior_session {
        if sess.view_offset_ms > 0
            && (view_offset_val - sess.view_offset_ms).abs() < min_watch_ms
            && playing_elapsed_ms < min_watch_ms
        {
            return false;
        }
    } else if last_paused_offset_ms > 0
        && (view_offset_val - last_paused_offset_ms).abs() < min_watch_ms
        && playing_elapsed_ms < min_watch_ms
    {
        return false;
    }

    true
}

async fn find_existing_webhook_scrobble(
    pool: &sqlx::SqlitePool,
    user_id: &str,
    media_type: &str,
    title: &str,
    grandparent_title: Option<&str>,
    season_number: Option<i32>,
    episode_number: Option<i32>,
    guid: Option<&str>,
    rating_key: Option<&str>,
    today_prefix: &str,
    six_hours_ago_str: &str,
) -> Option<ExistingScrobbleMatch> {
    if media_type == "episode" {
        sqlx::query_as::<_, ExistingScrobbleMatch>(
            r#"
            SELECT id, watch_status, progress_percent, view_offset_ms, duration_ms
            FROM scrobbles
            WHERE user_id = ?
              AND media_type = 'episode'
              AND (
                  (grandparent_title = ? AND season_number = ? AND episode_number = ?)
                  OR (guid IS NOT NULL AND guid = ?)
                  OR (rating_key IS NOT NULL AND rating_key = ?)
              )
              AND (
                  substr(watched_at, 1, 10) = ?
                  OR (watch_status = 'in_progress' AND watched_at >= ?)
              )
            ORDER BY watched_at DESC
            LIMIT 1
            "#,
        )
        .bind(user_id)
        .bind(grandparent_title)
        .bind(season_number)
        .bind(episode_number)
        .bind(guid)
        .bind(rating_key)
        .bind(today_prefix)
        .bind(six_hours_ago_str)
        .fetch_optional(pool)
        .await
        .unwrap_or(None)
    } else {
        sqlx::query_as::<_, ExistingScrobbleMatch>(
            r#"
            SELECT id, watch_status, progress_percent, view_offset_ms, duration_ms
            FROM scrobbles
            WHERE user_id = ?
              AND media_type = 'movie'
              AND (
                  title = ?
                  OR (guid IS NOT NULL AND guid = ?)
                  OR (rating_key IS NOT NULL AND rating_key = ?)
              )
              AND (
                  substr(watched_at, 1, 10) = ?
                  OR (watch_status = 'in_progress' AND watched_at >= ?)
              )
            ORDER BY watched_at DESC
            LIMIT 1
            "#,
        )
        .bind(user_id)
        .bind(title)
        .bind(guid)
        .bind(rating_key)
        .bind(today_prefix)
        .bind(six_hours_ago_str)
        .fetch_optional(pool)
        .await
        .unwrap_or(None)
    }
}

fn compute_existing_scrobble_progress(
    existing: &ExistingScrobbleMatch,
    incoming_duration_val: i64,
    incoming_offset_val: i64,
    incoming_progress_percent: i64,
    incoming_is_completed: bool,
    is_explicit_complete_event: bool,
    threshold: i64,
) -> (&'static str, i64, i64, i64) {
    let effective_duration = incoming_duration_val.max(existing.duration_ms.unwrap_or(0));
    let recalculated_pct = if effective_duration > 0 {
        ((incoming_offset_val as f64 / effective_duration as f64) * 100.0).round() as i64
    } else {
        0
    }
    .clamp(0, 100);

    let effective_completed = incoming_is_completed || (effective_duration > 0 && recalculated_pct >= threshold);
    let final_status = if existing.watch_status.as_deref() == Some("completed") || effective_completed {
        "completed"
    } else {
        "in_progress"
    };
    let final_progress = if is_explicit_complete_event {
        100.max(incoming_progress_percent).max(existing.progress_percent.unwrap_or(0))
    } else {
        incoming_progress_percent
            .max(recalculated_pct)
            .max(existing.progress_percent.unwrap_or(0))
    };
    let prev_offset = existing.view_offset_ms.unwrap_or(0);
    let final_offset = if final_status == "completed" && incoming_offset_val == 0 && effective_duration > 0 {
        if is_explicit_complete_event || prev_offset == 0 || (existing.watch_status.as_deref() != Some("completed") && final_progress >= 100) {
            effective_duration
        } else {
            prev_offset
        }
    } else {
        incoming_offset_val.max(prev_offset)
    };

    (final_status, final_progress, final_offset, effective_duration)
}

async fn backfill_missing_item_duration(
    pool: &sqlx::SqlitePool,
    user_id: &str,
    media_type: &str,
    title: &str,
    grandparent_title: Option<&str>,
    season_number: Option<i32>,
    episode_number: Option<i32>,
    rating_key: Option<&str>,
    guid: Option<&str>,
    duration_val: i64,
) {
    if duration_val <= 0 {
        return;
    }
    if media_type == "episode" {
        let gp = grandparent_title.unwrap_or("");
        let _ = sqlx::query(
            r#"
            UPDATE scrobbles
            SET duration_ms = ?,
                view_offset_ms = CASE
                    WHEN watch_status = 'completed' AND (view_offset_ms IS NULL OR view_offset_ms = 0) THEN ?
                    ELSE view_offset_ms
                END
            WHERE user_id = ?
              AND media_type = 'episode'
              AND (duration_ms IS NULL OR duration_ms = 0)
              AND (
                  (? != '' AND LOWER(TRIM(COALESCE(grandparent_title, ''))) = LOWER(TRIM(?)) AND season_number = ? AND episode_number = ?)
                  OR (guid IS NOT NULL AND guid = ?)
                  OR (rating_key IS NOT NULL AND rating_key = ?)
              )
            "#,
        )
        .bind(duration_val)
        .bind(duration_val)
        .bind(user_id)
        .bind(gp)
        .bind(gp)
        .bind(season_number)
        .bind(episode_number)
        .bind(guid)
        .bind(rating_key)
        .execute(pool)
        .await;
    } else {
        let _ = sqlx::query(
            r#"
            UPDATE scrobbles
            SET duration_ms = ?,
                view_offset_ms = CASE
                    WHEN watch_status = 'completed' AND (view_offset_ms IS NULL OR view_offset_ms = 0) THEN ?
                    ELSE view_offset_ms
                END
            WHERE user_id = ?
              AND media_type = 'movie'
              AND (duration_ms IS NULL OR duration_ms = 0)
              AND (
                  LOWER(TRIM(title)) = LOWER(TRIM(?))
                  OR (guid IS NOT NULL AND guid = ?)
                  OR (rating_key IS NOT NULL AND rating_key = ?)
              )
            "#,
        )
        .bind(duration_val)
        .bind(duration_val)
        .bind(user_id)
        .bind(title)
        .bind(guid)
        .bind(rating_key)
        .execute(pool)
        .await;
    }
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

    // Ignore pre-play trailers, cinema intros, clips, and movie extras
    if metadata.is_trailer_or_extra() {
        let title = metadata.title.clone().unwrap_or_else(|| "Unknown Trailer".to_string());
        state.log_buffer.push(
            "INFO",
            "bingesync::plex",
            &format!(
                "Ignored Plex trailer/clip for user {}: '{}' (type: {:?}, subtype: {:?})",
                user.email, title, metadata.media_type, metadata.subtype
            ),
        );
        tracing::info!(
            user_id = %user.id,
            title = %title,
            media_type = ?metadata.media_type,
            subtype = ?metadata.subtype,
            "Ignored Plex trailer, clip, or cinema intro"
        );
        return Ok((
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "ignored",
                "reason": "trailer_or_extra",
                "title": title,
                "type": metadata.media_type,
                "subtype": metadata.subtype,
            })),
        ));
    }

    let imdb_id = metadata.resolved_imdb_id();
    let tvdb_id = metadata.resolved_tvdb_id();
    let tmdb_id = metadata.resolved_tmdb_id();
    let (mut resolution, mut hdr_format, mut audio_codec, mut audio_channels, mut video_codec) = metadata.resolved_media_specs();
    let studio = metadata.studio.clone();
    let content_rating = metadata.content_rating.clone();
    let original_language = None::<String>;

    let mut duration_ms = metadata.resolved_duration();
    let mut view_offset_val = metadata.view_offset.unwrap_or(0).max(0);
    let rating_key = metadata.rating_key;
    let guid = metadata.guid;

    let title = metadata.title.unwrap_or_else(|| "Unknown Title".to_string());
    let media_type = metadata.media_type.unwrap_or_else(|| "movie".to_string());
    let grandparent_title = metadata.grandparent_title;
    let mut tmdb_id = tmdb_id;
    if media_type == "episode" && tmdb_id.is_none() {
        if let Some(ref gp) = grandparent_title {
            if let Ok(Some(cached_tmdb)) = sqlx::query_scalar::<_, String>(
                "SELECT tmdb_id FROM scrobbles WHERE user_id = ? AND grandparent_title IS NOT NULL AND LOWER(TRIM(grandparent_title)) = LOWER(TRIM(?)) AND tmdb_id IS NOT NULL AND tmdb_id != '' ORDER BY watched_at DESC LIMIT 1"
            )
            .bind(&user.id)
            .bind(gp)
            .fetch_optional(&state.pool)
            .await
            {
                tmdb_id = Some(cached_tmdb);
            }
        }
    }
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

    let is_scrobble_event = payload.event == "media.scrobble";
    let raw_view_offset_ms = view_offset_val;

    if rating_key.is_some() && (duration_ms.is_none() || (view_offset_val == 0 && !is_scrobble_event)) {
        if let Some(details) = fetch_plex_playback_details(
            &state.pool,
            &user.id,
            rating_key.as_deref(),
            account_title.as_deref(),
            account_id.as_deref(),
            &payload.event,
        )
        .await
        {
            if duration_ms.is_none() {
                duration_ms = details.duration_ms;
            }
            if view_offset_val == 0 {
                if let Some(vo) = details.view_offset_ms {
                    view_offset_val = vo;
                }
            }
            if resolution.is_none() {
                resolution = details.resolution;
            }
            if hdr_format.is_none() {
                hdr_format = details.hdr_format;
            }
            if audio_codec.is_none() {
                audio_codec = details.audio_codec;
            }
            if audio_channels.is_none() {
                audio_channels = details.audio_channels;
            }
            if video_codec.is_none() {
                video_codec = details.video_codec;
            }
        }
    }

    if duration_ms.is_none() {
        duration_ms = resolve_fallback_duration_ms(
            &state.pool,
            &user.id,
            &media_type,
            &title,
            grandparent_title.as_deref(),
            season_number,
            episode_number,
            rating_key.as_deref(),
            guid.as_deref(),
        )
        .await;
    }

    let duration_val = duration_ms.unwrap_or(0);
    let now = Utc::now();
    let now_str = now.to_rfc3339();
    let six_hours_ago_str = (now - chrono::Duration::hours(6)).to_rfc3339();

    let prior_session = fetch_active_session_for_item(
        &state.pool,
        &user.id,
        &media_type,
        &title,
        grandparent_title.as_deref(),
    )
    .await;
    let playing_elapsed_ms = active_session_playing_elapsed_ms(prior_session.as_ref(), &now_str);

    if view_offset_val == 0 && !is_scrobble_event && payload.event != "media.play" {
        view_offset_val = estimate_view_offset_from_session_snapshot(
            prior_session.as_ref(),
            playing_elapsed_ms,
            duration_val,
        );
    }

    let threshold = state.get_effective_scrobble_threshold().await;
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

    // Same-day or recent in-progress session de-duplication (handles cross-midnight playback)
    let today_prefix = if now_str.len() >= 10 { &now_str[..10] } else { "" };

    let existing = find_existing_webhook_scrobble(
        &state.pool,
        &user.id,
        &media_type,
        &title,
        grandparent_title.as_deref(),
        season_number,
        episode_number,
        guid.as_deref(),
        rating_key.as_deref(),
        today_prefix,
        &six_hours_ago_str,
    )
    .await;

    let last_paused_offset_ms = if !is_scrobble_event && existing.is_none() && prior_session.is_none() {
        fetch_last_paused_offset_ms(
            &state.pool,
            &user.id,
            &media_type,
            &title,
            grandparent_title.as_deref(),
            season_number,
            episode_number,
            tmdb_id.as_deref(),
            imdb_id.as_deref(),
            guid.as_deref(),
            rating_key.as_deref(),
        )
        .await
    } else {
        0
    };

    let is_pause_or_stop = matches!(payload.event.as_str(), "media.pause" | "media.stop");
    let should_scrobble = should_record_webhook_scrobble(
        is_scrobble_event,
        is_pause_or_stop,
        raw_view_offset_ms,
        view_offset_val,
        duration_val,
        prior_session.as_ref(),
        playing_elapsed_ms,
        existing.as_ref().and_then(|e| e.view_offset_ms),
        last_paused_offset_ms,
    );

    let player_name = payload.player.as_ref().and_then(|p| p.product.clone()).or_else(|| payload.player.as_ref().and_then(|p| p.title.clone()));
    let device_name = payload.player.as_ref().and_then(|p| p.device.clone().or_else(|| p.title.clone()));
    let platform = payload.player.as_ref().and_then(|p| p.platform.clone());
    let is_local = payload.player.as_ref().and_then(|p| p.local);

    let (scrobble_id, eff_status, eff_progress, eff_offset, eff_duration) = if !should_scrobble {
        (
            existing.as_ref().map(|e| e.id.clone()),
            existing
                .as_ref()
                .and_then(|e| e.watch_status.clone())
                .unwrap_or_else(|| watch_status.to_string()),
            progress_percent,
            view_offset_val,
            duration_val,
        )
    } else if let Some(existing) = existing {
        let (final_status, final_progress, final_offset, effective_duration) = compute_existing_scrobble_progress(
            &existing,
            duration_val,
            view_offset_val,
            progress_percent,
            is_completed,
            is_scrobble_event,
            threshold,
        );

        sqlx::query(
            r#"
            UPDATE scrobbles
            SET watch_status = ?,
                progress_percent = ?,
                view_offset_ms = ?,
                duration_ms = COALESCE(NULLIF(?, 0), duration_ms),
                watched_at = ?,
                thumb_url = COALESCE(thumb_url, ?),
                genres = COALESCE(genres, ?),
                directors = COALESCE(directors, ?),
                director = COALESCE(director, ?),
                actors = COALESCE(actors, ?),
                source = COALESCE(source, ?),
                source_user = COALESCE(source_user, ?),
                player_name = COALESCE(player_name, ?),
                device_name = COALESCE(device_name, ?),
                platform = COALESCE(platform, ?),
                is_local = COALESCE(is_local, ?),
                imdb_id = COALESCE(imdb_id, ?),
                resolution = COALESCE(resolution, ?),
                hdr_format = COALESCE(hdr_format, ?),
                audio_codec = COALESCE(audio_codec, ?),
                audio_channels = COALESCE(audio_channels, ?),
                video_codec = COALESCE(video_codec, ?),
                studio = COALESCE(studio, ?),
                content_rating = COALESCE(content_rating, ?),
                tvdb_id = COALESCE(NULLIF(?, ''), tvdb_id),
                tmdb_id = COALESCE(NULLIF(?, ''), tmdb_id)
            WHERE id = ?
            "#
        )
        .bind(final_status)
        .bind(final_progress)
        .bind(final_offset)
        .bind(effective_duration)
        .bind(&now_str)
        .bind(&thumb_url)
        .bind(&genres)
        .bind(&directors)
        .bind(&director)
        .bind(&actors)
        .bind(&source_name)
        .bind(&resolved_source_user)
        .bind(&player_name)
        .bind(&device_name)
        .bind(&platform)
        .bind(is_local)
        .bind(&imdb_id)
        .bind(&resolution)
        .bind(&hdr_format)
        .bind(&audio_codec)
        .bind(&audio_channels)
        .bind(&video_codec)
        .bind(&studio)
        .bind(&content_rating)
        .bind(&tvdb_id)
        .bind(&tmdb_id)
        .bind(&existing.id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to update scrobble: {}", e) })),
            )
        })?;

        (Some(existing.id), final_status.to_string(), final_progress, final_offset, effective_duration)
    } else {
        let is_rewatch = check_is_rewatch(
            &state.pool,
            &user.id,
            &media_type,
            &title,
            grandparent_title.as_deref(),
            season_number,
            episode_number,
            year,
            &now_str,
        )
        .await;
        let new_id = Uuid::new_v4().to_string();
        let initial_view_offset = if is_completed && view_offset_val == 0 && duration_val > 0 {
            duration_val
        } else {
            view_offset_val
        };
        let duration_to_insert = if duration_val > 0 { Some(duration_val) } else { None };
        let has_full_meta = genres.as_deref().map(|g| !g.is_empty()).unwrap_or(false)
            && overview.as_deref().map(|o| !o.is_empty()).unwrap_or(false);
        let meta_updated = if has_full_meta { Some(now_str.clone()) } else { None };

        let existing_rating = crate::api::history::find_existing_rating_for_item(
            &state.pool,
            &user.id,
            &media_type,
            &title,
            grandparent_title.as_deref(),
            season_number,
            episode_number,
            year,
            guid.as_deref(),
            tvdb_id.as_deref(),
            tmdb_id.as_deref(),
            imdb_id.as_deref(),
        )
        .await;

        let (existing_tags, existing_notes) = crate::api::history::find_existing_user_data_for_item(
            &state.pool,
            &user.id,
            &media_type,
            &title,
            grandparent_title.as_deref(),
            season_number,
            episode_number,
            year,
            guid.as_deref(),
            tvdb_id.as_deref(),
            tmdb_id.as_deref(),
            imdb_id.as_deref(),
        )
        .await;

        sqlx::query(
            r#"
            INSERT INTO scrobbles (
                id, user_id, title, media_type, grandparent_title, parent_title,
                season_number, episode_number, year, overview, thumb_url,
                rating_key, guid, duration_ms, user_rating,
                genres, directors, director, cinematographer, actors, watch_status, progress_percent, view_offset_ms,
                tags, notes,
                metadata_updated_at, source, source_user,
                player_name, device_name, platform, is_local,
                imdb_id, resolution, hdr_format, audio_codec, audio_channels, video_codec,
                studio, original_language, content_rating, is_rewatch,
                tvdb_id, tmdb_id,
                watched_at, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
        .bind(duration_to_insert)
        .bind(existing_rating)
        .bind(&genres)
        .bind(&directors)
        .bind(&director)
        .bind(&cinematographer)
        .bind(&actors)
        .bind(watch_status)
        .bind(progress_percent)
        .bind(initial_view_offset)
        .bind(&existing_tags)
        .bind(&existing_notes)
        .bind(meta_updated)
        .bind(&source_name)
        .bind(&resolved_source_user)
        .bind(&player_name)
        .bind(&device_name)
        .bind(&platform)
        .bind(is_local)
        .bind(&imdb_id)
        .bind(&resolution)
        .bind(&hdr_format)
        .bind(&audio_codec)
        .bind(&audio_channels)
        .bind(&video_codec)
        .bind(&studio)
        .bind(&original_language)
        .bind(&content_rating)
        .bind(is_rewatch)
        .bind(&tvdb_id)
        .bind(&tmdb_id)
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

        (Some(new_id), watch_status.to_string(), progress_percent, initial_view_offset, duration_val)
    };

    if should_scrobble && eff_duration > 0 {
        backfill_missing_item_duration(
            &state.pool,
            &user.id,
            &media_type,
            &title,
            grandparent_title.as_deref(),
            season_number,
            episode_number,
            rating_key.as_deref(),
            guid.as_deref(),
            eff_duration,
        )
        .await;
    }

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

            crate::api::outbound_webhooks::dispatch_notification(
                state.pool.clone(),
                user.id.clone(),
                crate::api::outbound_webhooks::NotificationData {
                    event: crate::api::outbound_webhooks::NotificationEvent::Scrobble,
                    title: title.clone(),
                    grandparent_title: grandparent_title.clone(),
                    media_type: media_type.clone(),
                    season_number,
                    episode_number,
                    year,
                    progress_percent: eff_progress,
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
                    "watch_status": eff_status,
                    "progress_percent": eff_progress,
                    "duration_ms": if eff_duration > 0 { Some(eff_duration) } else { None },
                    "view_offset_ms": eff_offset,
                })),
            ))
        }

        "media.play" | "media.resume" | "media.pause" => {
            let state_str = if payload.event == "media.pause" {
                "paused"
            } else {
                "playing"
            };

            let should_update_session = payload.event != "media.pause" || prior_session.is_some();
            if should_update_session {
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
                        player_name, device_name, platform, is_local,
                        resolution, hdr_format, audio_codec, audio_channels, video_codec, content_rating,
                        view_offset_ms, duration_ms, updated_at
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
                .bind(&player_name)
                .bind(&device_name)
                .bind(&platform)
                .bind(is_local)
                .bind(&resolution)
                .bind(&hdr_format)
                .bind(&audio_codec)
                .bind(&audio_channels)
                .bind(&video_codec)
                .bind(&content_rating)
                .bind(eff_offset)
                .bind(eff_duration)
                .bind(&now_str)
                .execute(&state.pool)
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": format!("Failed to update session: {}", e) })),
                    )
                })?;
            }

            if should_scrobble {
                let event_type = if payload.event == "media.pause" {
                    crate::api::outbound_webhooks::NotificationEvent::Stop
                } else {
                    crate::api::outbound_webhooks::NotificationEvent::Play
                };
                crate::api::outbound_webhooks::dispatch_notification(
                    state.pool.clone(),
                    user.id.clone(),
                    crate::api::outbound_webhooks::NotificationData {
                        event: event_type,
                        title: title.clone(),
                        grandparent_title: grandparent_title.clone(),
                        media_type: media_type.clone(),
                        season_number,
                        episode_number,
                        year,
                        progress_percent: eff_progress,
                        thumb_url: thumb_url.clone(),
                        user_rating: None,
                    },
                );
            }

            Ok((
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "session_updated",
                    "id": scrobble_id,
                    "state": state_str,
                    "title": title,
                    "watch_status": eff_status,
                    "progress_percent": eff_progress,
                    "duration_ms": if eff_duration > 0 { Some(eff_duration) } else { None },
                    "view_offset_ms": eff_offset,
                })),
            ))
        }

        "media.stop" => {
            let _ = sqlx::query("DELETE FROM active_sessions WHERE user_id = ?")
                .bind(&user.id)
                .execute(&state.pool)
                .await;

            if should_scrobble {
                crate::api::outbound_webhooks::dispatch_notification(
                    state.pool.clone(),
                    user.id.clone(),
                    crate::api::outbound_webhooks::NotificationData {
                        event: crate::api::outbound_webhooks::NotificationEvent::Stop,
                        title: title.clone(),
                        grandparent_title: grandparent_title.clone(),
                        media_type: media_type.clone(),
                        season_number,
                        episode_number,
                        year,
                        progress_percent: eff_progress,
                        thumb_url: thumb_url.clone(),
                        user_rating: None,
                    },
                );
            }

            Ok((
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "session_stopped",
                    "id": scrobble_id,
                    "title": title,
                    "watch_status": eff_status,
                    "progress_percent": eff_progress,
                    "duration_ms": if eff_duration > 0 { Some(eff_duration) } else { None },
                    "view_offset_ms": eff_offset,
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
    let is_movie = raw_type.eq_ignore_ascii_case("Movie");
    let is_trailer = raw_type.eq_ignore_ascii_case("Trailer")
        || raw_type.eq_ignore_ascii_case("Clip")
        || raw_type.eq_ignore_ascii_case("TrailerExtra");

    if is_trailer || (!is_episode && !is_movie) {
        let title = payload.resolved_name().unwrap_or_else(|| "Unknown Item".to_string());
        state.log_buffer.push(
            "INFO",
            &format!("bingesync::{}", source),
            &format!(
                "Ignored {} trailer/clip for user {}: '{}' (type: {})",
                if source == "emby" { "Emby" } else { "Jellyfin" },
                user.email,
                title,
                raw_type
            ),
        );
        return Ok((
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "ignored",
                "reason": "trailer_or_extra",
                "title": title,
                "type": raw_type,
            })),
        ));
    }

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

    let threshold = state.get_effective_scrobble_threshold().await;
    let lower_event = event_name.to_lowercase();
    let is_stop = lower_event.contains("stop") || lower_event.contains("scrobble") || lower_event.contains("markplayed");
    let is_start = lower_event.contains("start") || lower_event.contains("unpause") || lower_event.contains("resume");
    let is_pause = lower_event.contains("pause");
    let is_progress = lower_event.contains("progress");
    let is_saved = lower_event.contains("userdatasaved");

    let played_flag = payload.played_to_completion == Some(true) || payload.played == Some(true) || lower_event.contains("scrobble") || lower_event.contains("markplayed");

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

    // Jellyfin / Emby ticks: 10,000 ticks = 1 ms
    let mut duration_ms = payload.resolved_runtime_ticks().map(|t| t / 10_000).filter(|&d| d > 0);
    if duration_ms.is_none() {
        duration_ms = resolve_fallback_duration_ms(
            &state.pool,
            &user.id,
            &media_type,
            &title,
            grandparent_title.as_deref(),
            season_number,
            episode_number,
            rating_key.as_deref(),
            guid.as_deref(),
        )
        .await;
    }
    let duration_val = duration_ms.unwrap_or(0);
    let raw_view_offset_ms = payload.resolved_playback_position_ticks().map(|t| t / 10_000).unwrap_or(0).max(0);
    let mut view_offset_val = raw_view_offset_ms;
    let now = Utc::now();
    let now_str = now.to_rfc3339();
    let six_hours_ago_str = (now - chrono::Duration::hours(6)).to_rfc3339();
    let today_prefix = if now_str.len() >= 10 { &now_str[..10] } else { "" };

    let prior_session = fetch_active_session_for_item(
        &state.pool,
        &user.id,
        &media_type,
        &title,
        grandparent_title.as_deref(),
    )
    .await;
    let playing_elapsed_ms = active_session_playing_elapsed_ms(prior_session.as_ref(), &now_str);

    if view_offset_val == 0 && !played_flag && !lower_event.contains("start") {
        view_offset_val = estimate_view_offset_from_session_snapshot(
            prior_session.as_ref(),
            playing_elapsed_ms,
            duration_val,
        );
    }

    let calculated_pct = if duration_val > 0 {
        ((view_offset_val as f64 / duration_val as f64) * 100.0).round() as i64
    } else {
        0
    };

    let is_completed = played_flag || (is_stop && calculated_pct >= threshold) || (is_saved && played_flag);

    let progress_percent = if is_completed {
        100.max(calculated_pct)
    } else {
        calculated_pct.clamp(0, 100)
    };

    let watch_status = if is_completed { "completed" } else { "in_progress" };

    let existing = find_existing_webhook_scrobble(
        &state.pool,
        &user.id,
        &media_type,
        &title,
        grandparent_title.as_deref(),
        season_number,
        episode_number,
        guid.as_deref(),
        rating_key.as_deref(),
        today_prefix,
        &six_hours_ago_str,
    )
    .await;

    let player_name = payload.resolved_client();
    let device_name = payload.resolved_device_name();
    let platform = payload.resolved_platform();
    let is_local = payload.resolved_is_local();

    let imdb_id = payload.resolved_imdb_id();
    let tvdb_id = payload.resolved_tvdb_id();
    let mut tmdb_id = payload.resolved_tmdb_id();
    if is_episode && tmdb_id.is_none() {
        if let Some(ref gp) = grandparent_title {
            if let Ok(Some(cached_tmdb)) = sqlx::query_scalar::<_, String>(
                "SELECT tmdb_id FROM scrobbles WHERE user_id = ? AND grandparent_title IS NOT NULL AND LOWER(TRIM(grandparent_title)) = LOWER(TRIM(?)) AND tmdb_id IS NOT NULL AND tmdb_id != '' ORDER BY watched_at DESC LIMIT 1"
            )
            .bind(&user.id)
            .bind(gp)
            .fetch_optional(&state.pool)
            .await
            {
                tmdb_id = Some(cached_tmdb);
            }
        }
    }
    let (resolution, hdr_format, audio_codec, audio_channels, video_codec) = payload.resolved_media_specs();
    let studio = payload.resolved_studio();
    let original_language = payload.resolved_original_language();
    let content_rating = payload.resolved_content_rating();

    let last_paused_offset_ms = if !played_flag && existing.is_none() && prior_session.is_none() {
        fetch_last_paused_offset_ms(
            &state.pool,
            &user.id,
            &media_type,
            &title,
            grandparent_title.as_deref(),
            season_number,
            episode_number,
            tmdb_id.as_deref(),
            imdb_id.as_deref(),
            guid.as_deref(),
            rating_key.as_deref(),
        )
        .await
    } else {
        0
    };

    let is_pause_or_stop = is_pause || is_stop;
    let should_scrobble = should_record_webhook_scrobble(
        played_flag,
        is_pause_or_stop,
        raw_view_offset_ms,
        view_offset_val,
        duration_val,
        prior_session.as_ref(),
        playing_elapsed_ms,
        existing.as_ref().and_then(|e| e.view_offset_ms),
        last_paused_offset_ms,
    );

    let (scrobble_id, eff_status, eff_progress, eff_offset, eff_duration) = if !should_scrobble {
        (
            existing.as_ref().map(|e| e.id.clone()),
            existing
                .as_ref()
                .and_then(|e| e.watch_status.clone())
                .unwrap_or_else(|| watch_status.to_string()),
            progress_percent,
            view_offset_val,
            duration_val,
        )
    } else if let Some(existing) = existing {
        let (final_status, final_progress, final_offset, effective_duration) = compute_existing_scrobble_progress(
            &existing,
            duration_val,
            view_offset_val,
            progress_percent,
            is_completed,
            played_flag,
            threshold,
        );

        sqlx::query(
            r#"
            UPDATE scrobbles
            SET watch_status = ?,
                progress_percent = ?,
                view_offset_ms = ?,
                duration_ms = COALESCE(NULLIF(?, 0), duration_ms),
                watched_at = ?,
                thumb_url = COALESCE(thumb_url, ?),
                genres = COALESCE(genres, ?),
                source = COALESCE(source, ?),
                source_user = COALESCE(source_user, ?),
                player_name = COALESCE(player_name, ?),
                device_name = COALESCE(device_name, ?),
                platform = COALESCE(platform, ?),
                is_local = COALESCE(is_local, ?),
                imdb_id = COALESCE(imdb_id, ?),
                resolution = COALESCE(resolution, ?),
                hdr_format = COALESCE(hdr_format, ?),
                audio_codec = COALESCE(audio_codec, ?),
                audio_channels = COALESCE(audio_channels, ?),
                video_codec = COALESCE(video_codec, ?),
                studio = COALESCE(studio, ?),
                original_language = COALESCE(original_language, ?),
                content_rating = COALESCE(content_rating, ?),
                tvdb_id = COALESCE(NULLIF(?, ''), tvdb_id),
                tmdb_id = COALESCE(NULLIF(?, ''), tmdb_id)
            WHERE id = ?
            "#
        )
        .bind(final_status)
        .bind(final_progress)
        .bind(final_offset)
        .bind(effective_duration)
        .bind(&now_str)
        .bind(&thumb_url)
        .bind(&genres)
        .bind(&source_name)
        .bind(&resolved_source_user)
        .bind(&player_name)
        .bind(&device_name)
        .bind(&platform)
        .bind(is_local)
        .bind(&imdb_id)
        .bind(&resolution)
        .bind(&hdr_format)
        .bind(&audio_codec)
        .bind(&audio_channels)
        .bind(&video_codec)
        .bind(&studio)
        .bind(&original_language)
        .bind(&content_rating)
        .bind(&tvdb_id)
        .bind(&tmdb_id)
        .bind(&existing.id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to update scrobble: {}", e) })),
            )
        })?;

        (Some(existing.id), final_status.to_string(), final_progress, final_offset, effective_duration)
    } else {
        let is_rewatch = check_is_rewatch(
            &state.pool,
            &user.id,
            &media_type,
            &title,
            grandparent_title.as_deref(),
            season_number,
            episode_number,
            year,
            &now_str,
        )
        .await;
        let new_id = Uuid::new_v4().to_string();
        let initial_view_offset = if is_completed && view_offset_val == 0 && duration_val > 0 {
            duration_val
        } else {
            view_offset_val
        };
        let duration_to_insert = if duration_val > 0 { Some(duration_val) } else { None };
        let has_full_meta = genres.as_deref().map(|g| !g.is_empty()).unwrap_or(false)
            && overview.as_deref().map(|o| !o.is_empty()).unwrap_or(false);
        let meta_updated = if has_full_meta { Some(now_str.clone()) } else { None };

        let existing_rating = crate::api::history::find_existing_rating_for_item(
            &state.pool,
            &user.id,
            &media_type,
            &title,
            grandparent_title.as_deref(),
            season_number,
            episode_number,
            year,
            guid.as_deref(),
            tvdb_id.as_deref(),
            tmdb_id.as_deref(),
            imdb_id.as_deref(),
        )
        .await;

        let (existing_tags, existing_notes) = crate::api::history::find_existing_user_data_for_item(
            &state.pool,
            &user.id,
            &media_type,
            &title,
            grandparent_title.as_deref(),
            season_number,
            episode_number,
            year,
            guid.as_deref(),
            tvdb_id.as_deref(),
            tmdb_id.as_deref(),
            imdb_id.as_deref(),
        )
        .await;

        sqlx::query(
            r#"
            INSERT INTO scrobbles (
                id, user_id, title, media_type, grandparent_title, parent_title,
                season_number, episode_number, year, overview, thumb_url,
                rating_key, guid, duration_ms, user_rating,
                genres, directors, director, cinematographer, actors, watch_status, progress_percent, view_offset_ms,
                tags, notes,
                metadata_updated_at, source, source_user,
                player_name, device_name, platform, is_local,
                imdb_id, resolution, hdr_format, audio_codec, audio_channels, video_codec,
                studio, original_language, content_rating, is_rewatch,
                tvdb_id, tmdb_id,
                watched_at, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
        .bind(duration_to_insert)
        .bind(existing_rating)
        .bind(&genres)
        .bind(None::<String>) // directors
        .bind(None::<String>) // director
        .bind(None::<String>) // cinematographer
        .bind(None::<String>) // actors
        .bind(watch_status)
        .bind(progress_percent)
        .bind(initial_view_offset)
        .bind(&existing_tags)
        .bind(&existing_notes)
        .bind(meta_updated)
        .bind(&source_name)
        .bind(&resolved_source_user)
        .bind(&player_name)
        .bind(&device_name)
        .bind(&platform)
        .bind(is_local)
        .bind(&imdb_id)
        .bind(&resolution)
        .bind(&hdr_format)
        .bind(&audio_codec)
        .bind(&audio_channels)
        .bind(&video_codec)
        .bind(&studio)
        .bind(&original_language)
        .bind(&content_rating)
        .bind(is_rewatch)
        .bind(&tvdb_id)
        .bind(&tmdb_id)
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

        (Some(new_id), watch_status.to_string(), progress_percent, initial_view_offset, duration_val)
    };

    if should_scrobble && eff_duration > 0 {
        backfill_missing_item_duration(
            &state.pool,
            &user.id,
            &media_type,
            &title,
            grandparent_title.as_deref(),
            season_number,
            episode_number,
            rating_key.as_deref(),
            guid.as_deref(),
            eff_duration,
        )
        .await;
    }

    let eff_is_completed = should_scrobble && eff_status == "completed";

    if should_scrobble {
        if eff_is_completed {
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

            crate::api::outbound_webhooks::dispatch_notification(
                state.pool.clone(),
                user.id.clone(),
                crate::api::outbound_webhooks::NotificationData {
                    event: crate::api::outbound_webhooks::NotificationEvent::Scrobble,
                    title: title.clone(),
                    grandparent_title: grandparent_title.clone(),
                    media_type: media_type.clone(),
                    season_number,
                    episode_number,
                    year,
                    progress_percent: eff_progress,
                    thumb_url: thumb_url.clone(),
                    user_rating: None,
                },
            );
        } else if is_stop {
            crate::api::outbound_webhooks::dispatch_notification(
                state.pool.clone(),
                user.id.clone(),
                crate::api::outbound_webhooks::NotificationData {
                    event: crate::api::outbound_webhooks::NotificationEvent::Stop,
                    title: title.clone(),
                    grandparent_title: grandparent_title.clone(),
                    media_type: media_type.clone(),
                    season_number,
                    episode_number,
                    year,
                    progress_percent: eff_progress,
                    thumb_url: thumb_url.clone(),
                    user_rating: None,
                },
            );
        }
    }

    // 5. Handle active playback session state
    if is_stop || (is_saved && eff_is_completed) {
        let _ = sqlx::query("DELETE FROM active_sessions WHERE user_id = ?")
            .bind(&user.id)
            .execute(&state.pool)
            .await;

        let status_code = if eff_is_completed {
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
                "status": if eff_is_completed { "scrobbled" } else { "stopped" },
                "id": scrobble_id,
                "title": title,
                "media_type": media_type,
                "watched_at": now_str,
                "watch_status": eff_status,
                "progress_percent": eff_progress,
                "duration_ms": if eff_duration > 0 { Some(eff_duration) } else { None },
                "view_offset_ms": eff_offset,
            })),
        ))
    } else if is_start || is_progress || is_pause {
        if should_scrobble {
            if is_start {
                crate::api::outbound_webhooks::dispatch_notification(
                    state.pool.clone(),
                    user.id.clone(),
                    crate::api::outbound_webhooks::NotificationData {
                        event: crate::api::outbound_webhooks::NotificationEvent::Play,
                        title: title.clone(),
                        grandparent_title: grandparent_title.clone(),
                        media_type: media_type.clone(),
                        season_number,
                        episode_number,
                        year,
                        progress_percent: eff_progress,
                        thumb_url: thumb_url.clone(),
                        user_rating: None,
                    },
                );
            } else if is_pause {
                crate::api::outbound_webhooks::dispatch_notification(
                    state.pool.clone(),
                    user.id.clone(),
                    crate::api::outbound_webhooks::NotificationData {
                        event: crate::api::outbound_webhooks::NotificationEvent::Stop,
                        title: title.clone(),
                        grandparent_title: grandparent_title.clone(),
                        media_type: media_type.clone(),
                        season_number,
                        episode_number,
                        year,
                        progress_percent: eff_progress,
                        thumb_url: thumb_url.clone(),
                        user_rating: None,
                    },
                );
            }
        }
        let state_str = if is_pause { "paused" } else { "playing" };
        let should_update_session = !is_pause || prior_session.is_some();
        if should_update_session {
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
                    player_name, device_name, platform, is_local,
                    resolution, hdr_format, audio_codec, audio_channels, video_codec, content_rating,
                    view_offset_ms, duration_ms, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
            .bind(&player_name)
            .bind(&device_name)
            .bind(&platform)
            .bind(is_local)
            .bind(&resolution)
            .bind(&hdr_format)
            .bind(&audio_codec)
            .bind(&audio_channels)
            .bind(&video_codec)
            .bind(&content_rating)
            .bind(eff_offset)
            .bind(eff_duration)
            .bind(&now_str)
            .execute(&state.pool)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("Failed to update session: {}", e) })),
                )
            })?;
        }

        let status_code = if eff_is_completed {
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
                "status": if eff_is_completed { "scrobbled" } else { "session_updated" },
                "id": scrobble_id,
                "title": title,
                "media_type": media_type,
                "watch_status": eff_status,
                "progress_percent": eff_progress,
                "duration_ms": if eff_duration > 0 { Some(eff_duration) } else { None },
                "view_offset_ms": eff_offset,
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
                "watch_status": eff_status,
                "progress_percent": eff_progress,
                "duration_ms": if eff_duration > 0 { Some(eff_duration) } else { None },
                "view_offset_ms": eff_offset,
            })),
        ))
    }
}

