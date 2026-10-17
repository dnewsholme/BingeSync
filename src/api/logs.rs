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

    pub fn get_entries(&self, limit: usize, min_level: Option<&str>) -> Vec<LogEntry> {
        if let Ok(lock) = self.entries.read() {
            let iter = lock.iter().rev();

            let filtered: Vec<LogEntry> = match min_level.map(|s| s.to_uppercase()) {
                Some(ref lvl) if lvl == "ERROR" => iter.filter(|e| e.level == "ERROR").cloned().collect(),
                Some(ref lvl) if lvl == "WARN" => iter.filter(|e| e.level == "WARN" || e.level == "ERROR").cloned().collect(),
                Some(ref lvl) if lvl == "INFO" => iter.filter(|e| e.level != "DEBUG" && e.level != "TRACE").cloned().collect(),
                _ => iter.cloned().collect(),
            };

            filtered.into_iter().take(limit).collect()
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

        let mut visitor = LogMessageVisitor(String::new());
        event.record(&mut visitor);

        if !visitor.0.is_empty() {
            self.buffer.push(level, target, &visitor.0);
        }
    }
}

struct LogMessageVisitor(String);

impl tracing::field::Visit for LogMessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{:?}", value).trim_matches('"').to_string();
        } else if self.0.is_empty() {
            self.0 = format!("{}: {:?}", field.name(), value);
        } else {
            self.0.push_str(&format!(" {}={:?}", field.name(), value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.0 = value.to_string();
        } else if self.0.is_empty() {
            self.0 = format!("{}: {}", field.name(), value);
        } else {
            self.0.push_str(&format!(" {}={}", field.name(), value));
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct LogsQuery {
    pub limit: Option<usize>,
    pub level: Option<String>,
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

    let limit = query.limit.unwrap_or(200).clamp(1, 1000);
    let entries = state.log_buffer.get_entries(limit, query.level.as_deref());

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
