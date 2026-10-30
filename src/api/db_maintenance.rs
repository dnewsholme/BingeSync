use std::time::Duration;
use crate::api::AppState;

/// Executes the full database maintenance sequence:
/// 1. PRAGMA journal_mode = WAL;
/// 2. PRAGMA synchronous = NORMAL;
/// 3. PRAGMA cache_size = 10000;
/// 4. PRAGMA temp_store = MEMORY;
/// 5. REINDEX;
/// 6. ANALYZE;
/// 7. PRAGMA optimize;
/// 8. VACUUM;
pub async fn run_database_maintenance(state: &AppState) -> Result<String, String> {
    state.log_buffer.push("INFO", "bingesync::maintenance", "Starting database maintenance & optimization...");
    tracing::info!("Starting database maintenance & optimization: PRAGMAs, REINDEX, ANALYZE, optimize, VACUUM");

    let pool = &state.pool;

    // 1. PRAGMA optimizations
    sqlx::query("PRAGMA journal_mode = WAL;")
        .execute(pool)
        .await
        .map_err(|e| format!("Failed setting PRAGMA journal_mode: {}", e))?;

    sqlx::query("PRAGMA synchronous = NORMAL;")
        .execute(pool)
        .await
        .map_err(|e| format!("Failed setting PRAGMA synchronous: {}", e))?;

    sqlx::query("PRAGMA cache_size = 10000;")
        .execute(pool)
        .await
        .map_err(|e| format!("Failed setting PRAGMA cache_size: {}", e))?;

    sqlx::query("PRAGMA temp_store = MEMORY;")
        .execute(pool)
        .await
        .map_err(|e| format!("Failed setting PRAGMA temp_store: {}", e))?;

    // 2. Re-index all indexes
    sqlx::query("REINDEX;")
        .execute(pool)
        .await
        .map_err(|e| format!("Failed executing REINDEX: {}", e))?;

    // 3. Analyze tables and index statistics
    sqlx::query("ANALYZE;")
        .execute(pool)
        .await
        .map_err(|e| format!("Failed executing ANALYZE: {}", e))?;

    // 4. Run PRAGMA optimize
    sqlx::query("PRAGMA optimize;")
        .execute(pool)
        .await
        .map_err(|e| format!("Failed executing PRAGMA optimize: {}", e))?;

    // 5. VACUUM
    sqlx::query("VACUUM;")
        .execute(pool)
        .await
        .map_err(|e| format!("Failed executing VACUUM: {}", e))?;

    let msg = "Database maintenance completed successfully: PRAGMA settings applied, REINDEX, ANALYZE, optimize, and VACUUM executed".to_string();
    state.log_buffer.push("INFO", "bingesync::maintenance", &msg);
    tracing::info!("{}", msg);

    Ok(msg)
}

/// Spawns the background database maintenance worker.
/// Runs weekly by default (every 168 hours) or upon notification.
pub fn start_db_maintenance_worker(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Initial delay so application bootstrap finishes cleanly
        tokio::time::sleep(Duration::from_secs(10)).await;

        loop {
            let interval_hours = state.get_task_interval_hours("db_maintenance").await;
            let next_run = (chrono::Utc::now() + chrono::Duration::hours(interval_hours as i64)).to_rfc3339();
            state.task_tracker.set_db_maintenance_next_run(Some(next_run)).await;

            tokio::select! {
                _ = state.db_maintenance_notify.notified() => {
                    // Manual trigger signaled
                }
                _ = tokio::time::sleep(Duration::from_secs(interval_hours * 3600)) => {
                    // Periodic weekly timer expired
                }
            }

            state
                .task_tracker
                .set_db_maintenance_running(Some(&state.pool), "Executing database maintenance & optimization...")
                .await;

            match run_database_maintenance(&state).await {
                Ok(msg) => {
                    state
                        .task_tracker
                        .set_db_maintenance_idle(Some(&state.pool), 1, &msg)
                        .await;
                }
                Err(err) => {
                    state
                        .task_tracker
                        .set_db_maintenance_idle(Some(&state.pool), 0, &format!("Maintenance failed: {}", err))
                        .await;
                }
            }
        }
    })
}

