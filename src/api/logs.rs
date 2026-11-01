use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, RwLock,
    },
};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

use crate::{api::AppState, auth::AuthUser};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: u64,
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
}

#[derive(Debug)]
pub struct LogBuffer {
    max_entries: usize,
    counter: AtomicU64,
    entries: RwLock<VecDeque<LogEntry>>,
}

impl LogBuffer {
    pub fn new(max_entries: usize) -> Self {
        Self {
            max_entries,
            counter: AtomicU64::new(1),
            entries: RwLock::new(VecDeque::with_capacity(max_entries)),
        }
    }

    pub fn push(&self, level: &str, target: &str, message: &str) {
        let id = self.counter.fetch_add(1, Ordering::Relaxed);
        let entry = LogEntry {
            id,
            timestamp: chrono::Utc::now().to_rfc3339(),
            level: level.to_uppercase(),
            target: target.to_string(),
            message: message.to_string(),
        };

        if let Ok(mut lock) = self.entries.write() {
            if lock.len() >= self.max_entries {
                lock.pop_front();
            }
            lock.push_back(entry);
        }
    }

    pub fn get_entries(
        &self,
        limit: usize,
        min_level: Option<&str>,
        service: Option<&str>,
        search: Option<&str>,
        since: Option<&str>,
    ) -> Vec<LogEntry> {
        if let Ok(lock) = self.entries.read() {
            let iter = lock.iter().rev();

            let search_term = search
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty());

            let service_filter = service
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty() && s != "all");

            let since_cutoff: Option<chrono::DateTime<chrono::Utc>> = since.and_then(|s| {
                let now = chrono::Utc::now();
                match s {
                    "15m" => Some(now - chrono::Duration::minutes(15)),
                    "1h" => Some(now - chrono::Duration::hours(1)),
                    "6h" => Some(now - chrono::Duration::hours(6)),
                    "24h" => Some(now - chrono::Duration::hours(24)),
                    other => chrono::DateTime::parse_from_rfc3339(other)
                        .ok()
                        .map(|dt| dt.with_timezone(&chrono::Utc)),
                }
            });

            let mut filtered: Vec<LogEntry> = iter
                .filter(|e| {
                    if let Some(ref lvl) = min_level.map(|s| s.to_uppercase()) {
                        match lvl.as_str() {
                            "ERROR" => {
                                if e.level != "ERROR" {
                                    return false;
                                }
                            }
                            "WARN" => {
                                if e.level != "WARN" && e.level != "ERROR" {
                                    return false;
                                }
                            }
                            "INFO" => {
                                if e.level != "INFO" && e.level != "WARN" && e.level != "ERROR" {
                                    return false;
                                }
                            }
                            "DEBUG" => {
                                if e.level != "DEBUG" && e.level != "INFO" && e.level != "WARN" && e.level != "ERROR" {
                                    return false;
                                }
                            }
                            _ => {}
                        }
                    }

                    if let Some(cutoff) = since_cutoff {
                        if let Ok(entry_dt) = chrono::DateTime::parse_from_rfc3339(&e.timestamp) {
                            if entry_dt < cutoff {
                                return false;
                            }
                        }
                    }

                    if let Some(ref svc) = service_filter {
                        let target_lower = e.target.to_lowercase();
                        let msg_lower = e.message.to_lowercase();
                        let matches = match svc.as_str() {
                            "webhooks" | "webhook" | "plex" | "jellyfin" | "emby" => {
                                target_lower.contains("webhook")
                                    || msg_lower.contains("webhook")
                                    || target_lower.contains("plex")
                                    || msg_lower.contains("plex")
                                    || target_lower.contains("jellyfin")
                                    || msg_lower.contains("jellyfin")
                                    || target_lower.contains("emby")
                                    || msg_lower.contains("emby")
                            }
                            "auth" | "oidc" | "login" | "security" => {
                                target_lower.contains("auth")
                                    || target_lower.contains("oidc")
                                    || target_lower.contains("api_keys")
                                    || msg_lower.contains("login")
                                    || msg_lower.contains("jwt")
                                    || msg_lower.contains("token")
                                    || msg_lower.contains("backchannel")
                            }
                            "metadata" | "tvdb" | "tmdb" => {
                                target_lower.contains("tvdb")
                                    || target_lower.contains("tmdb")
                                    || target_lower.contains("metadata")
                                    || target_lower.contains("backfill")
                                    || msg_lower.contains("tvdb")
                                    || msg_lower.contains("tmdb")
                                    || msg_lower.contains("metadata")
                                    || msg_lower.contains("backfill")
                            }
                            "images" | "poster" => {
                                target_lower.contains("image")
                                    || target_lower.contains("poster")
                                    || target_lower.contains("thumb")
                                    || target_lower.contains("prune")
                                    || msg_lower.contains("image")
                                    || msg_lower.contains("poster")
                                    || msg_lower.contains("thumb")
                                    || msg_lower.contains("prune")
                                    || msg_lower.contains("orphaned")
                            }
                            "sync" | "writeback" | "media_sync" => {
                                target_lower.contains("writeback")
                                    || target_lower.contains("media_sync")
                                    || msg_lower.contains("writeback")
                                    || msg_lower.contains("played")
                            }
                            "tautulli" => {
                                target_lower.contains("tautulli")
                                    || msg_lower.contains("tautulli")
                            }
                            "integrations" | "radarr" | "sonarr" | "overseerr" => {
                                target_lower.contains("integration")
                                    || target_lower.contains("overseerr")
                                    || target_lower.contains("audit")
                                    || msg_lower.contains("radarr")
                                    || msg_lower.contains("sonarr")
                                    || msg_lower.contains("cleanup")
                                    || msg_lower.contains("overseerr")
                                    || msg_lower.contains("audit")
                            }
                            "maintenance" | "db" | "database" => {
                                target_lower.contains("maintenance")
                                    || target_lower.contains("db")
                                    || target_lower.contains("reaper")
                                    || target_lower.contains("duplicate")
                                    || msg_lower.contains("maintenance")
                                    || msg_lower.contains("vacuum")
                                    || msg_lower.contains("reindex")
                                    || msg_lower.contains("analyze")
                                    || msg_lower.contains("optimize")
                                    || msg_lower.contains("reaper")
                                    || msg_lower.contains("stale")
                                    || msg_lower.contains("duplicate")
                            }
                            "backup" | "backups" => {
                                target_lower.contains("backup")
                                    || msg_lower.contains("backup")
                                    || msg_lower.contains("restore")
                            }
                            "users" | "user_sync" => {
                                target_lower.contains("user_sync")
                                    || target_lower.contains("user")
                                    || msg_lower.contains("user_sync")
                                    || msg_lower.contains("user cache")
                            }
                            _ => target_lower.contains(svc) || msg_lower.contains(svc),
                        };

                        if !matches {
                            return false;
                        }
                    }

                    if let Some(ref q) = search_term {
                        let msg_lower = e.message.to_lowercase();
                        let target_lower = e.target.to_lowercase();

                        let matches = match q.as_str() {
                            "image_cache" | "images" => {
                                target_lower.contains("image")
                                    || msg_lower.contains("image")
                                    || msg_lower.contains("poster")
                            }
                            "tvdb_metadata" | "tvdb" | "metadata" => {
                                target_lower.contains("tvdb")
                                    || target_lower.contains("tmdb")
                                    || msg_lower.contains("tvdb")
                                    || msg_lower.contains("tmdb")
                                    || msg_lower.contains("metadata")
                            }
                            "user_sync" | "users" => {
                                target_lower.contains("user_sync")
                                    || target_lower.contains("user")
                                    || msg_lower.contains("user")
                                    || msg_lower.contains("sync")
                            }
                            "db_backup" | "backup" => {
                                target_lower.contains("backup") || msg_lower.contains("backup")
                            }
                            "db_maintenance" | "maintenance" | "optimize" => {
                                target_lower.contains("maintenance")
                                    || msg_lower.contains("maintenance")
                                    || msg_lower.contains("vacuum")
                                    || msg_lower.contains("reindex")
                                    || msg_lower.contains("analyze")
                                    || msg_lower.contains("optimize")
                            }
                            "tautulli_sync" | "tautulli" => {
                                target_lower.contains("tautulli")
                                    || msg_lower.contains("tautulli")
                            }
                            "media_sync" | "media_writeback" | "writeback" => {
                                target_lower.contains("writeback")
                                    || target_lower.contains("media_sync")
                                    || msg_lower.contains("writeback")
                                    || msg_lower.contains("plex")
                                    || msg_lower.contains("jellyfin")
                            }
                            "session_reaper" | "reaper" => {
                                target_lower.contains("reaper")
                                    || target_lower.contains("session")
                                    || msg_lower.contains("reaper")
                                    || msg_lower.contains("session")
                            }
                            "image_prune" | "prune_images" | "prune" => {
                                target_lower.contains("image_prune")
                                    || target_lower.contains("prune")
                                    || msg_lower.contains("prune")
                                    || msg_lower.contains("orphaned")
                            }
                            "duplicate_pruner" | "duplicates" | "dedupe" => {
                                target_lower.contains("duplicate")
                                    || msg_lower.contains("duplicate")
                                    || msg_lower.contains("pruner")
                            }
                            "metadata_backfill" | "backfill" => {
                                target_lower.contains("backfill")
                                    || target_lower.contains("metadata")
                                    || msg_lower.contains("backfill")
                                    || msg_lower.contains("enrichment")
                            }
                            "version_check" | "version" | "updates" => {
                                target_lower.contains("version")
                                    || msg_lower.contains("version")
                                    || msg_lower.contains("github")
                                    || msg_lower.contains("release")
                            }
                            "library_audit" | "audit" | "arr_audit" => {
                                target_lower.contains("audit")
                                    || target_lower.contains("library")
                                    || msg_lower.contains("audit")
                                    || msg_lower.contains("arr")
                            }
                            _ => msg_lower.contains(q) || target_lower.contains(q),
                        };

                        if !matches {
                            return false;
                        }
                    }

                    true
                })
                .take(limit)
                .cloned()
                .collect();

            // Return in top-to-bottom chronological order (oldest at top, newest at bottom)
            filtered.reverse();
            filtered
        } else {
            Vec::new()
        }
    }

    pub fn clear(&self) {
        if let Ok(mut lock) = self.entries.write() {
            lock.clear();
        }
    }
}

pub struct TracingLogLayer {
    buffer: Arc<LogBuffer>,
}

impl TracingLogLayer {
    pub fn new(buffer: Arc<LogBuffer>) -> Self {
        Self { buffer }
    }
}

impl<S: tracing::Subscriber> Layer<S> for TracingLogLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let level = meta.level().as_str();
        let target = meta.target();

        let mut visitor = LogMessageVisitor::new();
        event.record(&mut visitor);

        let msg = visitor.finish();
        if !msg.is_empty() {
            self.buffer.push(level, target, &msg);
        }
    }
}

struct LogMessageVisitor {
    message: String,
    fields: Vec<(String, String)>,
}

impl LogMessageVisitor {
    fn new() -> Self {
        Self {
            message: String::new(),
            fields: Vec::new(),
        }
    }

    fn finish(self) -> String {
        if self.message.is_empty() && self.fields.is_empty() {
            return String::new();
        }
        if self.fields.is_empty() {
            return self.message;
        }

        let fields_str = self
            .fields
            .into_iter()
            .map(|(k, v)| {
                let clean_v = v.trim();
                if clean_v.contains(' ') || clean_v.contains('"') || clean_v.is_empty() {
                    let escaped = clean_v.replace('"', "\\\"");
                    format!("{}=\"{}\"", k, escaped)
                } else {
                    format!("{}={}", k, clean_v)
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        if self.message.is_empty() {
            format!("[{}]", fields_str)
        } else {
            format!("{} [{}]", self.message, fields_str)
        }
    }
}

impl tracing::field::Visit for LogMessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value).trim_matches('"').to_string();
        } else {
            self.fields.push((
                field.name().to_string(),
                format!("{:?}", value).trim_matches('"').to_string(),
            ));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields
                .push((field.name().to_string(), value.to_string()));
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct LogsQuery {
    pub limit: Option<usize>,
    pub level: Option<String>,
    pub service: Option<String>,
    pub search: Option<String>,
    pub since: Option<String>,
}

pub async fn get_admin_logs_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<LogsQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if auth.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Admin privileges required to view application logs" })),
        ));
    }

    let limit = query.limit.unwrap_or(250).clamp(1, 2000);
    let entries = state.log_buffer.get_entries(
        limit,
        query.level.as_deref(),
        query.service.as_deref(),
        query.search.as_deref(),
        query.since.as_deref(),
    );

    Ok(Json(serde_json::json!({
        "total": entries.len(),
        "entries": entries,
    })))
}

pub async fn clear_admin_logs_handler(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if auth.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Admin privileges required to clear application logs" })),
        ));
    }

    state.log_buffer.clear();
    state.log_buffer.push("INFO", "bingesync::logs", "Application logs cleared by administrator");

    Ok(Json(serde_json::json!({
        "status": "cleared"
    })))
}
