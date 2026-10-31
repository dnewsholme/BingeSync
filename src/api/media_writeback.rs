use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::time::Duration;

use crate::{api::AppState, auth::AuthUser};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WritebackItem {
    pub title: String,
    pub media_type: String, // "movie" or "episode"
    pub grandparent_title: Option<String>,
    pub season_number: Option<i32>,
    pub episode_number: Option<i32>,
    pub year: Option<i32>,
    pub rating_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TriggerWritebackRequest {
    pub scrobble_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TestPlexRequest {
    pub plex_url: Option<String>,
    pub plex_api_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TestJellyfinRequest {
    pub jellyfin_url: Option<String>,
    pub jellyfin_api_key: Option<String>,
    pub jellyfin_username: Option<String>,
}

/// Tests connection to Plex Media Server using X-Plex-Token
pub async fn test_plex_connection(plex_url: &str, plex_token: &str) -> Result<String, String> {
    let base = plex_url.trim_end_matches('/');
    crate::api::ssrf::validate_integration_url(base).map_err(|e| format!("Invalid Plex URL: {}", e))?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;

    let url = format!("{}/identity", base);
    let resp = client
        .get(&url)
        .header("X-Plex-Token", plex_token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Could not reach Plex server: {}", e))?;

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err("Authentication failed: invalid Plex token".to_string());
    }

    if !status.is_success() {
        return Err(format!("Plex returned error HTTP {}", status));
    }

    let body = resp.json::<serde_json::Value>().await.ok();
    let version = body
        .as_ref()
        .and_then(|v| v.get("MediaContainer"))
        .and_then(|mc| mc.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    Ok(format!("Successfully connected to Plex Media Server (v{})", version))
}

/// Tests connection to Jellyfin / Emby server using X-Emby-Token
pub async fn test_jellyfin_connection(jellyfin_url: &str, api_key: &str) -> Result<String, String> {
    let base = jellyfin_url.trim_end_matches('/');
    crate::api::ssrf::validate_integration_url(base).map_err(|e| format!("Invalid Jellyfin URL: {}", e))?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;

    let url = format!("{}/System/Info", base);
    let resp = client
        .get(&url)
        .header("X-Emby-Token", api_key)
        .header("Authorization", format!("MediaBrowser Token=\"{}\"", api_key))
        .send()
        .await
        .map_err(|e| format!("Could not reach Jellyfin server: {}", e))?;

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err("Authentication failed: invalid Jellyfin API key".to_string());
    }

    if !status.is_success() {
        return Err(format!("Jellyfin returned error HTTP {}", status));
    }

    let body = resp.json::<serde_json::Value>().await.ok();
    let server_name = body
        .as_ref()
        .and_then(|v| v.get("ServerName"))
        .and_then(|v| v.as_str())
        .unwrap_or("Jellyfin");
    let version = body
        .as_ref()
        .and_then(|v| v.get("Version"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    Ok(format!("Successfully connected to {} (v{})", server_name, version))
}

/// Marks an item as watched in Plex Media Server
pub async fn writeback_item_to_plex(
    client: &reqwest::Client,
    plex_url: &str,
    plex_token: &str,
    item: &WritebackItem,
) -> Result<bool, String> {
    let base = plex_url.trim_end_matches('/');
    if let Err(e) = crate::api::ssrf::validate_integration_url(base) {
        return Err(format!("Invalid Plex URL: {}", e));
    }

    // 1. If we have a direct numeric rating_key from Plex, attempt direct scrobble
    if let Some(ref rkey) = item.rating_key {
        if rkey.chars().all(|c| c.is_ascii_digit()) {
            let scrobble_url = format!("{}/:/scrobble?key={}&identifier=com.plexapp.plugins.library", base, rkey);
            let resp = client
                .get(&scrobble_url)
                .header("X-Plex-Token", plex_token)
                .send()
                .await;

            if let Ok(r) = resp {
                if r.status().is_success() {
                    return Ok(true);
                }
            }
        }
    }

    // 2. Search Plex library for matching item
    let is_episode = item.media_type.eq_ignore_ascii_case("episode");
    let query_term = if is_episode {
        item.grandparent_title.as_deref().unwrap_or(&item.title)
    } else {
        &item.title
    };

    let search_url = format!("{}/library/all", base);
    let search_resp = client
        .get(&search_url)
        .header("X-Plex-Token", plex_token)
        .header("Accept", "application/json")
        .query(&[("query", query_term)])
        .send()
        .await
        .map_err(|e| format!("Plex search failed: {}", e))?;

    if !search_resp.status().is_success() {
        return Ok(false);
    }

    let search_json = search_resp
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Plex response parse error: {}", e))?;

    let metadata_items = search_json
        .get("MediaContainer")
        .and_then(|mc| mc.get("Metadata"))
        .and_then(|m| m.as_array());

    let mut found_rating_key: Option<String> = None;

    if let Some(meta_list) = metadata_items {
        if is_episode {
            // Check direct episode matches first
            for entry in meta_list {
                let entry_type = entry.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if entry_type == "episode" {
                    let parent_index = entry.get("parentIndex").and_then(|v| v.as_i64()).map(|v| v as i32);
                    let index = entry.get("index").and_then(|v| v.as_i64()).map(|v| v as i32);

                    if item.episode_number == index && (item.season_number.is_none() || item.season_number == parent_index) {
                        if let Some(rk) = entry.get("ratingKey").and_then(|k| k.as_str()) {
                            found_rating_key = Some(rk.to_string());
                            break;
                        }
                    }
                }
            }

            // If not found in direct episode results, search show's allLeaves
            if found_rating_key.is_none() {
                for entry in meta_list {
                    let entry_type = entry.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    if entry_type == "show" {
                        let show_rk = entry.get("ratingKey").and_then(|k| k.as_str());
                        if let Some(srk) = show_rk {
                            let leaves_url = format!("{}/library/metadata/{}/allLeaves", base, srk);
                            let leaves_resp = client
                                .get(&leaves_url)
                                .header("X-Plex-Token", plex_token)
                                .header("Accept", "application/json")
                                .send()
                                .await;

                            if let Ok(lresp) = leaves_resp {
                                if let Ok(lj) = lresp.json::<serde_json::Value>().await {
                                    if let Some(leaves) = lj.get("MediaContainer").and_then(|mc| mc.get("Metadata")).and_then(|m| m.as_array()) {
                                        for ep in leaves {
                                            let parent_index = ep.get("parentIndex").and_then(|v| v.as_i64()).map(|v| v as i32);
                                            let index = ep.get("index").and_then(|v| v.as_i64()).map(|v| v as i32);
                                            if item.episode_number == index && (item.season_number.is_none() || item.season_number == parent_index) {
                                                if let Some(rk) = ep.get("ratingKey").and_then(|k| k.as_str()) {
                                                    found_rating_key = Some(rk.to_string());
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if found_rating_key.is_some() {
                        break;
                    }
                }
            }
        } else {
            // Movie matching
            for entry in meta_list {
                let entry_type = entry.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if entry_type == "movie" {
                    let entry_title = entry.get("title").and_then(|t| t.as_str()).unwrap_or("");
                    if entry_title.eq_ignore_ascii_case(&item.title) {
                        if let Some(rk) = entry.get("ratingKey").and_then(|k| k.as_str()) {
                            found_rating_key = Some(rk.to_string());
                            break;
                        }
                    }
                }
            }
        }
    }

    if let Some(rkey) = found_rating_key {
        let scrobble_url = format!("{}/:/scrobble?key={}&identifier=com.plexapp.plugins.library", base, rkey);
        let resp = client
            .get(&scrobble_url)
            .header("X-Plex-Token", plex_token)
            .send()
            .await
            .map_err(|e| format!("Plex scrobble request failed: {}", e))?;

        Ok(resp.status().is_success())
    } else {
        Ok(false)
    }
}

/// Marks an item as watched in Jellyfin / Emby server
pub async fn writeback_item_to_jellyfin(
    client: &reqwest::Client,
    jellyfin_url: &str,
    api_key: &str,
    username: Option<&str>,
    item: &WritebackItem,
) -> Result<bool, String> {
    let base = jellyfin_url.trim_end_matches('/');
    if let Err(e) = crate::api::ssrf::validate_integration_url(base) {
        return Err(format!("Invalid Jellyfin URL: {}", e));
    }

    // 1. Discover user ID
    let users_url = format!("{}/Users", base);
    let users_resp = client
        .get(&users_url)
        .header("X-Emby-Token", api_key)
        .header("Authorization", format!("MediaBrowser Token=\"{}\"", api_key))
        .send()
        .await
        .map_err(|e| format!("Could not reach Jellyfin users endpoint: {}", e))?;

    if !users_resp.status().is_success() {
        return Err(format!("Jellyfin Users endpoint returned HTTP {}", users_resp.status()));
    }

    let users_list = users_resp
        .json::<Vec<serde_json::Value>>()
        .await
        .unwrap_or_default();

    let mut target_user_id: Option<String> = None;
    if let Some(uname) = username {
        for u in &users_list {
            if let Some(name) = u.get("Name").and_then(|n| n.as_str()) {
                if name.eq_ignore_ascii_case(uname) {
                    if let Some(id) = u.get("Id").and_then(|i| i.as_str()) {
                        target_user_id = Some(id.to_string());
                        break;
                    }
                }
            }
        }
    }

    if target_user_id.is_none() {
        if let Some(first) = users_list.first() {
            target_user_id = first.get("Id").and_then(|i| i.as_str()).map(|s| s.to_string());
        }
    }

    let user_id = match target_user_id {
        Some(id) => id,
        None => return Err("No Jellyfin user found to mark watched item for".to_string()),
    };

    // 2. Search for the item in Jellyfin
    let is_episode = item.media_type.eq_ignore_ascii_case("episode");
    let query_term = if is_episode {
        item.grandparent_title.as_deref().unwrap_or(&item.title)
    } else {
        &item.title
    };

    let items_url = format!("{}/Users/{}/Items", base, user_id);
    let search_resp = client
        .get(&items_url)
        .header("X-Emby-Token", api_key)
        .header("Authorization", format!("MediaBrowser Token=\"{}\"", api_key))
        .query(&[
            ("searchTerm", query_term),
            ("includeItemTypes", "Movie,Episode"),
            ("recursive", "true"),
        ])
        .send()
        .await
        .map_err(|e| format!("Jellyfin items search failed: {}", e))?;

    if !search_resp.status().is_success() {
        return Ok(false);
    }

    let search_json = search_resp
        .json::<serde_json::Value>()
        .await
        .unwrap_or_default();

    let items = search_json
        .get("Items")
        .and_then(|i| i.as_array())
        .cloned()
        .unwrap_or_default();

    let mut found_item_id: Option<String> = None;

    if is_episode {
        for it in &items {
            let item_type = it.get("Type").and_then(|t| t.as_str()).unwrap_or("");
            if item_type == "Episode" {
                let ep_index = it.get("IndexNumber").and_then(|v| v.as_i64()).map(|v| v as i32);
                let season_index = it.get("ParentIndexNumber").and_then(|v| v.as_i64()).map(|v| v as i32);

                if item.episode_number == ep_index && (item.season_number.is_none() || item.season_number == season_index) {
                    if let Some(id) = it.get("Id").and_then(|i| i.as_str()) {
                        found_item_id = Some(id.to_string());
                        break;
                    }
                }
            }
        }
    } else {
        for it in &items {
            let item_type = it.get("Type").and_then(|t| t.as_str()).unwrap_or("");
            if item_type == "Movie" {
                let name = it.get("Name").and_then(|n| n.as_str()).unwrap_or("");
                if name.eq_ignore_ascii_case(&item.title) {
                    if let Some(id) = it.get("Id").and_then(|i| i.as_str()) {
                        found_item_id = Some(id.to_string());
                        break;
                    }
                }
            }
        }
    }

    // 3. If found, mark played
    if let Some(item_id) = found_item_id {
        let played_url = format!("{}/Users/{}/PlayedItems/{}", base, user_id, item_id);
        let played_resp = client
            .post(&played_url)
            .header("X-Emby-Token", api_key)
            .header("Authorization", format!("MediaBrowser Token=\"{}\"", api_key))
            .send()
            .await
            .map_err(|e| format!("Jellyfin mark played request failed: {}", e))?;

        Ok(played_resp.status().is_success())
    } else {
        Ok(false)
    }
}

/// Dispatches media writeback asynchronously if watch history sync is enabled for user
pub fn dispatch_media_writeback(pool: SqlitePool, user_id: String, item: WritebackItem) {
    tokio::spawn(async move {
        let bidi = crate::api::user_settings::get_user_setting(&pool, &user_id, "bidirectional_sync_enabled")
            .await
            .map(|s| s == "true" || s == "1")
            .unwrap_or(false);

        let plex_sync = crate::api::user_settings::get_user_setting(&pool, &user_id, "plex_sync_enabled")
            .await
            .map(|s| s == "true" || s == "1")
            .unwrap_or(bidi);

        let jellyfin_sync = crate::api::user_settings::get_user_setting(&pool, &user_id, "jellyfin_sync_enabled")
            .await
            .map(|s| s == "true" || s == "1")
            .unwrap_or(bidi);

        if !plex_sync && !jellyfin_sync {
            return;
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(12))
            .build()
            .unwrap_or_default();

        // 1. Sync to Plex if configured & enabled
        if plex_sync {
            let plex_url = crate::api::user_settings::get_user_setting(&pool, &user_id, "plex_url").await;
            let plex_api_key = crate::api::user_settings::get_user_setting(&pool, &user_id, "plex_api_key").await;

            if let (Some(ref url), Some(ref token)) = (plex_url, plex_api_key) {
                if !url.trim().is_empty() && !token.trim().is_empty() {
                    match writeback_item_to_plex(&client, url, token, &item).await {
                        Ok(true) => tracing::info!(title = %item.title, "Successfully synced watch state to Plex"),
                        Ok(false) => tracing::debug!(title = %item.title, "Item not found in Plex library for writeback"),
                        Err(e) => tracing::warn!(title = %item.title, error = %e, "Failed to writeback watch state to Plex"),
                    }
                }
            }
        }

        // 2. Sync to Jellyfin if configured & enabled
        if jellyfin_sync {
            let jellyfin_url = crate::api::user_settings::get_user_setting(&pool, &user_id, "jellyfin_url").await;
            let jellyfin_api_key = crate::api::user_settings::get_user_setting(&pool, &user_id, "jellyfin_api_key").await;
            let jellyfin_username = crate::api::user_settings::get_user_setting(&pool, &user_id, "jellyfin_username").await;

            if let (Some(ref url), Some(ref key)) = (jellyfin_url, jellyfin_api_key) {
                if !url.trim().is_empty() && !key.trim().is_empty() {
                    match writeback_item_to_jellyfin(&client, url, key, jellyfin_username.as_deref(), &item).await {
                        Ok(true) => tracing::info!(title = %item.title, "Successfully synced watch state to Jellyfin"),
                        Ok(false) => tracing::debug!(title = %item.title, "Item not found in Jellyfin library for writeback"),
                        Err(e) => tracing::warn!(title = %item.title, error = %e, "Failed to writeback watch state to Jellyfin"),
                    }
                }
            }
        }
    });
}

pub fn start_media_sync_worker(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Initial delay before background sync starts
        tokio::time::sleep(Duration::from_secs(10)).await;

        loop {
            let interval_hours = state.get_task_interval_hours("media_sync").await;
            let next_run = chrono::Utc::now() + chrono::Duration::hours(interval_hours as i64);
            state.task_tracker.set_media_sync_next_run(Some(next_run.to_rfc3339())).await;

            let sleep_duration = Duration::from_secs(interval_hours.max(1) * 3600);

            tokio::select! {
                _ = tokio::time::sleep(sleep_duration) => {},
                _ = state.media_sync_notify.notified() => {
                    tracing::info!("Media sync notification received; running cycle immediately");
                }
            }

            run_media_sync_cycle(&state).await;
        }
    })
}

pub async fn run_media_sync_cycle(state: &AppState) {
    let pool = &state.pool;
    state.task_tracker.set_media_sync_running(Some(pool), "Running media server watch history sync...").await;
    state.log_buffer.push("INFO", "bingesync::writeback", "Starting media server watch history sync cycle (Plex & Jellyfin)...");

    match sync_all_users_media(state).await {
        Ok((total_synced, total_processed, users_synced)) => {
            let msg = if users_synced == 0 {
                "Watch history sync completed: no users configured for media server sync".to_string()
            } else {
                format!(
                    "Watch history sync completed across {} user{}: {} watches synced to media servers ({} processed)",
                    users_synced,
                    if users_synced == 1 { "" } else { "s" },
                    total_synced,
                    total_processed
                )
            };
            state.task_tracker.set_media_sync_idle(Some(pool), total_synced, &msg).await;
            state.log_buffer.push("INFO", "bingesync::writeback", &msg);
            tracing::info!("{}", msg);
        }
        Err(e) => {
            let err_msg = format!("Watch history sync cycle error: {}", e);
            tracing::error!("{}", err_msg);
            state.task_tracker.set_media_sync_idle(Some(pool), 0, &err_msg).await;
            state.log_buffer.push("ERROR", "bingesync::writeback", &err_msg);
        }
    }
}

pub async fn sync_all_users_media(state: &AppState) -> Result<(usize, usize, usize), String> {
    let pool = &state.pool;

    #[derive(sqlx::FromRow)]
    struct UserCandidate {
        id: String,
        email: String,
    }

    let users: Vec<UserCandidate> = sqlx::query_as(
        r#"
        SELECT DISTINCT u.id, u.email
        FROM users u
        JOIN user_settings s ON s.user_id = u.id
        WHERE (s.key = 'plex_sync_enabled' AND (s.value = 'true' OR s.value = '1'))
           OR (s.key = 'jellyfin_sync_enabled' AND (s.value = 'true' OR s.value = '1'))
           OR (s.key = 'bidirectional_sync_enabled' AND (s.value = 'true' OR s.value = '1'))
        "#
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch user candidates for media sync: {}", e))?;

    let mut total_synced = 0;
    let mut total_processed = 0;
    let mut users_synced = 0;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .unwrap_or_default();

    for u in users {
        let bidi = crate::api::user_settings::get_user_setting(pool, &u.id, "bidirectional_sync_enabled")
            .await
            .map(|s| s == "true" || s == "1")
            .unwrap_or(false);

        let plex_sync = crate::api::user_settings::get_user_setting(pool, &u.id, "plex_sync_enabled")
            .await
            .map(|s| s == "true" || s == "1")
            .unwrap_or(bidi);

        let jellyfin_sync = crate::api::user_settings::get_user_setting(pool, &u.id, "jellyfin_sync_enabled")
            .await
            .map(|s| s == "true" || s == "1")
            .unwrap_or(bidi);

        if !plex_sync && !jellyfin_sync {
            continue;
        }

        let plex_url = crate::api::user_settings::get_user_setting(pool, &u.id, "plex_url").await;
        let plex_api_key = crate::api::user_settings::get_user_setting(pool, &u.id, "plex_api_key").await;
        let jellyfin_url = crate::api::user_settings::get_user_setting(pool, &u.id, "jellyfin_url").await;
        let jellyfin_api_key = crate::api::user_settings::get_user_setting(pool, &u.id, "jellyfin_api_key").await;
        let jellyfin_username = crate::api::user_settings::get_user_setting(pool, &u.id, "jellyfin_username").await;

        let has_plex = plex_sync && plex_url.as_ref().map(|s| !s.trim().is_empty()).unwrap_or(false)
            && plex_api_key.as_ref().map(|s| !s.trim().is_empty()).unwrap_or(false);

        let has_jellyfin = jellyfin_sync && jellyfin_url.as_ref().map(|s| !s.trim().is_empty()).unwrap_or(false)
            && jellyfin_api_key.as_ref().map(|s| !s.trim().is_empty()).unwrap_or(false);

        if !has_plex && !has_jellyfin {
            continue;
        }

        // Fetch user's recent completed scrobbles
        #[derive(sqlx::FromRow)]
        struct RawScrobble {
            title: String,
            media_type: String,
            grandparent_title: Option<String>,
            season_number: Option<i32>,
            episode_number: Option<i32>,
            year: Option<i32>,
            rating_key: Option<String>,
        }

        let rows = sqlx::query_as::<_, RawScrobble>(
            "SELECT title, media_type, grandparent_title, season_number, episode_number, year, rating_key FROM scrobbles WHERE user_id = ? AND (watch_status = 'completed' OR progress_percent >= 80) ORDER BY watched_at DESC LIMIT 50"
        )
        .bind(&u.id)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        if rows.is_empty() {
            continue;
        }

        let mut user_plex_synced = 0;
        let mut user_jellyfin_synced = 0;

        for r in &rows {
            let item = WritebackItem {
                title: r.title.clone(),
                media_type: r.media_type.clone(),
                grandparent_title: r.grandparent_title.clone(),
                season_number: r.season_number,
                episode_number: r.episode_number,
                year: r.year,
                rating_key: r.rating_key.clone(),
            };

            if has_plex {
                if let (Some(ref url), Some(ref token)) = (&plex_url, &plex_api_key) {
                    if let Ok(true) = writeback_item_to_plex(&client, url, token, &item).await {
                        user_plex_synced += 1;
                    }
                }
            }

            if has_jellyfin {
                if let (Some(ref url), Some(ref key)) = (&jellyfin_url, &jellyfin_api_key) {
                    if let Ok(true) = writeback_item_to_jellyfin(&client, url, key, jellyfin_username.as_deref(), &item).await {
                        user_jellyfin_synced += 1;
                    }
                }
            }
        }

        total_processed += rows.len();
        let user_synced_count = user_plex_synced + user_jellyfin_synced;
        total_synced += user_synced_count;
        users_synced += 1;

        state.log_buffer.push(
            "INFO",
            "bingesync::writeback",
            &format!(
                "Media sync for user {}: {} scrobbles evaluated (Plex: {}, Jellyfin: {})",
                u.email, rows.len(), user_plex_synced, user_jellyfin_synced
            ),
        );
    }

    Ok((total_synced, total_processed, users_synced))
}

/// Handler for testing user's Plex connection: POST /api/v1/user/settings/test-plex
pub async fn test_plex_connection_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(payload): Json<TestPlexRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let url = match payload.plex_url {
        Some(u) if !u.trim().is_empty() => u.trim().to_string(),
        _ => match crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "plex_url").await {
            Some(u) if !u.trim().is_empty() => u.trim().to_string(),
            _ => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "Plex URL is required" })),
                ))
            }
        },
    };

    let token = match payload.plex_api_key {
        Some(k) if !k.trim().is_empty() && !k.contains('•') => k.trim().to_string(),
        _ => match crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "plex_api_key").await {
            Some(k) if !k.trim().is_empty() => k.trim().to_string(),
            _ => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "Plex API token is required" })),
                ))
            }
        },
    };

    match test_plex_connection(&url, &token).await {
        Ok(msg) => Ok(Json(serde_json::json!({
            "status": "success",
            "message": msg
        }))),
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e })),
        )),
    }
}

/// Handler for testing user's Jellyfin connection: POST /api/v1/user/settings/test-jellyfin
pub async fn test_jellyfin_connection_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(payload): Json<TestJellyfinRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let url = match payload.jellyfin_url {
        Some(u) if !u.trim().is_empty() => u.trim().to_string(),
        _ => match crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "jellyfin_url").await {
            Some(u) if !u.trim().is_empty() => u.trim().to_string(),
            _ => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "Jellyfin URL is required" })),
                ))
            }
        },
    };

    let api_key = match payload.jellyfin_api_key {
        Some(k) if !k.trim().is_empty() && !k.contains('•') => k.trim().to_string(),
        _ => match crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "jellyfin_api_key").await {
            Some(k) if !k.trim().is_empty() => k.trim().to_string(),
            _ => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "Jellyfin API key is required" })),
                ))
            }
        },
    };

    match test_jellyfin_connection(&url, &api_key).await {
        Ok(msg) => Ok(Json(serde_json::json!({
            "status": "success",
            "message": msg
        }))),
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e })),
        )),
    }
}

/// Handler for triggering media writeback on-demand: POST /api/v1/user/sync/writeback
pub async fn trigger_writeback_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(payload): Json<TriggerWritebackRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let plex_url = crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "plex_url").await;
    let plex_api_key = crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "plex_api_key").await;
    let jellyfin_url = crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "jellyfin_url").await;
    let jellyfin_api_key = crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "jellyfin_api_key").await;
    let jellyfin_username = crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "jellyfin_username").await;

    let has_plex = plex_url.as_ref().map(|u| !u.trim().is_empty()).unwrap_or(false)
        && plex_api_key.as_ref().map(|k| !k.trim().is_empty()).unwrap_or(false);
    let has_jellyfin = jellyfin_url.as_ref().map(|u| !u.trim().is_empty()).unwrap_or(false)
        && jellyfin_api_key.as_ref().map(|k| !k.trim().is_empty()).unwrap_or(false);

    if !has_plex && !has_jellyfin {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "No media server configured. Please configure Plex or Jellyfin in Connected Services."
            })),
        ));
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .unwrap_or_default();

    // Query scrobbles
    let items: Vec<WritebackItem> = if let Some(ref scrobble_id) = payload.scrobble_id {
        #[derive(sqlx::FromRow)]
        struct RawScrobble {
            title: String,
            media_type: String,
            grandparent_title: Option<String>,
            season_number: Option<i32>,
            episode_number: Option<i32>,
            year: Option<i32>,
            rating_key: Option<String>,
        }

        let raw = sqlx::query_as::<_, RawScrobble>(
            "SELECT title, media_type, grandparent_title, season_number, episode_number, year, rating_key FROM scrobbles WHERE id = ? AND user_id = ?"
        )
        .bind(scrobble_id)
        .bind(&auth.user_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))))?;

        match raw {
            Some(r) => vec![WritebackItem {
                title: r.title,
                media_type: r.media_type,
                grandparent_title: r.grandparent_title,
                season_number: r.season_number,
                episode_number: r.episode_number,
                year: r.year,
                rating_key: r.rating_key,
            }],
            None => return Err((StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "Scrobble not found" })))),
        }
    } else {
        let limit = payload.limit.unwrap_or(25).min(100) as i64;
        #[derive(sqlx::FromRow)]
        struct RawScrobble {
            title: String,
            media_type: String,
            grandparent_title: Option<String>,
            season_number: Option<i32>,
            episode_number: Option<i32>,
            year: Option<i32>,
            rating_key: Option<String>,
        }

        let rows = sqlx::query_as::<_, RawScrobble>(
            "SELECT title, media_type, grandparent_title, season_number, episode_number, year, rating_key FROM scrobbles WHERE user_id = ? AND (watch_status = 'completed' OR progress_percent >= 80) ORDER BY watched_at DESC LIMIT ?"
        )
        .bind(&auth.user_id)
        .bind(limit)
        .fetch_all(&state.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))))?;

        rows.into_iter().map(|r| WritebackItem {
            title: r.title,
            media_type: r.media_type,
            grandparent_title: r.grandparent_title,
            season_number: r.season_number,
            episode_number: r.episode_number,
            year: r.year,
            rating_key: r.rating_key,
        }).collect()
    };

    let total = items.len();
    let mut plex_synced = 0;
    let mut jellyfin_synced = 0;

    for item in &items {
        if has_plex {
            if let (Some(ref u), Some(ref t)) = (&plex_url, &plex_api_key) {
                if let Ok(true) = writeback_item_to_plex(&client, u, t, item).await {
                    plex_synced += 1;
                }
            }
        }
        if has_jellyfin {
            if let (Some(ref u), Some(ref k)) = (&jellyfin_url, &jellyfin_api_key) {
                if let Ok(true) = writeback_item_to_jellyfin(&client, u, k, jellyfin_username.as_deref(), item).await {
                    jellyfin_synced += 1;
                }
            }
        }
    }

    let msg = format!("Writeback complete: {} scrobbles processed (Plex: {}, Jellyfin: {})", total, plex_synced, jellyfin_synced);
    state.log_buffer.push(
        "INFO",
        "bingesync::writeback",
        &format!("Manual writeback completed for {}: {}", auth.email, msg),
    );
    tracing::info!("{}", msg);

    Ok(Json(serde_json::json!({
        "status": "success",
        "scrobbles_processed": total,
        "plex_synced": plex_synced,
        "jellyfin_synced": jellyfin_synced,
        "message": msg
    })))
}

