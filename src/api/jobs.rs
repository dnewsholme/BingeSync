use axum::{
    extract::{Path, State},
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
            tvdb_next_run: RwLock::new(next_str),
            tvdb_last_message: RwLock::new(Some("Worker initialized. Ready for scrobbles.".to_string())),
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

        vec![image_task, tvdb_task]
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

pub async fn trigger_admin_task_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(task_id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if auth.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Admin privileges required to trigger background tasks" })),
        ));
    }

    match task_id.as_str() {
        "image_cache" | "images" => {
            state.task_tracker.set_image_running("Manual run triggered...").await;
            state.image_notify.notify_waiters();
            state.image_notify.notify_one();
            state.log_buffer.push("INFO", "bingesync::tasks", "Manual run triggered for Image Caching worker");
            Ok(Json(serde_json::json!({
                "status": "triggered",
                "task_id": "image_cache",
                "message": "Image cache worker has been signaled to run immediately"
            })))
        }
        "tvdb_metadata" | "tvdb" => {
            state.task_tracker.set_tvdb_running("Manual run triggered...").await;
            state.metadata_notify.notify_waiters();
            state.metadata_notify.notify_one();
            state.log_buffer.push("INFO", "bingesync::tasks", "Manual run triggered for TheTVDB Metadata worker");
            Ok(Json(serde_json::json!({
                "status": "triggered",
                "task_id": "tvdb_metadata",
                "message": "TheTVDB metadata enrichment worker has been signaled to run immediately"
            })))
        }
        _ => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Unknown background task: {}", task_id) })),
        )),
    }
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
