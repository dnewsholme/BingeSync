use std::time::Duration;
use chrono::Utc;
use crate::api::AppState;

pub fn start_tautulli_sync_worker(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Initial delay before background sync starts
        tokio::time::sleep(Duration::from_secs(5)).await;

        loop {
            let interval_hours = state.get_task_interval_hours("tautulli_sync").await;
            let next_run = Utc::now() + chrono::Duration::hours(interval_hours as i64);
            state.task_tracker.set_tautulli_sync_next_run(Some(next_run.to_rfc3339())).await;

            let sleep_duration = Duration::from_secs(interval_hours.max(1) * 3600);

            tokio::select! {
                _ = tokio::time::sleep(sleep_duration) => {},
                _ = state.tautulli_sync_notify.notified() => {
                    tracing::info!("Tautulli continuous sync notification received; running cycle immediately");
                }
            }

            run_tautulli_sync_cycle(&state).await;
        }
    })
}

pub async fn run_tautulli_sync_cycle(state: &AppState) {
    let pool = &state.pool;
    state.task_tracker.set_tautulli_sync_running(Some(pool), "Running continuous sync...").await;

    match sync_tautulli_all_users(state).await {
        Ok((total_imported, total_skipped, users_synced)) => {
            let msg = if users_synced == 0 {
                "Sync completed: no users configured for continuous sync".to_string()
            } else {
                format!(
                    "Sync completed across {} user{}: {} new scrobbles added ({} duplicates skipped)",
                    users_synced,
                    if users_synced == 1 { "" } else { "s" },
                    total_imported,
                    total_skipped
                )
            };
            state.task_tracker.set_tautulli_sync_idle(Some(pool), total_imported, &msg).await;
            state.log_buffer.push("INFO", "bingesync::tautulli", &msg);
        }
        Err(e) => {
            let err_msg = format!("Tautulli continuous sync error: {}", e);
            tracing::error!("{}", err_msg);
            state.task_tracker.set_tautulli_sync_idle(Some(pool), 0, &err_msg).await;
            state.log_buffer.push("ERROR", "bingesync::tautulli", &err_msg);
        }
    }
}

pub async fn sync_tautulli_all_users(state: &AppState) -> Result<(usize, usize, usize), String> {
    let pool = &state.pool;

    #[derive(sqlx::FromRow)]
    struct UserCandidate {
        id: String,
        email: String,
        preferred_username: Option<String>,
    }

    // Find all users who enabled continuous sync or configured a Tautulli user
    let users: Vec<UserCandidate> = sqlx::query_as(
        r#"
        SELECT DISTINCT u.id, u.email, u.preferred_username
        FROM users u
        JOIN user_settings s ON s.user_id = u.id
        WHERE (s.key = 'tautulli_sync_enabled' AND (s.value = 'true' OR s.value = '1'))
           OR (s.key = 'tautulli_user' AND s.value IS NOT NULL AND s.value != '')
        "#
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch user candidates for Tautulli sync: {}", e))?;

    let mut total_imported = 0;
    let mut total_skipped = 0;
    let mut users_synced = 0;

    for u in users {
        let is_enabled_str = crate::api::user_settings::get_user_setting(pool, &u.id, "tautulli_sync_enabled").await;
        let is_enabled = is_enabled_str.as_deref().map(|s| s == "true" || s == "1").unwrap_or(false);
        if !is_enabled {
            continue;
        }

        let tautulli_user = match crate::api::user_settings::get_user_setting(pool, &u.id, "tautulli_user").await {
            Some(un) if !un.trim().is_empty() => un.trim().to_string(),
            _ => match u.preferred_username.as_deref() {
                Some(p) if !p.trim().is_empty() => p.trim().to_string(),
                _ => u.email.split('@').next().unwrap_or("").to_string(),
            },
        };

        if tautulli_user.is_empty() {
            continue;
        }

        // Resolve Tautulli URL: user settings -> global db setting -> env var
        let tautulli_url = match crate::api::user_settings::get_user_setting(pool, &u.id, "tautulli_url").await {
            Some(url) if !url.trim().is_empty() => Some(url),
            _ => match crate::api::settings::get_db_setting(pool, "tautulli_url").await {
                Some(url) if !url.trim().is_empty() => Some(url),
                _ => std::env::var("TAUTULLI_URL").ok().filter(|s| !s.trim().is_empty()),
            },
        };

        let tautulli_api_key = match crate::api::user_settings::get_user_setting(pool, &u.id, "tautulli_api_key").await {
            Some(key) if !key.trim().is_empty() => Some(key),
            _ => match crate::api::settings::get_db_setting(pool, "tautulli_api_key").await {
                Some(key) if !key.trim().is_empty() => Some(key),
                _ => std::env::var("TAUTULLI_API_KEY").ok().filter(|s| !s.trim().is_empty()),
            },
        };

        let (url, api_key) = match (tautulli_url, tautulli_api_key) {
            (Some(u), Some(k)) => (u, k),
            _ => {
                tracing::debug!(user_id = %u.id, email = %u.email, "Tautulli URL or API key not configured for continuous sync; skipping");
                continue;
            }
        };

        if let Err(e) = crate::api::ssrf::validate_integration_url(&url) {
            tracing::warn!(user_id = %u.id, error = %e, "Invalid Tautulli URL in continuous sync; skipping");
            continue;
        }

        // Fetch recent items (e.g. 100 recent watches)
        match crate::api::tautulli_import::fetch_recent_tautulli_history(&url, &api_key, &tautulli_user, 100).await {
            Ok(items) => {
                if !items.is_empty() {
                    match crate::api::tautulli_import::insert_tautulli_watch_items(
                        pool,
                        &u.id,
                        &items,
                        Some(&state.log_buffer),
                    )
                    .await
                    {
                        Ok((imported, skipped)) => {
                            total_imported += imported;
                            total_skipped += skipped;
                            users_synced += 1;
                            if imported > 0 {
                                state.image_notify.notify_one();
                                state.metadata_notify.notify_one();
                                state.log_buffer.push(
                                    "INFO",
                                    "bingesync::tautulli",
                                    &format!(
                                        "Tautulli sync for {} ({}): {} new scrobbles added, {} duplicates skipped",
                                        u.email, tautulli_user, imported, skipped
                                    ),
                                );
                            }
                        }
                        Err(e) => {
                            state.log_buffer.push(
                                "ERROR",
                                "bingesync::tautulli",
                                &format!("Failed inserting scrobbles for {}: {}", u.email, e),
                            );
                        }
                    }
                } else {
                    users_synced += 1;
                }
            }
            Err(e) => {
                tracing::warn!(user_id = %u.id, error = %e, "Failed to fetch recent history from Tautulli");
                state.log_buffer.push(
                    "WARN",
                    "bingesync::tautulli",
                    &format!("Failed to fetch Tautulli history for user {}: {}", u.email, e),
                );
            }
        }
    }

    Ok((total_imported, total_skipped, users_synced))
}

