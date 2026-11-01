use std::collections::HashSet;
use std::path::Path as FilePath;
use std::time::Duration;

use crate::api::AppState;

/// Reaps abandoned active sessions that have had no playback progress or heartbeat in over 2 hours.
pub async fn run_session_reaper(state: &AppState) -> Result<String, String> {
    state.log_buffer.push("INFO", "bingesync::tasks::reaper", "Sweeping for abandoned active playback sessions...");
    tracing::info!("Starting stale active sessions reaper sweep (threshold: 2 hours inactive)");

    let res = sqlx::query("DELETE FROM active_sessions WHERE updated_at < datetime('now', '-2 hours')")
        .execute(&state.pool)
        .await
        .map_err(|e| format!("Database error deleting stale active sessions: {}", e))?;

    let count = res.rows_affected() as usize;
    let msg = if count > 0 {
        format!("Reaped {} abandoned active playback session(s)", count)
    } else {
        "No abandoned active sessions found (0 cleared)".to_string()
    };

    state.log_buffer.push("INFO", "bingesync::tasks::reaper", &msg);
    tracing::info!("{}", msg);

    Ok(msg)
}

/// Spawns the background worker for reaping stale active sessions.
/// Runs hourly by default (every 1 hour) or upon notification.
pub fn start_session_reaper_worker(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(15)).await;

        loop {
            let interval_hours = state.get_task_interval_hours("session_reaper").await;
            let next_run = (chrono::Utc::now() + chrono::Duration::hours(interval_hours as i64)).to_rfc3339();
            state.task_tracker.set_session_reaper_next_run(Some(next_run)).await;

            tokio::select! {
                _ = state.session_reaper_notify.notified() => {}
                _ = tokio::time::sleep(Duration::from_secs(interval_hours * 3600)) => {}
            }

            state.task_tracker.set_session_reaper_running(Some(&state.pool), "Reaping stale active playback sessions...").await;
            match run_session_reaper(&state).await {
                Ok(msg) => {
                    let count = if let Some(first_word) = msg.strip_prefix("Reaped ") {
                        first_word.split_whitespace().next().and_then(|n| n.parse::<usize>().ok()).unwrap_or(0)
                    } else {
                        0
                    };
                    state.task_tracker.set_session_reaper_idle(Some(&state.pool), count, &msg).await;
                }
                Err(err) => {
                    state.task_tracker.set_session_reaper_idle(Some(&state.pool), 0, &format!("Reaper error: {}", err)).await;
                }
            }
        }
    })
}

/// Scans the disk image cache directory and deletes unreferenced orphaned poster files.
pub async fn run_image_prune(state: &AppState) -> Result<String, String> {
    state.log_buffer.push("INFO", "bingesync::tasks::images", "Scanning disk image cache for orphaned poster files...");
    tracing::info!("Starting orphaned poster image cache prune");

    let cache_dir = FilePath::new(&state.config.image_cache_dir);
    if !cache_dir.exists() {
        let msg = "Image cache directory does not exist yet (0 pruned)".to_string();
        state.log_buffer.push("INFO", "bingesync::tasks::images", &msg);
        return Ok(msg);
    }

    let mut referenced: HashSet<String> = HashSet::new();

    let scrobble_thumbs: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT thumb_url FROM scrobbles WHERE thumb_url IS NOT NULL AND thumb_url != ''"
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    for thumb in scrobble_thumbs {
        if let Some(filename) = thumb.split('/').last() {
            referenced.insert(filename.to_string());
        }
    }

    let user_avatars: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT avatar_url FROM users WHERE avatar_url IS NOT NULL AND avatar_url != ''"
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    for av in user_avatars {
        if let Some(filename) = av.split('/').last() {
            referenced.insert(filename.to_string());
        }
    }

    let mut deleted_count = 0usize;
    let mut bytes_freed = 0u64;

    if let Ok(mut entries) = tokio::fs::read_dir(cache_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.is_file() {
                if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                    if !filename.starts_with('.') && !referenced.contains(filename) {
                        let size = entry.metadata().await.map(|m| m.len()).unwrap_or(0);
                        if tokio::fs::remove_file(&path).await.is_ok() {
                            deleted_count += 1;
                            bytes_freed += size;
                        }
                    }
                }
            }
        }
    }

    let kb_freed = bytes_freed / 1024;
    let msg = format!("Pruned {} orphaned image file(s), reclaiming {} KB of disk storage", deleted_count, kb_freed);
    state.log_buffer.push("INFO", "bingesync::tasks::images", &msg);
    tracing::info!("{}", msg);

    Ok(msg)
}

/// Spawns the background worker for pruning orphaned poster image files.
/// Runs weekly by default (every 168 hours) or upon notification.
pub fn start_image_prune_worker(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;

        loop {
            let interval_hours = state.get_task_interval_hours("image_prune").await;
            let next_run = (chrono::Utc::now() + chrono::Duration::hours(interval_hours as i64)).to_rfc3339();
            state.task_tracker.set_image_prune_next_run(Some(next_run)).await;

            tokio::select! {
                _ = state.image_prune_notify.notified() => {}
                _ = tokio::time::sleep(Duration::from_secs(interval_hours * 3600)) => {}
            }

            state.task_tracker.set_image_prune_running(Some(&state.pool), "Pruning orphaned image cache files...").await;
            match run_image_prune(&state).await {
                Ok(msg) => {
                    let count = if let Some(first_word) = msg.strip_prefix("Pruned ") {
                        first_word.split_whitespace().next().and_then(|n| n.parse::<usize>().ok()).unwrap_or(0)
                    } else {
                        0
                    };
                    state.task_tracker.set_image_prune_idle(Some(&state.pool), count, &msg).await;
                }
                Err(err) => {
                    state.task_tracker.set_image_prune_idle(Some(&state.pool), 0, &format!("Prune error: {}", err)).await;
                }
            }
        }
    })
}

/// Automatically detects and prunes rapid duplicate scrobbles across all user accounts.
pub async fn run_duplicate_pruner(state: &AppState) -> Result<String, String> {
    state.log_buffer.push("INFO", "bingesync::tasks::duplicates", "Scanning for duplicate scrobbles across all accounts...");
    tracing::info!("Starting automated duplicate scrobble cleanup");

    let users: Vec<String> = sqlx::query_scalar("SELECT id FROM users")
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

    let mut total_deleted = 0usize;

    for user_id in users {
        if let Ok(count) = crate::api::history::clean_duplicates_for_user(&state.pool, &user_id, 2, "keep_oldest", None).await {
            total_deleted += count;
        }
    }

    let msg = if total_deleted > 0 {
        format!("Automatically pruned {} duplicate scrobble(s)", total_deleted)
    } else {
        "No duplicate scrobbles detected across user accounts (0 pruned)".to_string()
    };

    state.log_buffer.push("INFO", "bingesync::tasks::duplicates", &msg);
    tracing::info!("{}", msg);

    Ok(msg)
}

/// Spawns the background worker for cleaning duplicate scrobbles.
/// Runs daily by default (every 24 hours) or upon notification.
pub fn start_duplicate_pruner_worker(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(45)).await;

        loop {
            let interval_hours = state.get_task_interval_hours("duplicate_pruner").await;
            let next_run = (chrono::Utc::now() + chrono::Duration::hours(interval_hours as i64)).to_rfc3339();
            state.task_tracker.set_duplicate_pruner_next_run(Some(next_run)).await;

            tokio::select! {
                _ = state.duplicate_pruner_notify.notified() => {}
                _ = tokio::time::sleep(Duration::from_secs(interval_hours * 3600)) => {}
            }

            state.task_tracker.set_duplicate_pruner_running(Some(&state.pool), "Scanning and cleaning duplicate scrobbles...").await;
            match run_duplicate_pruner(&state).await {
                Ok(msg) => {
                    let count = if let Some(first_word) = msg.strip_prefix("Automatically pruned ") {
                        first_word.split_whitespace().next().and_then(|n| n.parse::<usize>().ok()).unwrap_or(0)
                    } else {
                        0
                    };
                    state.task_tracker.set_duplicate_pruner_idle(Some(&state.pool), count, &msg).await;
                }
                Err(err) => {
                    state.task_tracker.set_duplicate_pruner_idle(Some(&state.pool), 0, &format!("Duplicate pruner error: {}", err)).await;
                }
            }
        }
    })
}

/// Re-evaluates missing metadata and refreshes community ratings for recently watched media.
pub async fn run_metadata_backfill(state: &AppState) -> Result<String, String> {
    state.log_buffer.push("INFO", "bingesync::tasks::metadata", "Scanning for scrobbles requiring metadata backfill or score refresh...");
    tracing::info!("Starting missing metadata backfill & stale rating refresh");

    let res1 = sqlx::query(
        "UPDATE scrobbles SET metadata_updated_at = NULL WHERE (overview IS NULL OR overview = '') OR (genres IS NULL OR genres = '') OR (actors IS NULL OR actors = '')"
    )
    .execute(&state.pool)
    .await
    .unwrap_or_default();

    let res2 = sqlx::query(
        "UPDATE scrobbles SET metadata_updated_at = NULL WHERE watched_at >= datetime('now', '-30 days') AND metadata_updated_at < datetime('now', '-7 days')"
    )
    .execute(&state.pool)
    .await
    .unwrap_or_default();

    let count = (res1.rows_affected() + res2.rows_affected()) as usize;
    if count > 0 {
        state.metadata_notify.notify_waiters();
        state.metadata_notify.notify_one();
    }

    let msg = format!("Queued {} scrobble(s) for metadata enrichment & rating refresh", count);
    state.log_buffer.push("INFO", "bingesync::tasks::metadata", &msg);
    tracing::info!("{}", msg);

    Ok(msg)
}

/// Spawns the background worker for metadata backfilling and rating refresh.
/// Runs daily by default (every 24 hours) or upon notification.
pub fn start_metadata_backfill_worker(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;

        loop {
            let interval_hours = state.get_task_interval_hours("metadata_backfill").await;
            let next_run = (chrono::Utc::now() + chrono::Duration::hours(interval_hours as i64)).to_rfc3339();
            state.task_tracker.set_metadata_backfill_next_run(Some(next_run)).await;

            tokio::select! {
                _ = state.metadata_backfill_notify.notified() => {}
                _ = tokio::time::sleep(Duration::from_secs(interval_hours * 3600)) => {}
            }

            state.task_tracker.set_metadata_backfill_running(Some(&state.pool), "Backfilling missing metadata & refreshing ratings...").await;
            match run_metadata_backfill(&state).await {
                Ok(msg) => {
                    let count = if let Some(first_word) = msg.strip_prefix("Queued ") {
                        first_word.split_whitespace().next().and_then(|n| n.parse::<usize>().ok()).unwrap_or(0)
                    } else {
                        0
                    };
                    state.task_tracker.set_metadata_backfill_idle(Some(&state.pool), count, &msg).await;
                }
                Err(err) => {
                    state.task_tracker.set_metadata_backfill_idle(Some(&state.pool), 0, &format!("Metadata backfill error: {}", err)).await;
                }
            }
        }
    })
}

/// Checks the GitHub releases API for latest BingeSync release.
pub async fn run_version_check(state: &AppState) -> Result<String, String> {
    state.log_buffer.push("INFO", "bingesync::tasks::version", "Checking GitHub API for latest BingeSync release...");
    tracing::info!("Running automated GitHub release version check");

    let resp = crate::api::settings::check_latest_github_release(Some(&state.pool)).await;
    let msg = if let Some(latest) = resp.latest_version {
        if resp.update_available {
            format!("New version v{} is available! (Current: v{})", latest, resp.current_version)
        } else {
            format!("BingeSync is up to date (v{})", resp.current_version)
        }
    } else {
        format!("Current version: v{} (GitHub release check completed)", resp.current_version)
    };

    state.log_buffer.push("INFO", "bingesync::tasks::version", &msg);
    tracing::info!("{}", msg);

    Ok(msg)
}

/// Spawns the background worker for checking GitHub releases.
/// Runs daily by default (every 24 hours) or upon notification.
pub fn start_version_check_worker(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(90)).await;

        loop {
            let interval_hours = state.get_task_interval_hours("version_check").await;
            let next_run = (chrono::Utc::now() + chrono::Duration::hours(interval_hours as i64)).to_rfc3339();
            state.task_tracker.set_version_check_next_run(Some(next_run)).await;

            tokio::select! {
                _ = state.version_check_notify.notified() => {}
                _ = tokio::time::sleep(Duration::from_secs(interval_hours * 3600)) => {}
            }

            state.task_tracker.set_version_check_running(Some(&state.pool), "Checking GitHub API for releases...").await;
            match run_version_check(&state).await {
                Ok(msg) => {
                    state.task_tracker.set_version_check_idle(Some(&state.pool), 1, &msg).await;
                }
                Err(err) => {
                    state.task_tracker.set_version_check_idle(Some(&state.pool), 0, &format!("Version check error: {}", err)).await;
                }
            }
        }
    })
}

/// Audits watched scrobbles against connected Radarr and Sonarr libraries.
pub async fn run_library_audit(state: &AppState) -> Result<String, String> {
    state.log_buffer.push("INFO", "bingesync::tasks::integrations", "Auditing watched media against connected Radarr & Sonarr libraries...");
    tracing::info!("Starting Radarr/Sonarr watched library desync audit");

    let users: Vec<String> = sqlx::query_scalar("SELECT id FROM users")
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

    let mut total_audited = 0usize;
    let mut total_reclaimable_bytes = 0u64;

    for user_id in users {
        if let Ok((items_count, bytes)) = crate::api::integrations::perform_library_audit_for_user(state, &user_id).await {
            total_audited += items_count;
            total_reclaimable_bytes += bytes;
        }
    }

    let mb_reclaimable = total_reclaimable_bytes / (1024 * 1024);
    let msg = format!("Audited {} watched item(s) across Arr libraries (~{} MB reclaimable space)", total_audited, mb_reclaimable);
    state.log_buffer.push("INFO", "bingesync::tasks::integrations", &msg);
    tracing::info!("{}", msg);

    Ok(msg)
}

/// Spawns the background worker for auditing Radarr/Sonarr libraries.
/// Runs daily by default (every 24 hours) or upon notification.
pub fn start_library_audit_worker(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(120)).await;

        loop {
            let interval_hours = state.get_task_interval_hours("library_audit").await;
            let next_run = (chrono::Utc::now() + chrono::Duration::hours(interval_hours as i64)).to_rfc3339();
            state.task_tracker.set_library_audit_next_run(Some(next_run)).await;

            tokio::select! {
                _ = state.library_audit_notify.notified() => {}
                _ = tokio::time::sleep(Duration::from_secs(interval_hours * 3600)) => {}
            }

            state.task_tracker.set_library_audit_running(Some(&state.pool), "Auditing connected Radarr & Sonarr libraries...").await;
            match run_library_audit(&state).await {
                Ok(msg) => {
                    let count = if let Some(first_word) = msg.strip_prefix("Audited ") {
                        first_word.split_whitespace().next().and_then(|n| n.parse::<usize>().ok()).unwrap_or(0)
                    } else {
                        0
                    };
                    state.task_tracker.set_library_audit_idle(Some(&state.pool), count, &msg).await;
                }
                Err(err) => {
                    state.task_tracker.set_library_audit_idle(Some(&state.pool), 0, &format!("Library audit error: {}", err)).await;
                }
            }
        }
    })
}

