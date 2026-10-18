use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::RwLock;

use crate::{api::AppState, auth::AuthUser};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundTaskInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub status: String, // "idle", "running", "disabled"
    pub schedule: String, // e.g. "Every 4 hours"
    pub interval_hours: u64,
    pub items_processed: usize,
    pub items_pending: usize,
    pub last_run_at: Option<String>,
    pub last_success_at: Option<String>,
    pub next_run_at: Option<String>,
    pub last_message: Option<String>,
}

#[derive(Debug)]
pub struct BackgroundTaskTracker {
    pub image_status: RwLock<String>,
    pub image_processed: AtomicUsize,
    pub image_last_run: RwLock<Option<String>>,
    pub image_last_success: RwLock<Option<String>>,
    pub image_next_run: RwLock<Option<String>>,
    pub image_last_message: RwLock<Option<String>>,

    pub tvdb_status: RwLock<String>,
    pub tvdb_processed: AtomicUsize,
    pub tvdb_last_run: RwLock<Option<String>>,
    pub tvdb_last_success: RwLock<Option<String>>,
    pub tvdb_next_run: RwLock<Option<String>>,
    pub tvdb_last_message: RwLock<Option<String>>,

    pub user_sync_status: RwLock<String>,
    pub user_sync_processed: AtomicUsize,
    pub user_sync_last_run: RwLock<Option<String>>,
    pub user_sync_last_success: RwLock<Option<String>>,
    pub user_sync_next_run: RwLock<Option<String>>,
    pub user_sync_last_message: RwLock<Option<String>>,
}

impl Default for BackgroundTaskTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl BackgroundTaskTracker {
    pub fn new() -> Self {
        let initial_next = chrono::Utc::now() + chrono::Duration::hours(4);
        let next_str = Some(initial_next.to_rfc3339());
        Self {
            image_status: RwLock::new("idle".to_string()),
            image_processed: AtomicUsize::new(0),
            image_last_run: RwLock::new(None),
            image_last_success: RwLock::new(None),
            image_next_run: RwLock::new(next_str.clone()),
            image_last_message: RwLock::new(Some("Worker initialized. Ready for scrobbles.".to_string())),

            tvdb_status: RwLock::new("idle".to_string()),
            tvdb_processed: AtomicUsize::new(0),
            tvdb_last_run: RwLock::new(None),
            tvdb_last_success: RwLock::new(None),
            tvdb_next_run: RwLock::new(next_str.clone()),
            tvdb_last_message: RwLock::new(Some("Worker initialized. Ready for scrobbles.".to_string())),

            user_sync_status: RwLock::new("idle".to_string()),
            user_sync_processed: AtomicUsize::new(0),
            user_sync_last_run: RwLock::new(None),
            user_sync_last_success: RwLock::new(None),
            user_sync_next_run: RwLock::new(next_str),
            user_sync_last_message: RwLock::new(Some("Worker initialized. Ready to sync user caches.".to_string())),
        }
    }

    pub async fn set_image_running(&self, message: &str) {
        *self.image_status.write().await = "running".to_string();
        *self.image_last_run.write().await = Some(chrono::Utc::now().to_rfc3339());
        *self.image_last_message.write().await = Some(message.to_string());
    }

    pub async fn set_image_idle(&self, processed: usize, message: &str) {
        *self.image_status.write().await = "idle".to_string();
        if processed > 0 {
            self.image_processed.fetch_add(processed, Ordering::Relaxed);
            *self.image_last_success.write().await = Some(chrono::Utc::now().to_rfc3339());
        }
        *self.image_last_message.write().await = Some(message.to_string());
    }

    pub async fn set_image_next_run(&self, next_run: Option<String>) {
        *self.image_next_run.write().await = next_run;
    }

    pub async fn set_tvdb_running(&self, message: &str) {
        *self.tvdb_status.write().await = "running".to_string();
        *self.tvdb_last_run.write().await = Some(chrono::Utc::now().to_rfc3339());
        *self.tvdb_last_message.write().await = Some(message.to_string());
    }

    pub async fn set_tvdb_idle(&self, processed: usize, message: &str) {
        *self.tvdb_status.write().await = "idle".to_string();
        if processed > 0 {
            self.tvdb_processed.fetch_add(processed, Ordering::Relaxed);
            *self.tvdb_last_success.write().await = Some(chrono::Utc::now().to_rfc3339());
        }
        *self.tvdb_last_message.write().await = Some(message.to_string());
    }

    pub async fn set_tvdb_next_run(&self, next_run: Option<String>) {
        *self.tvdb_next_run.write().await = next_run;
    }

    pub async fn set_user_sync_running(&self, message: &str) {
        *self.user_sync_status.write().await = "running".to_string();
        *self.user_sync_last_run.write().await = Some(chrono::Utc::now().to_rfc3339());
        *self.user_sync_last_message.write().await = Some(message.to_string());
    }

    pub async fn set_user_sync_idle(&self, processed: usize, message: &str) {
        *self.user_sync_status.write().await = "idle".to_string();
        if processed > 0 {
            self.user_sync_processed.fetch_add(processed, Ordering::Relaxed);
            *self.user_sync_last_success.write().await = Some(chrono::Utc::now().to_rfc3339());
        }
        *self.user_sync_last_message.write().await = Some(message.to_string());
    }

    pub async fn set_user_sync_next_run(&self, next_run: Option<String>) {
        *self.user_sync_next_run.write().await = next_run;
    }

    pub async fn get_tasks_summary(&self, state: &AppState, tvdb_configured: bool) -> Vec<BackgroundTaskInfo> {
        let pool = &state.pool;
        let pending_images: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM scrobbles WHERE thumb_url IS NULL OR thumb_url = '' OR thumb_url LIKE '/library/%' OR thumb_url LIKE '/photo/%'"
        )
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        let processed_images: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM scrobbles WHERE thumb_url IS NOT NULL AND thumb_url != '' AND thumb_url NOT LIKE '/library/%' AND thumb_url NOT LIKE '/photo/%'"
        )
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        let pending_metadata: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM scrobbles WHERE metadata_updated_at IS NULL AND ((genres IS NULL OR genres = '') OR (overview IS NULL OR overview = '') OR (actors IS NULL OR actors = ''))"
        )
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        let processed_metadata: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM scrobbles WHERE metadata_updated_at IS NOT NULL"
        )
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        let image_interval = state.get_task_interval_hours("image_cache").await;
        let tvdb_interval = state.get_task_interval_hours("tvdb_metadata").await;

        let image_sched_str = format!("Every {} hour{}", image_interval, if image_interval == 1 { "" } else { "s" });
        let tvdb_sched_str = format!("Every {} hour{}", tvdb_interval, if tvdb_interval == 1 { "" } else { "s" });

        let image_task = BackgroundTaskInfo {
            id: "image_cache".to_string(),
            name: "Media Poster & Thumbnail Caching".to_string(),
            description: "Discovers scrobbles lacking posters, searches for artwork, downloads to disk, and updates scrobble records.".to_string(),
            status: self.image_status.read().await.clone(),
            schedule: image_sched_str,
            interval_hours: image_interval,
            items_processed: (processed_images.max(0) as usize).max(self.image_processed.load(Ordering::Relaxed)),
            items_pending: pending_images.max(0) as usize,
            last_run_at: self.image_last_run.read().await.clone(),
            last_success_at: self.image_last_success.read().await.clone(),
            next_run_at: self.image_next_run.read().await.clone(),
            last_message: self.image_last_message.read().await.clone(),
        };

        let tvdb_task = BackgroundTaskInfo {
            id: "tvdb_metadata".to_string(),
            name: "TheTVDB v4 Metadata Enrichment".to_string(),
            description: "Fetches official genres, actor cast lists, director credits, and episode/plot overviews from TheTVDB.".to_string(),
            status: if tvdb_configured { self.tvdb_status.read().await.clone() } else { "disabled (unconfigured)".to_string() },
            schedule: tvdb_sched_str,
            interval_hours: tvdb_interval,
            items_processed: (processed_metadata.max(0) as usize).max(self.tvdb_processed.load(Ordering::Relaxed)),
            items_pending: pending_metadata.max(0) as usize,
            last_run_at: self.tvdb_last_run.read().await.clone(),
            last_success_at: self.tvdb_last_success.read().await.clone(),
            next_run_at: self.tvdb_next_run.read().await.clone(),
            last_message: self.tvdb_last_message.read().await.clone(),
        };

        let user_sync_interval = state.get_task_interval_hours("user_sync").await;
        let user_sync_sched_str = format!("Every {} hour{}", user_sync_interval, if user_sync_interval == 1 { "" } else { "s" });

        let cached_users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM plex_user_cache")
            .fetch_one(pool)
            .await
            .unwrap_or(0);

        let user_sync_task = BackgroundTaskInfo {
            id: "user_sync".to_string(),
            name: "Plex & Media Server User Cache Sync".to_string(),
            description: "Discovers and syncs Plex and Jellyfin user IDs and usernames into local cache for reliable webhook filtering and mapping.".to_string(),
            status: self.user_sync_status.read().await.clone(),
            schedule: user_sync_sched_str,
            interval_hours: user_sync_interval,
            items_processed: (cached_users.max(0) as usize).max(self.user_sync_processed.load(Ordering::Relaxed)),
            items_pending: 0,
            last_run_at: self.user_sync_last_run.read().await.clone(),
            last_success_at: self.user_sync_last_success.read().await.clone(),
            next_run_at: self.user_sync_next_run.read().await.clone(),
            last_message: self.user_sync_last_message.read().await.clone(),
        };

        vec![image_task, tvdb_task, user_sync_task]
    }
}

pub async fn get_admin_tasks_handler(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if auth.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Admin privileges required to view background task status" })),
        ));
    }

    let tvdb_key = state.get_effective_tvdb_api_key().await;
    let tvdb_configured = tvdb_key.is_some() && !tvdb_key.as_deref().unwrap_or("").is_empty();

    let tasks = state.task_tracker.get_tasks_summary(&state, tvdb_configured).await;

    Ok(Json(serde_json::json!({
        "tasks": tasks
    })))
}

#[derive(Debug, Deserialize, Default)]
pub struct TriggerTaskQuery {
    pub force: Option<bool>,
}

pub async fn trigger_admin_task_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(task_id): Path<String>,
    Query(query): Query<TriggerTaskQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if auth.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Admin privileges required to trigger background tasks" })),
        ));
    }

    match task_id.as_str() {
        "image_cache" | "images" => {
            let is_forced = query.force.unwrap_or(false);
            if is_forced {
                force_refresh_all_images_internal(&state).await
            } else {
                state.task_tracker.set_image_running("Manual run triggered...").await;
                state.image_notify.notify_waiters();
                state.image_notify.notify_one();
                state.log_buffer.push("INFO", "bingesync::tasks", "Manual run triggered for Image Caching worker");
                Ok(Json(serde_json::json!({
                    "status": "triggered",
                    "task_id": "image_cache",
                    "forced": false,
                    "message": "Image cache worker has been signaled to run immediately"
                })))
            }
        }
        "force_images_refresh" => {
            force_refresh_all_images_internal(&state).await
        }
        "tvdb_metadata" | "tvdb" => {
            let is_forced = query.force.unwrap_or(false);
            if is_forced {
                let _ = sqlx::query("UPDATE scrobbles SET metadata_updated_at = NULL")
                    .execute(&state.pool)
                    .await;
                state.task_tracker.set_tvdb_running("Forced full metadata refresh triggered...").await;
                state.metadata_notify.notify_waiters();
                state.metadata_notify.notify_one();
                state.log_buffer.push(
                    "INFO",
                    "bingesync::tasks",
                    "Forced full metadata refresh triggered: cleared metadata timestamps for all scrobbles",
                );
                Ok(Json(serde_json::json!({
                    "status": "triggered",
                    "task_id": "tvdb_metadata",
                    "forced": true,
                    "message": "TheTVDB metadata worker signaled to re-download metadata for all records"
                })))
            } else {
                // Reset metadata_updated_at for scrobbles still missing core metadata so they get re-evaluated
                let _ = sqlx::query(
                    "UPDATE scrobbles SET metadata_updated_at = NULL WHERE (overview IS NULL OR overview = '') OR (genres IS NULL OR genres = '') OR (actors IS NULL OR actors = '')"
                )
                .execute(&state.pool)
                .await;

                state.task_tracker.set_tvdb_running("Manual run triggered...").await;
                state.metadata_notify.notify_waiters();
                state.metadata_notify.notify_one();
                state.log_buffer.push("INFO", "bingesync::tasks", "Manual run triggered for TheTVDB Metadata worker");
                Ok(Json(serde_json::json!({
                    "status": "triggered",
                    "task_id": "tvdb_metadata",
                    "forced": false,
                    "message": "TheTVDB metadata enrichment worker has been signaled to run immediately"
                })))
            }
        }
        "force_metadata_refresh" => {
            force_refresh_all_metadata_internal(&state).await
        }
        "user_sync" | "users" | "plex_users" => {
            let is_forced = query.force.unwrap_or(false);
            if is_forced {
                force_refresh_user_cache_internal(&state).await
            } else {
                state.task_tracker.set_user_sync_running("Manual run triggered...").await;
                state.user_sync_notify.notify_waiters();
                state.user_sync_notify.notify_one();
                state.log_buffer.push("INFO", "bingesync::tasks", "Manual run triggered for User Cache Sync worker");
                Ok(Json(serde_json::json!({
                    "status": "triggered",
                    "task_id": "user_sync",
                    "forced": false,
                    "message": "User cache sync worker has been signaled to run immediately"
                })))
            }
        }
        "force_user_sync_refresh" => {
            force_refresh_user_cache_internal(&state).await
        }
        _ => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Unknown background task: {}", task_id) })),
        )),
    }
}

pub async fn force_refresh_all_images_internal(
    state: &AppState,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let res = sqlx::query("UPDATE scrobbles SET thumb_url = NULL")
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Database error resetting poster thumbnails: {}", e) })),
            )
        })?;

    let count = res.rows_affected();
    state.task_tracker.set_image_running("Forced full poster refresh triggered...").await;
    state.image_notify.notify_waiters();
    state.image_notify.notify_one();
    state.log_buffer.push(
        "INFO",
        "bingesync::tasks",
        &format!("Forced full poster refresh triggered: reset {} records", count),
    );

    Ok(Json(serde_json::json!({
        "status": "triggered",
        "task_id": "image_cache",
        "records_reset": count,
        "forced": true,
        "message": format!("Thumbnails reset for {} records; image cache worker signaled to re-download all posters", count)
    })))
}

pub async fn force_refresh_all_images_handler(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if auth.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Admin privileges required to force refresh poster images" })),
        ));
    }
    force_refresh_all_images_internal(&state).await
}

pub async fn force_refresh_user_cache_internal(
    state: &AppState,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let res = sqlx::query("DELETE FROM plex_user_cache")
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Database error clearing user cache: {}", e) })),
            )
        })?;

    let count = res.rows_affected();
    state.task_tracker.set_user_sync_running("Forced user cache refresh triggered...").await;
    state.user_sync_notify.notify_waiters();
    state.user_sync_notify.notify_one();
    state.log_buffer.push(
        "INFO",
        "bingesync::tasks",
        &format!("Forced user cache refresh triggered: cleared {} cached user mappings", count),
    );

    Ok(Json(serde_json::json!({
        "status": "triggered",
        "task_id": "user_sync",
        "records_reset": count,
        "forced": true,
        "message": format!("User cache cleared ({} records deleted); user sync worker signaled to re-sync all users", count)
    })))
}

pub async fn force_refresh_user_cache_handler(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if auth.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Admin privileges required to force refresh user cache" })),
        ));
    }
    force_refresh_user_cache_internal(&state).await
}

async fn force_refresh_all_metadata_internal(
    state: &AppState,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let res = sqlx::query("UPDATE scrobbles SET metadata_updated_at = NULL")
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Database error resetting metadata timestamps: {}", e) })),
            )
        })?;

    let count = res.rows_affected();
    state.task_tracker.set_tvdb_running("Forced full metadata refresh triggered...").await;
    state.metadata_notify.notify_waiters();
    state.metadata_notify.notify_one();
    state.log_buffer.push(
        "INFO",
        "bingesync::tasks",
        &format!("Forced full metadata refresh triggered: reset {} records", count),
    );

    Ok(Json(serde_json::json!({
        "status": "triggered",
        "task_id": "tvdb_metadata",
        "records_reset": count,
        "forced": true,
        "message": format!("Metadata reset for {} records; TVDB background worker signaled to re-download all metadata", count)
    })))
}

pub async fn force_refresh_all_metadata_handler(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if auth.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Admin privileges required to force refresh metadata" })),
        ));
    }
    force_refresh_all_metadata_internal(&state).await
}

#[derive(Debug, Deserialize)]
pub struct UpdateTaskScheduleRequest {
    pub interval_hours: Option<u64>,
}

pub async fn update_admin_task_schedule_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(task_id): Path<String>,
    Json(payload): Json<UpdateTaskScheduleRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if auth.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Admin privileges required to configure background tasks" })),
        ));
    }

    let hours = payload.interval_hours.unwrap_or(4).clamp(1, 168);
    state.set_task_interval_hours(&task_id, hours).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to update schedule: {}", e) })),
        )
    })?;

    let next_run = (chrono::Utc::now() + chrono::Duration::hours(hours as i64)).to_rfc3339();
    match task_id.as_str() {
        "image_cache" | "images" => {
            state.task_tracker.set_image_next_run(Some(next_run.clone())).await;
        }
        "tvdb_metadata" | "tvdb" => {
            state.task_tracker.set_tvdb_next_run(Some(next_run.clone())).await;
        }
        "user_sync" | "users" | "plex_users" => {
            state.task_tracker.set_user_sync_next_run(Some(next_run.clone())).await;
        }
        _ => {}
    }

    let sched_str = format!("Every {} hour{}", hours, if hours == 1 { "" } else { "s" });
    state.log_buffer.push("INFO", "bingesync::tasks", &format!("Updated schedule for {} to {}", task_id, sched_str));

    Ok(Json(serde_json::json!({
        "status": "updated",
        "task_id": task_id,
        "interval_hours": hours,
        "schedule": sched_str,
        "next_run_at": next_run,
    })))
}
