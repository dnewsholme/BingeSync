use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, NaiveDateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::{api::AppState, auth::AuthUser};

#[derive(Debug, Deserialize)]
pub struct TautulliUsersQuery {
    pub url: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TautulliImportPayload {
    pub url: Option<String>,
    pub api_key: Option<String>,
    pub username: Option<String>,
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TautulliUserItem {
    pub user_id: serde_json::Value,
    pub username: String,
    pub friendly_name: Option<String>,
    pub thumb: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TautulliWatchItem {
    pub title: String,
    pub media_type: String, // "movie" or "episode"
    pub grandparent_title: Option<String>,
    pub parent_title: Option<String>,
    pub season_number: Option<i32>,
    pub episode_number: Option<i32>,
    pub year: Option<i32>,
    pub watched_at: String, // RFC3339 UTC
    pub duration_ms: Option<i64>,
    pub rating_key: Option<String>,
    pub thumb_url: Option<String>,
    pub progress_percent: Option<i64>,
    pub user: Option<String>,
    pub friendly_name: Option<String>,
}

/// Fetch list of users from Tautulli API (for dropdown selector in UI)
pub async fn get_tautulli_users_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<TautulliUsersQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let url = match query.url.filter(|s| !s.trim().is_empty()) {
        Some(u) => u,
        None => match crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "tautulli_url").await {
            Some(u) if !u.trim().is_empty() => u,
            _ => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": "Tautulli URL is not configured. Please configure your Tautulli settings in your user Settings."
                    })),
                ));
            }
        },
    };

    crate::api::ssrf::validate_integration_url(&url).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("Invalid Tautulli URL: {}", e)
            })),
        )
    })?;

    let api_key = match query.api_key.filter(|s| !s.trim().is_empty()) {
        Some(k) => k,
        None => match crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "tautulli_api_key").await {
            Some(k) if !k.trim().is_empty() => k,
            _ => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": "Tautulli API Key is not configured. Please configure your Tautulli settings in your user Settings."
                    })),
                ));
            }
        },
    };

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("HTTP client error: {}", e) })),
            )
        })?;

    let (_endpoint, json_val) = fetch_tautulli_users_data(&client, &url, &api_key).await?;

    let mut users: Vec<TautulliUserItem> = Vec::new();
    if let Some(data_array) = json_val
        .get("response")
        .and_then(|r| r.get("data"))
        .and_then(|d| d.as_array())
    {
        for u in data_array {
            if let Some(username) = u.get("username").and_then(|un| un.as_str()) {
                let friendly_name = u.get("friendly_name").and_then(|f| f.as_str());
                let user_id_val = u.get("user_id").cloned().unwrap_or(serde_json::json!(0));
                let id_str = match &user_id_val {
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::String(s) => s.clone(),
                    _ => user_id_val.to_string(),
                };
                let _ = sqlx::query(
                    r#"
                    INSERT INTO plex_user_cache (plex_id, username, friendly_name, source, updated_at)
                    VALUES (?, ?, ?, 'tautulli', datetime('now'))
                    ON CONFLICT(plex_id) DO UPDATE SET username = excluded.username, friendly_name = excluded.friendly_name, updated_at = excluded.updated_at
                    "#
                )
                .bind(&id_str)
                .bind(username)
                .bind(friendly_name)
                .execute(&state.pool)
                .await;

                users.push(TautulliUserItem {
                    user_id: user_id_val,
                    username: username.to_string(),
                    friendly_name: friendly_name.map(|s| s.to_string()),
                    thumb: u.get("thumb").and_then(|t| t.as_str()).map(|s| s.to_string()),
                });
            }
        }
    }

    Ok(Json(serde_json::json!({ "users": users })))
}

/// Helper to sync Tautulli users into plex_user_cache table
pub async fn sync_tautulli_users_cache(pool: &sqlx::SqlitePool, url: &str, api_key: &str) -> Vec<(String, String)> {
    let client = match Client::builder().timeout(std::time::Duration::from_secs(5)).build() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let (_endpoint, json_val) = match fetch_tautulli_users_data(&client, url, api_key).await {
        Ok(res) => res,
        Err(_) => return Vec::new(),
    };
    let mut mapped = Vec::new();
    if let Some(data_array) = json_val
        .get("response")
        .and_then(|r| r.get("data"))
        .and_then(|d| d.as_array())
    {
        for u in data_array {
            if let Some(username) = u.get("username").and_then(|un| un.as_str()) {
                let friendly_name = u.get("friendly_name").and_then(|f| f.as_str());
                let id_str = match u.get("user_id") {
                    Some(serde_json::Value::Number(n)) => n.to_string(),
                    Some(serde_json::Value::String(s)) => s.clone(),
                    _ => continue,
                };
                let _ = sqlx::query(
                    r#"
                    INSERT INTO plex_user_cache (plex_id, username, friendly_name, source, updated_at)
                    VALUES (?, ?, ?, 'tautulli', datetime('now'))
                    ON CONFLICT(plex_id) DO UPDATE SET username = excluded.username, friendly_name = excluded.friendly_name, updated_at = excluded.updated_at
                    "#
                )
                .bind(&id_str)
                .bind(username)
                .bind(friendly_name)
                .execute(pool)
                .await;

                mapped.push((id_str, username.to_string()));
            }
        }
    }
    mapped
}

/// Main Tautulli import handler (API fetch or raw content, with deduplication)
pub async fn import_tautulli_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    body_bytes: axum::body::Bytes,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let (target_username, raw_content, api_url, api_key) = if content_type.starts_with("multipart/form-data") {
        let (uname, content) = parse_tautulli_multipart(content_type, body_bytes).await.map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Failed to parse multipart import: {}", e) })),
            )
        })?;
        (uname, Some(content), None, None)
    } else if let Ok(payload) = serde_json::from_slice::<TautulliImportPayload>(&body_bytes) {
        (payload.username, payload.content, payload.url, payload.api_key)
    } else {
        let body_str = String::from_utf8(body_bytes.to_vec()).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Invalid UTF-8 payload: {}", e) })),
            )
        })?;
        (None, Some(body_str), None, None)
    };

    let mut target_username = target_username;

    // Determine target username if not provided
    if target_username.as_deref().unwrap_or("").trim().is_empty() {
        // Fall back to current user's preferred_username from DB
        let pref_user: Option<String> = sqlx::query_scalar("SELECT preferred_username FROM users WHERE id = ?")
            .bind(&auth.user_id)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();
        target_username = pref_user.or_else(|| auth.email.split('@').next().map(|s| s.to_string()));
    }

    let username_filter = target_username.as_deref().unwrap_or("").trim().to_string();

    let mut items = if let Some(content) = raw_content.filter(|s| !s.trim().is_empty()) {
        let trimmed = content.trim();
        if trimmed.starts_with('[') || trimmed.starts_with('{') {
            parse_tautulli_json(trimmed, &username_filter)?
        } else {
            parse_tautulli_csv(trimmed, &username_filter)?
        }
    } else {
        // Direct Tautulli API sync
        let resolved_url = match api_url.filter(|s| !s.trim().is_empty()) {
            Some(u) => u,
            None => match crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "tautulli_url").await {
                Some(u) if !u.trim().is_empty() => u,
                _ => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({
                            "error": "Tautulli URL is not configured. Please configure your Tautulli settings in your user Settings."
                        })),
                    ));
                }
            },
        };

        crate::api::ssrf::validate_integration_url(&resolved_url).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("Invalid Tautulli URL: {}", e)
                })),
            )
        })?;

        let resolved_key = match api_key.filter(|s| !s.trim().is_empty()) {
            Some(k) => k,
            None => match crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "tautulli_api_key").await {
                Some(k) if !k.trim().is_empty() => k,
                _ => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({
                            "error": "Tautulli API Key is not configured. Please configure your Tautulli settings in your user Settings."
                        })),
                    ));
                }
            },
        };

        if username_filter.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Tautulli username must be specified to import history"
                })),
            ));
        }

        fetch_all_tautulli_history(&resolved_url, &resolved_key, &username_filter).await?
    };

    if items.is_empty() {
        return Ok(Json(serde_json::json!({
            "status": "success",
            "imported": 0,
            "skipped": 0,
            "total_processed": 0,
            "message": format!("No matching watch history found in Tautulli for user '{}'", username_filter)
        })));
    }

    // Sort items chronologically
    items.sort_by(|a, b| a.watched_at.cmp(&b.watched_at));

    // Deduplication against existing database records
    let mut tx = state.pool.begin().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Database transaction error: {}", e) })),
        )
    })?;

    #[derive(sqlx::FromRow)]
    struct ExistingScrobble {
        title: String,
        media_type: String,
        grandparent_title: Option<String>,
        season_number: Option<i32>,
        episode_number: Option<i32>,
        watched_at: String,
        rating_key: Option<String>,
    }

    let existing_records: Vec<ExistingScrobble> = sqlx::query_as(
        r#"
        SELECT title, media_type, grandparent_title, season_number, episode_number, watched_at, rating_key
        FROM scrobbles
        WHERE user_id = ?
        "#
    )
    .bind(&auth.user_id)
    .fetch_all(&mut *tx)
    .await
    .unwrap_or_default();

    let mut exact_set: HashSet<String> = HashSet::new();
    let mut day_set: HashSet<String> = HashSet::new();
    let mut rating_key_day_set: HashSet<String> = HashSet::new();

    for rec in existing_records {
        let day_prefix = if rec.watched_at.len() >= 10 { &rec.watched_at[..10] } else { "" };
        if rec.media_type == "episode" {
            let show = rec.grandparent_title.as_deref().unwrap_or("").to_lowercase().trim().to_string();
            let s = rec.season_number.unwrap_or(0);
            let ep = rec.episode_number.unwrap_or(0);
            exact_set.insert(format!("ep:{}:{}:{}:{}", show, s, ep, rec.watched_at));
            day_set.insert(format!("ep_day:{}:{}:{}:{}", show, s, ep, day_prefix));
        } else {
            let t = rec.title.to_lowercase().trim().to_string();
            exact_set.insert(format!("mv:{}:{}", t, rec.watched_at));
            day_set.insert(format!("mv_day:{}:{}", t, day_prefix));
        }
        if let Some(ref rk) = rec.rating_key {
            rating_key_day_set.insert(format!("rk_day:{}:{}", rk, day_prefix));
        }
    }

    let mut imported_count = 0;
    let mut skipped_count = 0;
    let now = Utc::now().to_rfc3339();

    for item in &items {
        let day_prefix = if item.watched_at.len() >= 10 { &item.watched_at[..10] } else { "" };
        let is_episode = item.media_type == "episode";

        let (exact_key, day_key) = if is_episode {
            let show = item.grandparent_title.as_deref().unwrap_or("").to_lowercase().trim().to_string();
            let s = item.season_number.unwrap_or(0);
            let ep = item.episode_number.unwrap_or(0);
            (
                format!("ep:{}:{}:{}:{}", show, s, ep, item.watched_at),
                format!("ep_day:{}:{}:{}:{}", show, s, ep, day_prefix),
            )
        } else {
            let t = item.title.to_lowercase().trim().to_string();
            (
                format!("mv:{}:{}", t, item.watched_at),
                format!("mv_day:{}:{}", t, day_prefix),
            )
        };

        let rk_key = item.rating_key.as_ref().map(|rk| format!("rk_day:{}:{}", rk, day_prefix));

        // Deduplication check:
        // 1. Exact match on title/show, season, episode, watched_at
        // 2. Same-day match on title/show, season, episode
        // 3. Same-day match on Plex rating_key
        let is_dup = exact_set.contains(&exact_key)
            || day_set.contains(&day_key)
            || (rk_key.as_ref().map(|k| rating_key_day_set.contains(k)).unwrap_or(false));

        if is_dup {
            skipped_count += 1;
            continue;
        }

        let scrobble_id = Uuid::new_v4().to_string();
        let insert_res = sqlx::query(
            r#"
            INSERT INTO scrobbles (
                id, user_id, title, media_type, grandparent_title, parent_title,
                season_number, episode_number, year, overview, thumb_url,
                rating_key, guid, duration_ms, user_rating,
                genres, directors, director, cinematographer, actors, watch_status, progress_percent, view_offset_ms,
                watched_at, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&scrobble_id)
        .bind(&auth.user_id)
        .bind(&item.title)
        .bind(&item.media_type)
        .bind(&item.grandparent_title)
        .bind(&item.parent_title)
        .bind(item.season_number)
        .bind(item.episode_number)
        .bind(item.year)
        .bind(None::<String>)
        .bind(&item.thumb_url)
        .bind(&item.rating_key)
        .bind(None::<String>)
        .bind(item.duration_ms)
        .bind(None::<i32>)
        .bind(None::<String>)
        .bind(None::<String>) // directors
        .bind(None::<String>) // director
        .bind(None::<String>) // cinematographer
        .bind(None::<String>) // actors
        .bind("completed")
        .bind(item.progress_percent.unwrap_or(100))
        .bind(item.duration_ms.unwrap_or(0))
        .bind(&item.watched_at)
        .bind(&now)
        .execute(&mut *tx)
        .await;

        match insert_res {
            Ok(_) => {
                imported_count += 1;
                exact_set.insert(exact_key);
                day_set.insert(day_key);
                if let Some(rk) = rk_key {
                    rating_key_day_set.insert(rk);
                }
            }
            Err(e) => {
                tracing::error!("Failed to insert Tautulli scrobble '{}': {:?}", item.title, e);
                state.log_buffer.push(
                    "ERROR",
                    "bingesync::tautulli",
                    &format!("Failed to insert scrobble '{}': {}", item.title, e),
                );
                skipped_count += 1;
            }
        }
    }

    tx.commit().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to commit scrobbles: {}", e) })),
        )
    })?;

    // Trigger background poster caching and TheTVDB enrichment workers
    state.image_notify.notify_one();
    state.metadata_notify.notify_one();

    state.log_buffer.push(
        "INFO",
        "bingesync::tautulli",
        &format!(
            "Tautulli import completed for user {}: {} imported, {} duplicates skipped",
            auth.email, imported_count, skipped_count
        ),
    );

    Ok(Json(serde_json::json!({
        "status": "success",
        "imported": imported_count,
        "skipped": skipped_count,
        "total_processed": items.len(),
        "message": format!(
            "Successfully imported {} watches from Tautulli ({} duplicates skipped)",
            imported_count, skipped_count
        )
    })))
}

/// Helper to call Tautulli endpoint and fetch users data using the exact configured path
async fn fetch_tautulli_users_data(
    client: &Client,
    raw_url: &str,
    api_key: &str,
) -> Result<(String, serde_json::Value), (StatusCode, Json<serde_json::Value>)> {
    let base = raw_url.trim().trim_end_matches('/');
    let endpoint = if base.ends_with("/api/v2") {
        base.to_string()
    } else if base.ends_with("/api") {
        format!("{}/v2", base)
    } else {
        format!("{}/api/v2", base)
    };

    let target_url = if endpoint.contains('?') {
        format!("{}&apikey={}&cmd=get_users", endpoint, api_key.trim())
    } else {
        format!("{}?apikey={}&cmd=get_users", endpoint, api_key.trim())
    };

    let resp = client.get(&target_url).send().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({
                "error": format!("Failed to connect to Tautulli at {}: {}", endpoint, e)
            })),
        )
    })?;

    let status = resp.status();
    if status.is_success() {
        let json_val = resp.json::<serde_json::Value>().await.map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "error": format!("Failed to parse Tautulli response JSON from {}: {}", endpoint, e)
                })),
            )
        })?;

        if let Some(res) = json_val.get("response").and_then(|r| r.get("result")).and_then(|s| s.as_str()) {
            if res == "error" {
                let msg = json_val.get("response")
                    .and_then(|r| r.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("Tautulli returned an error");
                return Err((
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({ "error": format!("Tautulli error: {}", msg) })),
                ));
            }
        }
        return Ok((endpoint, json_val));
    }

    let body = resp.text().await.unwrap_or_default();
    Err((
        StatusCode::BAD_GATEWAY,
        Json(serde_json::json!({
            "error": format!("Tautulli server responded with status {}: {}", status, body.chars().take(200).collect::<String>())
        })),
    ))
}

async fn resolve_tautulli_api_endpoint(
    client: &Client,
    raw_url: &str,
    api_key: &str,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    let (endpoint, _) = fetch_tautulli_users_data(client, raw_url, api_key).await?;
    Ok(endpoint)
}

/// Fetch history pages from Tautulli API until exhausted
async fn fetch_all_tautulli_history(
    url: &str,
    api_key: &str,
    username: &str,
) -> Result<Vec<TautulliWatchItem>, (StatusCode, Json<serde_json::Value>)> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("HTTP client error: {}", e) })),
            )
        })?;

    let endpoint = resolve_tautulli_api_endpoint(&client, url, api_key).await?;
    let mut all_items: Vec<TautulliWatchItem> = Vec::new();
    let page_size = 1000;
    let mut start = 0;

    loop {
        let encoded_username: String = url::form_urlencoded::byte_serialize(username.as_bytes()).collect();
        let target_url = format!(
            "{}?apikey={}&cmd=get_history&user={}&length={}&start={}",
            endpoint,
            api_key.trim(),
            encoded_username,
            page_size,
            start
        );

        let resp = client.get(&target_url).send().await.map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("Failed to query Tautulli history: {}", e) })),
            )
        })?;

        if !resp.status().is_success() {
            return Err((
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "error": format!("Tautulli returned error status: {}", resp.status())
                })),
            ));
        }

        let json_body: serde_json::Value = resp.json().await.map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("Invalid JSON response from Tautulli: {}", e) })),
            )
        })?;

        let data_array = json_body
            .get("response")
            .and_then(|r| r.get("data"))
            .and_then(|d| d.get("data"))
            .and_then(|arr| arr.as_array());

        let rows = match data_array {
            Some(arr) if !arr.is_empty() => arr,
            _ => break,
        };

        let count_in_page = rows.len();
        for row in rows {
            if let Some(item) = parse_tautulli_row_value(row, username) {
                all_items.push(item);
            }
        }

        if count_in_page < page_size {
            break;
        }
        start += page_size;
    }

    Ok(all_items)
}

/// Parses Tautulli JSON export
fn parse_tautulli_json(
    raw: &str,
    username_filter: &str,
) -> Result<Vec<TautulliWatchItem>, (StatusCode, Json<serde_json::Value>)> {
    let root: serde_json::Value = serde_json::from_str(raw).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("Failed to parse Tautulli JSON: {}", e) })),
        )
    })?;

    let array_opt = if let Some(arr) = root.as_array() {
        Some(arr)
    } else if let Some(arr) = root.get("data").and_then(|d| d.as_array()) {
        Some(arr)
    } else if let Some(arr) = root
        .get("response")
        .and_then(|r| r.get("data"))
        .and_then(|d| d.get("data"))
        .and_then(|arr| arr.as_array())
    {
        Some(arr)
    } else {
        None
    };

    let arr = match array_opt {
        Some(a) => a,
        None => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "No valid data array found in Tautulli JSON" })),
            ));
        }
    };

    let mut items = Vec::new();
    for row in arr {
        if let Some(item) = parse_tautulli_row_value(row, username_filter) {
            items.push(item);
        }
    }

    Ok(items)
}

fn parse_tautulli_row_value(row: &serde_json::Value, username_filter: &str) -> Option<TautulliWatchItem> {
    let row_user = row.get("user").and_then(|u| u.as_str());
    let row_friendly = row.get("friendly_name").and_then(|f| f.as_str());

    if !username_filter.is_empty() {
        let match_user = row_user.map(|u| u.eq_ignore_ascii_case(username_filter)).unwrap_or(false);
        let match_friendly = row_friendly.map(|f| f.eq_ignore_ascii_case(username_filter)).unwrap_or(false);
        if !match_user && !match_friendly {
            return None;
        }
    }

    let media_type = row.get("media_type").and_then(|m| m.as_str()).unwrap_or("movie");
    if media_type != "movie" && media_type != "episode" {
        return None;
    }

    let title = row.get("title").and_then(|t| t.as_str()).unwrap_or("").trim().to_string();
    if title.is_empty() {
        return None;
    }

    let raw_date = row.get("date").or_else(|| row.get("stopped")).or_else(|| row.get("started"));
    let watched_at = match raw_date {
        Some(v) if v.is_i64() => {
            let ts = v.as_i64().unwrap();
            DateTime::from_timestamp(ts, 0).map(|dt| dt.to_rfc3339())?
        }
        Some(v) if v.is_string() => {
            let s = v.as_str().unwrap();
            if let Ok(ts) = s.parse::<i64>() {
                DateTime::from_timestamp(ts, 0).map(|dt| dt.to_rfc3339())?
            } else {
                normalize_timestamp(s)
            }
        }
        _ => Utc::now().to_rfc3339(),
    };

    let full_title = row.get("full_title").and_then(|f| f.as_str()).map(|s| s.to_string());
    let mut grandparent_title = row.get("grandparent_title").and_then(|g| g.as_str()).map(|s| s.to_string());
    let parent_title = row.get("parent_title").and_then(|p| p.as_str()).map(|s| s.to_string());

    if media_type == "episode" && grandparent_title.is_none() {
        if let Some(ref ft) = full_title {
            if let Some(idx) = ft.find(" - ") {
                grandparent_title = Some(ft[..idx].trim().to_string());
            }
        }
    }

    let year = row.get("year").and_then(|y| {
        if let Some(n) = y.as_i64() {
            Some(n as i32)
        } else if let Some(s) = y.as_str() {
            s.parse::<i32>().ok()
        } else {
            None
        }
    });

    let season_number = row.get("parent_media_index").and_then(|s| {
        if let Some(n) = s.as_i64() {
            Some(n as i32)
        } else if let Some(str_val) = s.as_str() {
            str_val.parse::<i32>().ok()
        } else {
            None
        }
    });

    let episode_number = row.get("media_index").and_then(|e| {
        if let Some(n) = e.as_i64() {
            Some(n as i32)
        } else if let Some(str_val) = e.as_str() {
            str_val.parse::<i32>().ok()
        } else {
            None
        }
    });

    let duration_ms = row.get("duration").and_then(|d| {
        if let Some(n) = d.as_i64() {
            // Tautulli duration is in seconds
            Some(n * 1000)
        } else if let Some(s) = d.as_str() {
            s.parse::<i64>().ok().map(|n| n * 1000)
        } else {
            None
        }
    });

    let rating_key = row.get("rating_key").map(|r| {
        if let Some(n) = r.as_i64() {
            n.to_string()
        } else if let Some(s) = r.as_str() {
            s.to_string()
        } else {
            r.to_string()
        }
    });

    let thumb_url = row.get("thumb")
        .or_else(|| row.get("grandparent_thumb"))
        .or_else(|| row.get("parent_thumb"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string());

    let progress_percent = row.get("percent_complete").and_then(|p| {
        if let Some(n) = p.as_i64() {
            Some(n)
        } else if let Some(s) = p.as_str() {
            s.parse::<i64>().ok()
        } else {
            None
        }
    });

    Some(TautulliWatchItem {
        title,
        media_type: media_type.to_string(),
        grandparent_title,
        parent_title,
        season_number,
        episode_number,
        year,
        watched_at,
        duration_ms,
        rating_key,
        thumb_url,
        progress_percent,
        user: row_user.map(|s| s.to_string()),
        friendly_name: row_friendly.map(|s| s.to_string()),
    })
}

/// Parses Tautulli CSV export
fn parse_tautulli_csv(
    csv_text: &str,
    username_filter: &str,
) -> Result<Vec<TautulliWatchItem>, (StatusCode, Json<serde_json::Value>)> {
    let mut lines = csv_text.lines().filter(|l| !l.trim().is_empty());
    let header_line = match lines.next() {
        Some(h) => h,
        None => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "CSV file is empty" })),
            ));
        }
    };

    let headers: Vec<String> = parse_csv_line(header_line)
        .into_iter()
        .map(|h| h.to_lowercase().trim().to_string())
        .collect();

    let col_map: HashMap<String, usize> = headers
        .into_iter()
        .enumerate()
        .map(|(i, h)| (h, i))
        .collect();

    let mut items = Vec::new();

    for line in lines {
        let fields = parse_csv_line(line);
        if fields.is_empty() {
            continue;
        }

        let get_field = |names: &[&str]| -> Option<String> {
            for name in names {
                if let Some(&idx) = col_map.get(*name) {
                    if idx < fields.len() {
                        let val = fields[idx].trim();
                        if !val.is_empty() {
                            return Some(val.to_string());
                        }
                    }
                }
            }
            None
        };

        let row_user = get_field(&["user", "username"]);
        let row_friendly = get_field(&["friendly_name"]);

        if !username_filter.is_empty() {
            let match_user = row_user.as_deref().map(|u| u.eq_ignore_ascii_case(username_filter)).unwrap_or(false);
            let match_friendly = row_friendly.as_deref().map(|f| f.eq_ignore_ascii_case(username_filter)).unwrap_or(false);
            if !match_user && !match_friendly {
                continue;
            }
        }

        let media_type = get_field(&["media_type"]).unwrap_or_else(|| "movie".to_string()).to_lowercase();
        if media_type != "movie" && media_type != "episode" {
            continue;
        }

        let title = match get_field(&["title"]) {
            Some(t) => t,
            None => continue,
        };

        let raw_date = get_field(&["date", "stopped", "started"]);
        let watched_at = match raw_date {
            Some(s) => {
                if let Ok(ts) = s.parse::<i64>() {
                    DateTime::from_timestamp(ts, 0).map(|dt| dt.to_rfc3339()).unwrap_or_else(|| Utc::now().to_rfc3339())
                } else {
                    normalize_timestamp(&s)
                }
            }
            None => Utc::now().to_rfc3339(),
        };

        let full_title = get_field(&["full_title"]);
        let mut grandparent_title = get_field(&["grandparent_title", "show"]);
        let parent_title = get_field(&["parent_title", "season"]);

        if media_type == "episode" && grandparent_title.is_none() {
            if let Some(ref ft) = full_title {
                if let Some(idx) = ft.find(" - ") {
                    grandparent_title = Some(ft[..idx].trim().to_string());
                }
            }
        }

        let year = get_field(&["year"]).and_then(|y| y.parse::<i32>().ok());
        let season_number = get_field(&["parent_media_index", "season_num"]).and_then(|s| s.parse::<i32>().ok());
        let episode_number = get_field(&["media_index", "episode_num"]).and_then(|e| e.parse::<i32>().ok());

        let duration_ms = get_field(&["duration"]).and_then(|d| d.parse::<i64>().ok()).map(|s| s * 1000);
        let rating_key = get_field(&["rating_key"]);
        let thumb_url = get_field(&["thumb", "grandparent_thumb", "parent_thumb"]);
        let progress_percent = get_field(&["percent_complete"]).and_then(|p| p.parse::<i64>().ok());

        items.push(TautulliWatchItem {
            title,
            media_type,
            grandparent_title,
            parent_title,
            season_number,
            episode_number,
            year,
            watched_at,
            duration_ms,
            rating_key,
            thumb_url,
            progress_percent,
            user: row_user,
            friendly_name: row_friendly,
        });
    }

    Ok(items)
}

fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '"' => {
                if in_quotes && chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = !in_quotes;
                }
            }
            ',' if !in_quotes => {
                fields.push(cur.trim().to_string());
                cur = String::new();
            }
            _ => {
                cur.push(c);
            }
        }
    }
    fields.push(cur.trim().to_string());
    fields
}

fn normalize_timestamp(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return dt.with_timezone(&Utc).to_rfc3339();
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S") {
        return DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc).to_rfc3339();
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d") {
        return DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc).to_rfc3339();
    }
    Utc::now().to_rfc3339()
}

async fn parse_tautulli_multipart(
    content_type: &str,
    body_bytes: axum::body::Bytes,
) -> Result<(Option<String>, String), String> {
    let boundary = multer::parse_boundary(content_type)
        .map_err(|e| format!("Failed to parse boundary: {}", e))?;

    let cursor = std::io::Cursor::new(body_bytes);
    let stream = tokio_util::io::ReaderStream::new(cursor);
    let mut multipart = multer::Multipart::new(stream, boundary);

    let mut username = None;
    let mut file_content = String::new();

    while let Some(field) = multipart.next_field().await.map_err(|e| e.to_string())? {
        let name = field.name().unwrap_or("").to_string();
        if name == "username" || name == "user" {
            username = Some(field.text().await.map_err(|e| e.to_string())?);
        } else if name == "file" || name == "payload" || name == "content" {
            file_content = field.text().await.map_err(|e| e.to_string())?;
        }
    }

    Ok((username, file_content))
}
