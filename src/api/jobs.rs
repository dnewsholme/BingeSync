use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::RwLock;

use crate::{api::AppState, auth::AuthUser};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundTaskInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub status: String, // "idle", "running", "disabled"
    pub items_processed: usize,
    pub items_pending: usize,
    pub last_run_at: Option<String>,
    pub last_success_at: Option<String>,
    pub last_message: Option<String>,
}

#[derive(Debug)]
pub struct BackgroundTaskTracker {
    pub image_status: RwLock<String>,
    pub image_processed: AtomicUsize,
    pub image_last_run: RwLock<Option<String>>,
    pub image_last_success: RwLock<Option<String>>,
    pub image_last_message: RwLock<Option<String>>,

    pub tvdb_status: RwLock<String>,
    pub tvdb_processed: AtomicUsize,
    pub tvdb_last_run: RwLock<Option<String>>,
    pub tvdb_last_success: RwLock<Option<String>>,
    pub tvdb_last_message: RwLock<Option<String>>,
}

impl Default for BackgroundTaskTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl BackgroundTaskTracker {
    pub fn new() -> Self {
        Self {
            image_status: RwLock::new("idle".to_string()),
            image_processed: AtomicUsize::new(0),
            image_last_run: RwLock::new(None),
            image_last_success: RwLock::new(None),
            image_last_message: RwLock::new(Some("Worker initialized. Ready for scrobbles.".to_string())),

            tvdb_status: RwLock::new("idle".to_string()),
            tvdb_processed: AtomicUsize::new(0),
            tvdb_last_run: RwLock::new(None),
            tvdb_last_success: RwLock::new(None),
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

    pub async fn get_tasks_summary(&self, pool: &SqlitePool, tvdb_configured: bool) -> Vec<BackgroundTaskInfo> {
        let pending_images: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM scrobbles WHERE thumb_url IS NULL OR thumb_url = ''"
        )
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        let pending_metadata: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM scrobbles WHERE genres IS NULL OR overview IS NULL"
        )
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        let image_task = BackgroundTaskInfo {
            id: "image_cache".to_string(),
            name: "Media Poster & Thumbnail Caching".to_string(),
            description: "Discovers scrobbles lacking posters, searches for artwork, downloads to disk, and updates scrobble records.".to_string(),
            status: self.image_status.read().await.clone(),
            items_processed: self.image_processed.load(Ordering::Relaxed),
            items_pending: pending_images.max(0) as usize,
            last_run_at: self.image_last_run.read().await.clone(),
            last_success_at: self.image_last_success.read().await.clone(),
            last_message: self.image_last_message.read().await.clone(),
        };

        let tvdb_task = BackgroundTaskInfo {
            id: "tvdb_metadata".to_string(),
            name: "TheTVDB v4 Metadata Enrichment".to_string(),
            description: "Fetches official genres, actor cast lists, director credits, and episode/film plot overviews from TheTVDB.".to_string(),
            status: if tvdb_configured { self.tvdb_status.read().await.clone() } else { "disabled (unconfigured)".to_string() },
            items_processed: self.tvdb_processed.load(Ordering::Relaxed),
            items_pending: pending_metadata.max(0) as usize,
            last_run_at: self.tvdb_last_run.read().await.clone(),
            last_success_at: self.tvdb_last_success.read().await.clone(),
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

    let tasks = state.task_tracker.get_tasks_summary(&state.pool, tvdb_configured).await;

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
            state.image_notify.notify_one();
            state.log_buffer.push("INFO", "bingesync::tasks", "Manual run triggered for Image Caching worker");
            Ok(Json(serde_json::json!({
                "status": "triggered",
                "task_id": "image_cache",
                "message": "Image cache worker has been signaled to run immediately"
            })))
        }
        "tvdb_metadata" | "tvdb" => {
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
