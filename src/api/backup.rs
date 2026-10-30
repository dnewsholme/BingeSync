use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path as StdPath, PathBuf},
    time::{Duration, SystemTime},
};

use crate::{api::AppState, auth::AuthUser};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupItem {
    pub filename: String,
    pub size_bytes: u64,
    pub size_formatted: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupsListResponse {
    pub backup_dir: String,
    pub backups: Vec<BackupItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RestoreBackupRequest {
    pub filename: String,
}

/// Format bytes into human-readable representation
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Resolves the local database backup directory.
/// Tries the configured directory (defaults to /backup or BACKUP_DIR env var).
/// If writing to that directory fails (e.g. unprivileged user outside docker or test env),
/// falls back to `./data/backup` or `./backup`.
pub fn resolve_backup_dir(config: &crate::config::Config) -> PathBuf {
    let configured_path = PathBuf::from(&config.backup_dir);
    if std::fs::create_dir_all(&configured_path).is_ok() {
        return configured_path;
    }

    // Fallback 1: directory relative to database path (e.g. data/backup)
    if let Some(path_str) = config.database_url.strip_prefix("sqlite://").or_else(|| config.database_url.strip_prefix("sqlite:")) {
        let clean_path = path_str.split('?').next().unwrap_or(path_str);
        if let Some(parent) = StdPath::new(clean_path).parent() {
            if !parent.as_os_str().is_empty() {
                let candidate = parent.join("backup");
                if std::fs::create_dir_all(&candidate).is_ok() {
                    return candidate;
                }
            }
        }
    }

    // Fallback 2: local ./backup directory
    let local = PathBuf::from("./backup");
    let _ = std::fs::create_dir_all(&local);
    local
}

/// Prune backups older than retention_days (default: 7 days)
pub async fn prune_old_backups(backup_dir: &StdPath, retention_days: i64) -> usize {
    let mut pruned_count = 0;
    let cutoff = SystemTime::now() - Duration::from_secs(retention_days.max(1) as u64 * 86400);

    if let Ok(entries) = std::fs::read_dir(backup_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(fname) = path.file_name().and_then(|n| n.to_str()) {
                    if fname.starts_with("bingesync_backup_") && fname.ends_with(".db") {
                        let is_old = if let Ok(meta) = entry.metadata() {
                            if let Ok(modified) = meta.modified() {
                                modified < cutoff
                            } else {
                                false
                            }
                        } else {
                            false
                        };

                        if is_old {
                            if let Ok(()) = std::fs::remove_file(&path) {
                                pruned_count += 1;
                                tracing::info!(
                                    filename = %fname,
                                    retention_days = retention_days,
                                    "Pruned expired database backup"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    pruned_count
}

/// Creates a live, consistent SQLite database backup into the backup folder
pub async fn create_database_backup(state: &AppState, prefix: Option<&str>) -> Result<BackupItem, String> {
    let backup_dir = resolve_backup_dir(&state.config);
    let now = chrono::Utc::now();
    let timestamp = now.format("%Y-%m-%d_%H-%M-%S").to_string();

    let base_name = match prefix {
        Some(p) if !p.trim().is_empty() => format!("bingesync_backup_{}_{}", p.trim(), timestamp),
        _ => format!("bingesync_backup_{}", timestamp),
    };

    let mut filename = format!("{}.db", base_name);
    let mut target_path = backup_dir.join(&filename);
    let mut counter = 1;
    while target_path.exists() {
        filename = format!("{}_{}.db", base_name, counter);
        target_path = backup_dir.join(&filename);
        counter += 1;
    }
    let target_str = target_path.to_str().ok_or_else(|| "Invalid target path".to_string())?;

    tracing::info!(target_path = %target_str, "Initiating live database backup via VACUUM INTO");

    // Execute SQLite VACUUM INTO for live, transaction-consistent single-file backup
    let escaped_path = target_str.replace('\'', "''");
    let query_str = format!("VACUUM INTO '{}'", escaped_path);

    sqlx::query(&query_str).execute(&state.pool).await.map_err(|e| {
        tracing::warn!(error = %e, target_path = %target_str, "Failed to execute VACUUM INTO database backup");
        format!("Failed to create database backup: {}", e)
    })?;

    let size_bytes = std::fs::metadata(&target_path)
        .map(|m| m.len())
        .unwrap_or(0);

    let backup_item = BackupItem {
        filename: filename.clone(),
        size_bytes,
        size_formatted: format_bytes(size_bytes),
        created_at: now.to_rfc3339(),
    };

    tracing::info!(
        filename = %filename,
        size_bytes = size_bytes,
        size = %backup_item.size_formatted,
        "Live database backup successfully created"
    );

    // Prune backups older than 7 days
    let pruned = prune_old_backups(&backup_dir, 7).await;
    if pruned > 0 {
        tracing::info!(pruned_count = pruned, "Retention cleanup: pruned old backups");
    }

    Ok(backup_item)
}

/// List available database backups sorted by creation date descending
pub async fn list_backups(state: &AppState) -> Result<BackupsListResponse, String> {
    let backup_dir = resolve_backup_dir(&state.config);
    let mut backups = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&backup_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(fname) = path.file_name().and_then(|n| n.to_str()) {
                    if fname.starts_with("bingesync_backup_") && fname.ends_with(".db") {
                        if let Ok(meta) = entry.metadata() {
                            let size_bytes = meta.len();
                            let created_at = meta
                                .modified()
                                .ok()
                                .map(|t| {
                                    let dt: chrono::DateTime<chrono::Utc> = t.into();
                                    dt.to_rfc3339()
                                })
                                .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

                            backups.push(BackupItem {
                                filename: fname.to_string(),
                                size_bytes,
                                size_formatted: format_bytes(size_bytes),
                                created_at,
                            });
                        }
                    }
                }
            }
        }
    }

    // Sort newest first
    backups.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    Ok(BackupsListResponse {
        backup_dir: backup_dir.to_string_lossy().to_string(),
        backups,
    })
}

/// Restores the database from a specified backup file.
/// Performs safety checks:
/// 1. Sanitizes filename (guards against path traversal).
/// 2. Takes an automatic pre-restore safety backup of current data.
/// 3. Restores database content transactionally.
/// 4. Re-applies migrations to guarantee full schema alignment.
pub async fn restore_database_backup(state: &AppState, filename: &str) -> Result<String, String> {
    let clean_name = filename.trim();

    // Guard against path traversal
    if clean_name.is_empty()
        || clean_name.contains('/')
        || clean_name.contains('\\')
        || clean_name.contains("..")
        || !clean_name.starts_with("bingesync_backup_")
        || !clean_name.ends_with(".db")
    {
        return Err("Invalid backup filename".to_string());
    }

    let backup_dir = resolve_backup_dir(&state.config);
    let backup_path = backup_dir.join(clean_name);

    if !backup_path.is_file() {
        return Err(format!("Backup file '{}' not found", clean_name));
    }

    tracing::info!(
        filename = %clean_name,
        "Starting database restore process"
    );

    // 1. Create a pre-restore safety backup before applying the restore
    let safety_backup = create_database_backup(state, Some("pre_restore")).await;
    match safety_backup {
        Ok(item) => {
            tracing::info!(safety_backup = %item.filename, "Safety pre-restore backup created successfully");
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to create pre-restore safety backup, continuing with restore");
        }
    }

    // 2. Perform transactional database restore via SQLite ATTACH on a dedicated connection
    let backup_path_str = backup_path
        .to_str()
        .ok_or_else(|| "Invalid backup path encoding".to_string())?;
    let escaped_path = backup_path_str.replace('\'', "''");

    let mut conn = state
        .pool
        .acquire()
        .await
        .map_err(|e| format!("Failed to acquire database connection: {}", e))?;

    sqlx::query(&format!("ATTACH DATABASE '{}' AS backup_db", escaped_path))
        .execute(&mut *conn)
        .await
        .map_err(|e| format!("Failed to attach backup database: {}", e))?;

    let restore_result = async {
        // Temporarily disable foreign keys during restore
        sqlx::query("PRAGMA foreign_keys = OFF").execute(&mut *conn).await?;

        // Discover all user tables present in the backup database
        #[derive(sqlx::FromRow)]
        struct TableRow {
            name: String,
        }
        let backup_tables: Vec<TableRow> = sqlx::query_as(
            "SELECT name FROM backup_db.sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'"
        )
        .fetch_all(&mut *conn)
        .await?;

        // Clear existing tables in main database in dependent order
        for table in ["active_sessions", "scrobbles", "user_settings", "oidc_states", "plex_user_cache", "system_settings", "users"] {
            let _ = sqlx::query(&format!("DELETE FROM main.{}", table)).execute(&mut *conn).await;
        }

        // Copy records for each table that exists in the backup
        for t in backup_tables {
            let table = t.name;

            #[derive(sqlx::FromRow)]
            struct ColInfo {
                name: String,
            }

            let main_cols: Vec<String> = sqlx::query_as::<_, ColInfo>(&format!("PRAGMA main.table_info({})", table))
                .fetch_all(&mut *conn)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|c| c.name)
                .collect();

            let bkp_cols: Vec<String> = sqlx::query_as::<_, ColInfo>(&format!("PRAGMA backup_db.table_info({})", table))
                .fetch_all(&mut *conn)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|c| c.name)
                .collect();

            let common_cols: Vec<String> = main_cols
                .into_iter()
                .filter(|c| bkp_cols.contains(c))
                .collect();

            if !common_cols.is_empty() {
                let col_names = common_cols
                    .iter()
                    .map(|c| format!("\"{}\"", c))
                    .collect::<Vec<_>>()
                    .join(", ");
                let insert_sql = format!(
                    "INSERT INTO main.\"{}\" ({}) SELECT {} FROM backup_db.\"{}\"",
                    table, col_names, col_names, table
                );
                sqlx::query(&insert_sql).execute(&mut *conn).await?;
            }
        }

        // Re-enable foreign keys
        sqlx::query("PRAGMA foreign_keys = ON").execute(&mut *conn).await?;

        Ok::<(), sqlx::Error>(())
    }
    .await;

    // Detach backup database
    let _ = sqlx::query("DETACH DATABASE backup_db").execute(&mut *conn).await;
    drop(conn);

    restore_result.map_err(|e| {
        tracing::warn!(error = %e, "Failed to restore database from backup");
        format!("Failed to restore database: {}", e)
    })?;

    // Re-run embedded migrations to ensure complete schema compliance
    crate::db::migrations::run_migrations(&state.pool).await.map_err(|e| {
        tracing::warn!(error = %e, "Failed to run migrations after restore");
        format!("Failed to run migrations after restore: {}", e)
    })?;

    tracing::info!(filename = %clean_name, "Database restored successfully from backup");
    Ok(format!("Database successfully restored from '{}'", clean_name))
}

/// Background worker loop for periodic database backups
pub fn start_db_backup_worker(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Initial delay after startup before running first check
        tokio::time::sleep(Duration::from_secs(5)).await;

        loop {
            let interval_hours = state.get_task_interval_hours("db_backup").await;

            state
                .task_tracker
                .set_db_backup_running(Some(&state.pool), "Creating scheduled database backup...")
                .await;

            match create_database_backup(&state, None).await {
                Ok(item) => {
                    state
                        .task_tracker
                        .set_db_backup_idle(Some(&state.pool), 1, &format!("Backup created: {}", item.filename))
                        .await;
                }
                Err(e) => {
                    state
                        .task_tracker
                        .set_db_backup_idle(Some(&state.pool), 0, &format!("Backup failed: {}", e))
                        .await;
                }
            }

            let next_run = (chrono::Utc::now() + chrono::Duration::hours(interval_hours as i64)).to_rfc3339();
            state.task_tracker.set_db_backup_next_run(Some(next_run)).await;

            tokio::select! {
                _ = state.db_backup_notify.notified() => {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                _ = tokio::time::sleep(Duration::from_secs(interval_hours * 3600)) => {}
            }
        }
    })
}

// ==========================================
// API Handlers (Admin Only)
// ==========================================

/// GET /api/v1/admin/backups
pub async fn get_admin_backups_handler(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if auth.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Admin privileges required to view backups" })),
        ));
    }

    match list_backups(&state).await {
        Ok(resp) => Ok(Json(serde_json::to_value(resp).unwrap_or_default())),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e })),
        )),
    }
}

/// POST /api/v1/admin/backups/create
pub async fn create_admin_backup_handler(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if auth.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Admin privileges required to create database backups" })),
        ));
    }

    state.task_tracker.set_db_backup_running(Some(&state.pool), "Manual backup triggered...").await;

    match create_database_backup(&state, None).await {
        Ok(item) => {
            state
                .task_tracker
                .set_db_backup_idle(Some(&state.pool), 1, &format!("Backup created: {}", item.filename))
                .await;
            state.log_buffer.push("INFO", "bingesync::backup", &format!("Database backup created: {}", item.filename));

            Ok(Json(serde_json::json!({
                "status": "success",
                "backup": item,
                "message": "Database backup created successfully"
            })))
        }
        Err(e) => {
            state
                .task_tracker
                .set_db_backup_idle(Some(&state.pool), 0, &format!("Backup failed: {}", e))
                .await;
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            ))
        }
    }
}

/// POST /api/v1/admin/backups/restore
pub async fn restore_admin_backup_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(payload): Json<RestoreBackupRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if auth.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Admin privileges required to restore database backups" })),
        ));
    }

    match restore_database_backup(&state, &payload.filename).await {
        Ok(msg) => {
            state.log_buffer.push("INFO", "bingesync::backup", &format!("Database restored from backup: {}", payload.filename));
            Ok(Json(serde_json::json!({
                "status": "restored",
                "filename": payload.filename,
                "message": msg
            })))
        }
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e })),
        )),
    }
}

/// DELETE /api/v1/admin/backups/{filename}
pub async fn delete_admin_backup_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(filename): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if auth.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Admin privileges required to delete backups" })),
        ));
    }

    let clean_name = filename.trim();
    if clean_name.is_empty()
        || clean_name.contains('/')
        || clean_name.contains('\\')
        || clean_name.contains("..")
        || !clean_name.starts_with("bingesync_backup_")
        || !clean_name.ends_with(".db")
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Invalid backup filename" })),
        ));
    }

    let backup_dir = resolve_backup_dir(&state.config);
    let target = backup_dir.join(clean_name);

    if !target.is_file() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Backup file '{}' not found", clean_name) })),
        ));
    }

    if let Err(e) = std::fs::remove_file(&target) {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to delete backup file: {}", e) })),
        ));
    }

    state.log_buffer.push("INFO", "bingesync::backup", &format!("Deleted backup file: {}", clean_name));

    Ok(Json(serde_json::json!({
        "status": "deleted",
        "filename": clean_name,
        "message": format!("Backup file '{}' deleted successfully", clean_name)
    })))
}
