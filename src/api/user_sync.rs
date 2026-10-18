use reqwest::Client;
use serde::Deserialize;
use std::{collections::HashSet, time::Duration};

use crate::api::AppState;

/// Start background user cache sync worker loop.
pub fn start_user_cache_worker(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();

        // Initial delay to let application start up cleanly
        tokio::time::sleep(Duration::from_secs(2)).await;

        loop {
            state
                .task_tracker
                .set_user_sync_running("Syncing user accounts from Tautulli / Jellyfin...")
                .await;

            let processed = sync_user_cache_once_with_client(&state, &client).await;
            state
                .task_tracker
                .set_user_sync_idle(
                    processed,
                    &format!("Synced {} user mapping(s)", processed),
                )
                .await;

            let interval_hours = state.get_task_interval_hours("user_sync").await;
            let next_run = (chrono::Utc::now() + chrono::Duration::hours(interval_hours as i64)).to_rfc3339();
            state.task_tracker.set_user_sync_next_run(Some(next_run)).await;

            tokio::select! {
                _ = state.user_sync_notify.notified() => {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                _ = tokio::time::sleep(Duration::from_secs(interval_hours * 3600)) => {}
            }
        }
    })
}

/// Run a single user cache sync cycle across all configured Tautulli and Jellyfin instances.
pub async fn sync_user_cache_once(state: &AppState) -> usize {
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default();
    sync_user_cache_once_with_client(state, &client).await
}

async fn sync_user_cache_once_with_client(state: &AppState, client: &Client) -> usize {
    let mut total_synced = 0;
    let pool = &state.pool;

    // 1. Sync from Tautulli instances
    let mut tautulli_instances: HashSet<(String, String)> = HashSet::new();
    if let (Ok(url), Ok(key)) = (std::env::var("TAUTULLI_URL"), std::env::var("TAUTULLI_API_KEY")) {
        if !url.trim().is_empty() && !key.trim().is_empty() {
            tautulli_instances.insert((url.trim().to_string(), key.trim().to_string()));
        }
    }

    if let (Some(url), Some(key)) = (
        crate::api::settings::get_db_setting(pool, "tautulli_url").await,
        crate::api::settings::get_db_setting(pool, "tautulli_api_key").await,
    ) {
        if !url.trim().is_empty() && !key.trim().is_empty() {
            tautulli_instances.insert((url.trim().to_string(), key.trim().to_string()));
        }
    }

    #[derive(sqlx::FromRow)]
    struct UserSettingRow {
        user_id: String,
        value: String,
    }

    let user_urls: Vec<UserSettingRow> = sqlx::query_as(
        "SELECT user_id, value FROM user_settings WHERE key = 'tautulli_url' AND value IS NOT NULL AND value != ''"
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for row in user_urls {
        if let Some(key) = crate::api::user_settings::get_user_setting(pool, &row.user_id, "tautulli_api_key").await {
            if !row.value.trim().is_empty() && !key.trim().is_empty() {
                tautulli_instances.insert((row.value.trim().to_string(), key.trim().to_string()));
            }
        }
    }

    for (url, key) in tautulli_instances {
        let mapped = crate::api::tautulli_import::sync_tautulli_users_cache(pool, &url, &key).await;
        total_synced += mapped.len();
    }

    // 2. Sync from Jellyfin instances
    let mut jellyfin_instances: HashSet<(String, String)> = HashSet::new();
    if let (Some(url), Some(key)) = (
        crate::api::settings::get_db_setting(pool, "jellyfin_url").await,
        crate::api::settings::get_db_setting(pool, "jellyfin_api_key").await,
    ) {
        if !url.trim().is_empty() && !key.trim().is_empty() {
            jellyfin_instances.insert((url.trim().to_string(), key.trim().to_string()));
        }
    }

    let j_urls: Vec<UserSettingRow> = sqlx::query_as(
        "SELECT user_id, value FROM user_settings WHERE key = 'jellyfin_url' AND value IS NOT NULL AND value != ''"
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for row in j_urls {
        if let Some(key) = crate::api::user_settings::get_user_setting(pool, &row.user_id, "jellyfin_api_key").await {
            if !row.value.trim().is_empty() && !key.trim().is_empty() {
                jellyfin_instances.insert((row.value.trim().to_string(), key.trim().to_string()));
            }
        }
    }

    for (url, key) in jellyfin_instances {
        let count = sync_jellyfin_users_cache(pool, client, &url, &key).await;
        total_synced += count;
    }

    total_synced
}

#[derive(Deserialize, Debug)]
struct JellyfinUserResponseItem {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Name")]
    name: String,
}

pub async fn sync_jellyfin_users_cache(
    pool: &sqlx::SqlitePool,
    client: &Client,
    url: &str,
    api_key: &str,
) -> usize {
    let clean_url = url.trim().trim_end_matches('/');
    let endpoint = format!("{}/Users", clean_url);

    let res = match client
        .get(&endpoint)
        .header("X-Emby-Token", api_key)
        .header("Authorization", format!("MediaBrowser Token=\"{}\"", api_key))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(endpoint = %endpoint, error = %e, "Failed to connect to Jellyfin for user sync");
            return 0;
        }
    };

    if !res.status().is_success() {
        tracing::warn!(endpoint = %endpoint, status = %res.status(), "Jellyfin Users endpoint returned error");
        return 0;
    }

    let users: Vec<JellyfinUserResponseItem> = match res.json().await {
        Ok(u) => u,
        Err(e) => {
            tracing::warn!(endpoint = %endpoint, error = %e, "Failed to parse Jellyfin Users JSON");
            return 0;
        }
    };

    let mut count = 0;
    for u in users {
        let user_id = u.id.trim();
        let username = u.name.trim();
        if user_id.is_empty() || username.is_empty() {
            continue;
        }

        let _ = sqlx::query(
            r#"
            INSERT INTO plex_user_cache (plex_id, username, friendly_name, source, updated_at)
            VALUES (?, ?, ?, 'jellyfin', datetime('now'))
            ON CONFLICT(plex_id) DO UPDATE SET username = excluded.username, friendly_name = excluded.friendly_name, source = 'jellyfin', updated_at = excluded.updated_at
            "#
        )
        .bind(user_id)
        .bind(username)
        .bind(username)
        .execute(pool)
        .await;

        let unhyphenated = user_id.replace('-', "");
        if unhyphenated != user_id {
            let _ = sqlx::query(
                r#"
                INSERT INTO plex_user_cache (plex_id, username, friendly_name, source, updated_at)
                VALUES (?, ?, ?, 'jellyfin', datetime('now'))
                ON CONFLICT(plex_id) DO UPDATE SET username = excluded.username, friendly_name = excluded.friendly_name, source = 'jellyfin', updated_at = excluded.updated_at
                "#
            )
            .bind(&unhyphenated)
            .bind(username)
            .bind(username)
            .execute(pool)
            .await;
        }

        count += 1;
    }

    count
}
