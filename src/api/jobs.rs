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

    pub db_backup_status: RwLock<String>,
    pub db_backup_processed: AtomicUsize,
    pub db_backup_last_run: RwLock<Option<String>>,
    pub db_backup_last_success: RwLock<Option<String>>,
    pub db_backup_next_run: RwLock<Option<String>>,
    pub db_backup_last_message: RwLock<Option<String>>,

    pub db_maintenance_status: RwLock<String>,
    pub db_maintenance_processed: AtomicUsize,
    pub db_maintenance_last_run: RwLock<Option<String>>,
    pub db_maintenance_last_success: RwLock<Option<String>>,
    pub db_maintenance_next_run: RwLock<Option<String>>,
    pub db_maintenance_last_message: RwLock<Option<String>>,

    pub tautulli_sync_status: RwLock<String>,
    pub tautulli_sync_processed: AtomicUsize,
    pub tautulli_sync_last_run: RwLock<Option<String>>,
    pub tautulli_sync_last_success: RwLock<Option<String>>,
    pub tautulli_sync_next_run: RwLock<Option<String>>,
    pub tautulli_sync_last_message: RwLock<Option<String>>,

    pub media_sync_status: RwLock<String>,
    pub media_sync_processed: AtomicUsize,
    pub media_sync_last_run: RwLock<Option<String>>,
    pub media_sync_last_success: RwLock<Option<String>>,
    pub media_sync_next_run: RwLock<Option<String>>,
    pub media_sync_last_message: RwLock<Option<String>>,
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
        let initial_backup_next = chrono::Utc::now() + chrono::Duration::hours(24);
        let initial_maintenance_next = chrono::Utc::now() + chrono::Duration::hours(168);
        let initial_tautulli_next = chrono::Utc::now() + chrono::Duration::hours(1);
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

            db_backup_status: RwLock::new("idle".to_string()),
            db_backup_processed: AtomicUsize::new(0),
            db_backup_last_run: RwLock::new(None),
            db_backup_last_success: RwLock::new(None),
            db_backup_next_run: RwLock::new(Some(initial_backup_next.to_rfc3339())),
            db_backup_last_message: RwLock::new(Some("Worker initialized. Ready for database backups.".to_string())),

            db_maintenance_status: RwLock::new("idle".to_string()),
            db_maintenance_processed: AtomicUsize::new(0),
            db_maintenance_last_run: RwLock::new(None),
            db_maintenance_last_success: RwLock::new(None),
            db_maintenance_next_run: RwLock::new(Some(initial_maintenance_next.to_rfc3339())),
            db_maintenance_last_message: RwLock::new(Some("Worker initialized. Ready for database maintenance.".to_string())),

            tautulli_sync_status: RwLock::new("idle".to_string()),
            tautulli_sync_processed: AtomicUsize::new(0),
            tautulli_sync_last_run: RwLock::new(None),
            tautulli_sync_last_success: RwLock::new(None),
            tautulli_sync_next_run: RwLock::new(Some(initial_tautulli_next.to_rfc3339())),
            tautulli_sync_last_message: RwLock::new(Some("Worker initialized. Ready to sync Tautulli history.".to_string())),

            media_sync_status: RwLock::new("idle".to_string()),
            media_sync_processed: AtomicUsize::new(0),
            media_sync_last_run: RwLock::new(None),
            media_sync_last_success: RwLock::new(None),
            media_sync_next_run: RwLock::new(Some(initial_next.to_rfc3339())),
            media_sync_last_message: RwLock::new(Some("Worker initialized. Ready to sync watch history to media servers.".to_string())),
        }
    }

    pub async fn set_image_running(&self, pool: Option<&sqlx::SqlitePool>, message: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        *self.image_status.write().await = "running".to_string();
        *self.image_last_run.write().await = Some(now.clone());
        *self.image_last_message.write().await = Some(message.to_string());
        if let Some(p) = pool {
            let _ = crate::api::settings::set_db_setting(p, "task_last_run:image_cache", &now).await;
        }
    }

    pub async fn set_image_idle(&self, pool: Option<&sqlx::SqlitePool>, processed: usize, message: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        *self.image_status.write().await = "idle".to_string();
        self.image_processed.fetch_add(processed, Ordering::Relaxed);
        *self.image_last_success.write().await = Some(now.clone());
        *self.image_last_message.write().await = Some(message.to_string());
        if let Some(p) = pool {
            let _ = crate::api::settings::set_db_setting(p, "task_last_success:image_cache", &now).await;
        }
    }

    pub async fn set_image_next_run(&self, next_run: Option<String>) {
        *self.image_next_run.write().await = next_run;
    }

    pub async fn set_tvdb_running(&self, pool: Option<&sqlx::SqlitePool>, message: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        *self.tvdb_status.write().await = "running".to_string();
        *self.tvdb_last_run.write().await = Some(now.clone());
        *self.tvdb_last_message.write().await = Some(message.to_string());
        if let Some(p) = pool {
            let _ = crate::api::settings::set_db_setting(p, "task_last_run:tvdb_metadata", &now).await;
        }
    }

    pub async fn set_tvdb_idle(&self, pool: Option<&sqlx::SqlitePool>, processed: usize, message: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        *self.tvdb_status.write().await = "idle".to_string();
        self.tvdb_processed.fetch_add(processed, Ordering::Relaxed);
        *self.tvdb_last_success.write().await = Some(now.clone());
        *self.tvdb_last_message.write().await = Some(message.to_string());
        if let Some(p) = pool {
            let _ = crate::api::settings::set_db_setting(p, "task_last_success:tvdb_metadata", &now).await;
        }
    }

    pub async fn set_tvdb_idle_unconfigured(&self, message: &str) {
        *self.tvdb_status.write().await = "idle".to_string();
        *self.tvdb_last_message.write().await = Some(message.to_string());
    }

    pub async fn set_tvdb_next_run(&self, next_run: Option<String>) {
        *self.tvdb_next_run.write().await = next_run;
    }

    pub async fn set_user_sync_running(&self, pool: Option<&sqlx::SqlitePool>, message: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        *self.user_sync_status.write().await = "running".to_string();
        *self.user_sync_last_run.write().await = Some(now.clone());
        *self.user_sync_last_message.write().await = Some(message.to_string());
        if let Some(p) = pool {
            let _ = crate::api::settings::set_db_setting(p, "task_last_run:user_sync", &now).await;
        }
    }

    pub async fn set_user_sync_idle(&self, pool: Option<&sqlx::SqlitePool>, processed: usize, message: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        *self.user_sync_status.write().await = "idle".to_string();
        self.user_sync_processed.fetch_add(processed, Ordering::Relaxed);
        *self.user_sync_last_success.write().await = Some(now.clone());
        *self.user_sync_last_message.write().await = Some(message.to_string());
        if let Some(p) = pool {
            let _ = crate::api::settings::set_db_setting(p, "task_last_success:user_sync", &now).await;
        }
    }

    pub async fn set_user_sync_next_run(&self, next_run: Option<String>) {
        *self.user_sync_next_run.write().await = next_run;
    }

    pub async fn set_db_backup_running(&self, pool: Option<&sqlx::SqlitePool>, message: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        *self.db_backup_status.write().await = "running".to_string();
        *self.db_backup_last_run.write().await = Some(now.clone());
        *self.db_backup_last_message.write().await = Some(message.to_string());
        if let Some(p) = pool {
            let _ = crate::api::settings::set_db_setting(p, "task_last_run:db_backup", &now).await;
        }
    }

    pub async fn set_db_backup_idle(&self, pool: Option<&sqlx::SqlitePool>, processed: usize, message: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        *self.db_backup_status.write().await = "idle".to_string();
        self.db_backup_processed.fetch_add(processed, Ordering::Relaxed);
        if processed > 0 {
            *self.db_backup_last_success.write().await = Some(now.clone());
            if let Some(p) = pool {
                let _ = crate::api::settings::set_db_setting(p, "task_last_success:db_backup", &now).await;
            }
        }
        *self.db_backup_last_message.write().await = Some(message.to_string());
    }

    pub async fn set_db_backup_next_run(&self, next_run: Option<String>) {
        *self.db_backup_next_run.write().await = next_run;
    }

    pub async fn set_db_maintenance_running(&self, pool: Option<&sqlx::SqlitePool>, message: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        *self.db_maintenance_status.write().await = "running".to_string();
        *self.db_maintenance_last_run.write().await = Some(now.clone());
        *self.db_maintenance_last_message.write().await = Some(message.to_string());
        if let Some(p) = pool {
            let _ = crate::api::settings::set_db_setting(p, "task_last_run:db_maintenance", &now).await;
        }
    }

    pub async fn set_db_maintenance_idle(&self, pool: Option<&sqlx::SqlitePool>, processed: usize, message: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        *self.db_maintenance_status.write().await = "idle".to_string();
        self.db_maintenance_processed.fetch_add(processed, Ordering::Relaxed);
        if processed > 0 {
            *self.db_maintenance_last_success.write().await = Some(now.clone());
            if let Some(p) = pool {
                let _ = crate::api::settings::set_db_setting(p, "task_last_success:db_maintenance", &now).await;
            }
        }
        *self.db_maintenance_last_message.write().await = Some(message.to_string());
    }

    pub async fn set_db_maintenance_next_run(&self, next_run: Option<String>) {
        *self.db_maintenance_next_run.write().await = next_run;
    }

    pub async fn set_tautulli_sync_running(&self, pool: Option<&sqlx::SqlitePool>, message: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        *self.tautulli_sync_status.write().await = "running".to_string();
        *self.tautulli_sync_last_run.write().await = Some(now.clone());
        *self.tautulli_sync_last_message.write().await = Some(message.to_string());
        if let Some(p) = pool {
            let _ = crate::api::settings::set_db_setting(p, "task_last_run:tautulli_sync", &now).await;
        }
    }

    pub async fn set_tautulli_sync_idle(&self, pool: Option<&sqlx::SqlitePool>, processed: usize, message: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        *self.tautulli_sync_status.write().await = "idle".to_string();
        self.tautulli_sync_processed.fetch_add(processed, Ordering::Relaxed);
        *self.tautulli_sync_last_success.write().await = Some(now.clone());
        *self.tautulli_sync_last_message.write().await = Some(message.to_string());
        if let Some(p) = pool {
            let _ = crate::api::settings::set_db_setting(p, "task_last_success:tautulli_sync", &now).await;
        }
    }

    pub async fn set_tautulli_sync_next_run(&self, next_run: Option<String>) {
        *self.tautulli_sync_next_run.write().await = next_run;
    }

    pub async fn set_media_sync_running(&self, pool: Option<&sqlx::SqlitePool>, message: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        *self.media_sync_status.write().await = "running".to_string();
        *self.media_sync_last_run.write().await = Some(now.clone());
        *self.media_sync_last_message.write().await = Some(message.to_string());
        if let Some(p) = pool {
            let _ = crate::api::settings::set_db_setting(p, "task_last_run:media_sync", &now).await;
        }
    }

    pub async fn set_media_sync_idle(&self, pool: Option<&sqlx::SqlitePool>, processed: usize, message: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        *self.media_sync_status.write().await = "idle".to_string();
        self.media_sync_processed.fetch_add(processed, Ordering::Relaxed);
        *self.media_sync_last_success.write().await = Some(now.clone());
        *self.media_sync_last_message.write().await = Some(message.to_string());
        if let Some(p) = pool {
            let _ = crate::api::settings::set_db_setting(p, "task_last_success:media_sync", &now).await;
        }
    }

    pub async fn set_media_sync_next_run(&self, next_run: Option<String>) {
        *self.media_sync_next_run.write().await = next_run;
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

        let image_last_run = match self.image_last_run.read().await.clone() {
            Some(lr) => Some(lr),
            None => crate::api::settings::get_db_setting(pool, "task_last_run:image_cache").await,
        };

        let image_last_success = match self.image_last_success.read().await.clone() {
            Some(ls) => Some(ls),
            None => {
                if let Some(saved) = crate::api::settings::get_db_setting(pool, "task_last_success:image_cache").await {
                    Some(saved)
                } else {
                    let max_thumb: Option<String> = sqlx::query_scalar(
                        "SELECT MAX(created_at) FROM scrobbles WHERE thumb_url IS NOT NULL AND thumb_url != '' AND thumb_url NOT LIKE '/library/%' AND thumb_url NOT LIKE '/photo/%'"
                    )
                    .fetch_optional(pool)
                    .await
                    .unwrap_or(None);
                    max_thumb
                }
            }
        };

        let image_task = BackgroundTaskInfo {
            id: "image_cache".to_string(),
            name: "Media Poster & Thumbnail Caching".to_string(),
            description: "Discovers scrobbles lacking posters, searches for artwork, downloads to disk, and updates scrobble records.".to_string(),
            status: self.image_status.read().await.clone(),
            schedule: image_sched_str,
            interval_hours: image_interval,
            items_processed: (processed_images.max(0) as usize).max(self.image_processed.load(Ordering::Relaxed)),
            items_pending: pending_images.max(0) as usize,
            last_run_at: image_last_run,
            last_success_at: image_last_success,
            next_run_at: self.image_next_run.read().await.clone(),
            last_message: self.image_last_message.read().await.clone(),
        };

        let metadata_source = state.get_effective_metadata_source().await;
        let (meta_desc, meta_configured) = if metadata_source == "tmdb" {
            let tmdb_key = state.get_effective_tmdb_api_key().await;
            let tmdb_configured = tmdb_key.is_some() && !tmdb_key.as_deref().unwrap_or("").is_empty();
            (
                "Fetches official genres, actor cast lists, director credits, and episode/plot overviews from The Movie Database (TMDB).".to_string(),
                tmdb_configured
            )
        } else {
            (
                "Fetches official genres, actor cast lists, director credits, and episode/plot overviews from TheTVDB.".to_string(),
                tvdb_configured
            )
        };

        let tvdb_last_run = match self.tvdb_last_run.read().await.clone() {
            Some(lr) => Some(lr),
            None => crate::api::settings::get_db_setting(pool, "task_last_run:tvdb_metadata").await,
        };

        let tvdb_last_success = match self.tvdb_last_success.read().await.clone() {
            Some(ls) => Some(ls),
            None => {
                if let Some(saved) = crate::api::settings::get_db_setting(pool, "task_last_success:tvdb_metadata").await {
                    Some(saved)
                } else {
                    let max_meta: Option<String> = sqlx::query_scalar(
                        "SELECT MAX(metadata_updated_at) FROM scrobbles WHERE metadata_updated_at IS NOT NULL"
                    )
                    .fetch_optional(pool)
                    .await
                    .unwrap_or(None);
                    max_meta
                }
            }
        };

        let tvdb_task = BackgroundTaskInfo {
            id: "tvdb_metadata".to_string(),
            name: "Metadata Enrichment".to_string(),
            description: meta_desc,
            status: if meta_configured { self.tvdb_status.read().await.clone() } else { "disabled (unconfigured)".to_string() },
            schedule: tvdb_sched_str,
            interval_hours: tvdb_interval,
            items_processed: (processed_metadata.max(0) as usize).max(self.tvdb_processed.load(Ordering::Relaxed)),
            items_pending: pending_metadata.max(0) as usize,
            last_run_at: tvdb_last_run,
            last_success_at: tvdb_last_success,
            next_run_at: self.tvdb_next_run.read().await.clone(),
            last_message: self.tvdb_last_message.read().await.clone(),
        };

        let user_sync_interval = state.get_task_interval_hours("user_sync").await;
        let user_sync_sched_str = format!("Every {} hour{}", user_sync_interval, if user_sync_interval == 1 { "" } else { "s" });

        let cached_users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM plex_user_cache")
            .fetch_one(pool)
            .await
            .unwrap_or(0);

        let user_sync_last_run = match self.user_sync_last_run.read().await.clone() {
            Some(lr) => Some(lr),
            None => crate::api::settings::get_db_setting(pool, "task_last_run:user_sync").await,
        };

        let user_sync_last_success = match self.user_sync_last_success.read().await.clone() {
            Some(ls) => Some(ls),
            None => {
                if let Some(saved) = crate::api::settings::get_db_setting(pool, "task_last_success:user_sync").await {
                    Some(saved)
                } else {
                    let max_sync: Option<String> = sqlx::query_scalar(
                        "SELECT MAX(last_synced_at) FROM plex_user_cache"
                    )
                    .fetch_optional(pool)
                    .await
                    .unwrap_or(None);
                    max_sync
                }
            }
        };

        let user_sync_task = BackgroundTaskInfo {
            id: "user_sync".to_string(),
            name: "Plex & Media Server User Cache Sync".to_string(),
            description: "Discovers and syncs Plex and Jellyfin user IDs and usernames into local cache for reliable webhook filtering and mapping.".to_string(),
            status: self.user_sync_status.read().await.clone(),
            schedule: user_sync_sched_str,
            interval_hours: user_sync_interval,
            items_processed: (cached_users.max(0) as usize).max(self.user_sync_processed.load(Ordering::Relaxed)),
            items_pending: 0,
            last_run_at: user_sync_last_run,
            last_success_at: user_sync_last_success,
            next_run_at: self.user_sync_next_run.read().await.clone(),
            last_message: self.user_sync_last_message.read().await.clone(),
        };

        let db_backup_interval = state.get_task_interval_hours("db_backup").await;
        let db_backup_sched_str = format!("Every {} hour{}", db_backup_interval, if db_backup_interval == 1 { "" } else { "s" });

        let current_backups_count = crate::api::backup::list_backups(state)
            .await
            .map(|r| r.backups.len())
            .unwrap_or(0);

        let db_backup_last_run = match self.db_backup_last_run.read().await.clone() {
            Some(lr) => Some(lr),
            None => crate::api::settings::get_db_setting(pool, "task_last_run:db_backup").await,
        };

        let db_backup_last_success = match self.db_backup_last_success.read().await.clone() {
            Some(ls) => Some(ls),
            None => {
                if let Some(saved) = crate::api::settings::get_db_setting(pool, "task_last_success:db_backup").await {
                    Some(saved)
                } else {
                    crate::api::backup::list_backups(state)
                        .await
                        .ok()
                        .and_then(|r| r.backups.first().map(|b| b.created_at.clone()))
                }
            }
        };

        let db_backup_task = BackgroundTaskInfo {
            id: "db_backup".to_string(),
            name: "Database Backup".to_string(),
            description: "Creates live local SQLite database backups into the backup folder and automatically keeps 7 days worth of backups.".to_string(),
            status: self.db_backup_status.read().await.clone(),
            schedule: db_backup_sched_str,
            interval_hours: db_backup_interval,
            items_processed: current_backups_count,
            items_pending: 0,
            last_run_at: db_backup_last_run,
            last_success_at: db_backup_last_success,
            next_run_at: self.db_backup_next_run.read().await.clone(),
            last_message: self.db_backup_last_message.read().await.clone(),
        };

        let db_maintenance_interval = state.get_task_interval_hours("db_maintenance").await;
        let db_maintenance_sched_str = if db_maintenance_interval == 168 {
            "Every 168 hours (Weekly)".to_string()
        } else {
            format!("Every {} hour{}", db_maintenance_interval, if db_maintenance_interval == 1 { "" } else { "s" })
        };

        let db_maintenance_last_run = match self.db_maintenance_last_run.read().await.clone() {
            Some(lr) => Some(lr),
            None => crate::api::settings::get_db_setting(pool, "task_last_run:db_maintenance").await,
        };

        let db_maintenance_last_success = match self.db_maintenance_last_success.read().await.clone() {
            Some(ls) => Some(ls),
            None => crate::api::settings::get_db_setting(pool, "task_last_success:db_maintenance").await,
        };

        let db_maintenance_task = BackgroundTaskInfo {
            id: "db_maintenance".to_string(),
            name: "Database Maintenance & Optimization".to_string(),
            description: "Runs VACUUM, REINDEX, ANALYZE, and optimizes SQLite PRAGMAs (WAL mode, NORMAL synchronous, cache size, memory temp store).".to_string(),
            status: self.db_maintenance_status.read().await.clone(),
            schedule: db_maintenance_sched_str,
            interval_hours: db_maintenance_interval,
            items_processed: self.db_maintenance_processed.load(Ordering::Relaxed),
            items_pending: 0,
            last_run_at: db_maintenance_last_run,
            last_success_at: db_maintenance_last_success,
            next_run_at: self.db_maintenance_next_run.read().await.clone(),
            last_message: self.db_maintenance_last_message.read().await.clone(),
        };

        let tautulli_sync_interval = state.get_task_interval_hours("tautulli_sync").await;
        let tautulli_sync_sched_str = format!("Every {} hour{}", tautulli_sync_interval, if tautulli_sync_interval == 1 { "" } else { "s" });

        let tautulli_sync_last_run = match self.tautulli_sync_last_run.read().await.clone() {
            Some(lr) => Some(lr),
            None => crate::api::settings::get_db_setting(pool, "task_last_run:tautulli_sync").await,
        };

        let tautulli_sync_last_success = match self.tautulli_sync_last_success.read().await.clone() {
            Some(ls) => Some(ls),
            None => crate::api::settings::get_db_setting(pool, "task_last_success:tautulli_sync").await,
        };

        let tautulli_sync_task = BackgroundTaskInfo {
            id: "tautulli_sync".to_string(),
            name: "Tautulli Continuous 1-Way Sync".to_string(),
            description: "Periodically polls Tautulli API for user playback history and syncs completed watches into BingeSync with automatic deduplication.".to_string(),
            status: self.tautulli_sync_status.read().await.clone(),
            schedule: tautulli_sync_sched_str,
            interval_hours: tautulli_sync_interval,
            items_processed: self.tautulli_sync_processed.load(Ordering::Relaxed),
            items_pending: 0,
            last_run_at: tautulli_sync_last_run,
            last_success_at: tautulli_sync_last_success,
            next_run_at: self.tautulli_sync_next_run.read().await.clone(),
            last_message: self.tautulli_sync_last_message.read().await.clone(),
        };

        let media_sync_interval = state.get_task_interval_hours("media_sync").await;
        let media_sync_sched_str = format!("Every {} hour{}", media_sync_interval, if media_sync_interval == 1 { "" } else { "s" });

        let media_sync_last_run = match self.media_sync_last_run.read().await.clone() {
            Some(lr) => Some(lr),
            None => crate::api::settings::get_db_setting(pool, "task_last_run:media_sync").await,
        };

        let media_sync_last_success = match self.media_sync_last_success.read().await.clone() {
            Some(ls) => Some(ls),
            None => crate::api::settings::get_db_setting(pool, "task_last_success:media_sync").await,
        };

        let media_sync_task = BackgroundTaskInfo {
            id: "media_sync".to_string(),
            name: "Watch History Sync (Plex & Jellyfin)".to_string(),
            description: "Periodically synchronizes recent watch history and scrobbles to configured Plex and Jellyfin media servers with automatic writeback.".to_string(),
            status: self.media_sync_status.read().await.clone(),
            schedule: media_sync_sched_str,
            interval_hours: media_sync_interval,
            items_processed: self.media_sync_processed.load(Ordering::Relaxed),
            items_pending: 0,
            last_run_at: media_sync_last_run,
            last_success_at: media_sync_last_success,
            next_run_at: self.media_sync_next_run.read().await.clone(),
            last_message: self.media_sync_last_message.read().await.clone(),
        };

        vec![image_task, tvdb_task, user_sync_task, tautulli_sync_task, media_sync_task, db_backup_task, db_maintenance_task]
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
                state.task_tracker.set_image_running(Some(&state.pool), "Manual run triggered...").await;
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
                state.task_tracker.set_tvdb_running(Some(&state.pool), "Forced full metadata refresh triggered...").await;
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

                state.task_tracker.set_tvdb_running(Some(&state.pool), "Manual run triggered...").await;
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
                state.task_tracker.set_user_sync_running(Some(&state.pool), "Manual run triggered...").await;
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
        "db_backup" | "backup" => {
            state.task_tracker.set_db_backup_running(Some(&state.pool), "Manual backup triggered...").await;
            state.log_buffer.push("INFO", "bingesync::tasks", "Manual run triggered for Database Backup worker");

            match crate::api::backup::create_database_backup(&state, None).await {
                Ok(item) => {
                    state
                        .task_tracker
                        .set_db_backup_idle(Some(&state.pool), 1, &format!("Backup created: {}", item.filename))
                        .await;
                    Ok(Json(serde_json::json!({
                        "status": "triggered",
                        "task_id": "db_backup",
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
        "db_maintenance" | "maintenance" | "optimize" => {
            state.task_tracker.set_db_maintenance_running(Some(&state.pool), "Manual maintenance triggered...").await;
            state.log_buffer.push("INFO", "bingesync::tasks", "Manual run triggered for Database Maintenance worker");

            match crate::api::db_maintenance::run_database_maintenance(&state).await {
                Ok(msg) => {
                    state
                        .task_tracker
                        .set_db_maintenance_idle(Some(&state.pool), 1, &msg)
                        .await;
                    Ok(Json(serde_json::json!({
                        "status": "triggered",
                        "task_id": "db_maintenance",
                        "message": msg
                    })))
                }
                Err(e) => {
                    state
                        .task_tracker
                        .set_db_maintenance_idle(Some(&state.pool), 0, &format!("Maintenance failed: {}", e))
                        .await;
                    Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": e })),
                    ))
                }
            }
        }
        "tautulli_sync" | "tautulli" => {
            state.task_tracker.set_tautulli_sync_running(Some(&state.pool), "Manual sync triggered...").await;
            state.tautulli_sync_notify.notify_waiters();
            state.tautulli_sync_notify.notify_one();
            state.log_buffer.push("INFO", "bingesync::tasks", "Manual run triggered for Tautulli Sync worker");
            Ok(Json(serde_json::json!({
                "status": "triggered",
                "task_id": "tautulli_sync",
                "forced": false,
                "message": "Tautulli continuous sync worker has been signaled to run immediately"
            })))
        }
        "media_sync" | "media_writeback" | "writeback" => {
            state.task_tracker.set_media_sync_running(Some(&state.pool), "Manual sync triggered...").await;
            state.media_sync_notify.notify_waiters();
            state.media_sync_notify.notify_one();
            state.log_buffer.push("INFO", "bingesync::tasks", "Manual run triggered for Watch History Sync worker");
            Ok(Json(serde_json::json!({
                "status": "triggered",
                "task_id": "media_sync",
                "forced": false,
                "message": "Watch history sync worker has been signaled to run immediately"
            })))
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
    state.task_tracker.set_image_running(Some(&state.pool), "Forced full poster refresh triggered...").await;
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
    state.task_tracker.set_user_sync_running(Some(&state.pool), "Forced user cache refresh triggered...").await;
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
    state.task_tracker.set_tvdb_running(Some(&state.pool), "Forced full metadata refresh triggered...").await;
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
        "db_backup" | "backup" => {
            state.task_tracker.set_db_backup_next_run(Some(next_run.clone())).await;
        }
        "db_maintenance" | "maintenance" => {
            state.task_tracker.set_db_maintenance_next_run(Some(next_run.clone())).await;
        }
        "tautulli_sync" | "tautulli" => {
            state.task_tracker.set_tautulli_sync_next_run(Some(next_run.clone())).await;
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
