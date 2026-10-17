use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use std::collections::BTreeMap;

use crate::{
    api::AppState,
    auth::AuthUser,
    models::{Scrobble, TimelineDay},
};

#[derive(Debug, Deserialize)]
pub struct TimelineQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub date: Option<String>,
    pub genre: Option<String>,
    pub person: Option<String>,
    pub rating: Option<String>,
    pub watch_status: Option<String>,
    pub media_type: Option<String>,
    pub search: Option<String>,
}

pub async fn get_timeline(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<TimelineQuery>,
) -> Result<Json<Vec<TimelineDay>>, (StatusCode, Json<serde_json::Value>)> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0).max(0);

    let media_type_filter = query.media_type.as_deref().unwrap_or("all");
    let search_filter = query.search.as_deref().unwrap_or("").trim();
    let date_filter = query.date.as_deref().unwrap_or("").trim();
    let genre_filter = query.genre.as_deref().unwrap_or("").trim();
    let person_filter = query.person.as_deref().unwrap_or("").trim();
    let rating_filter = query.rating.as_deref().unwrap_or("all").trim();
    let watch_status_filter = query.watch_status.as_deref().unwrap_or("all").trim();

    let mut sql = "SELECT * FROM scrobbles WHERE user_id = ?".to_string();
    let mut bind_params: Vec<String> = vec![auth.user_id.clone()];

    if media_type_filter != "all" && !media_type_filter.is_empty() {
        sql.push_str(" AND media_type = ?");
        bind_params.push(media_type_filter.to_string());
    }

    if !search_filter.is_empty() {
        let pattern = format!("%{}%", search_filter);
        sql.push_str(" AND (title LIKE ? OR grandparent_title LIKE ? OR overview LIKE ?)");
        bind_params.push(pattern.clone());
        bind_params.push(pattern.clone());
        bind_params.push(pattern);
    }

    if !date_filter.is_empty() {
        let pattern = format!("{}%", date_filter);
        sql.push_str(" AND (watched_at LIKE ? OR substr(watched_at, 1, 10) = ?)");
        bind_params.push(pattern);
        bind_params.push(date_filter.to_string());
    }

    if !genre_filter.is_empty() {
        let pattern = format!("%{}%", genre_filter);
        sql.push_str(" AND genres LIKE ?");
        bind_params.push(pattern);
    }

    if !person_filter.is_empty() {
        let pattern = format!("%{}%", person_filter);
        sql.push_str(" AND (directors LIKE ? OR actors LIKE ?)");
        bind_params.push(pattern.clone());
        bind_params.push(pattern);
    }

    match rating_filter {
        "rated" => {
            sql.push_str(" AND user_rating IS NOT NULL");
        }
        "unrated" => {
            sql.push_str(" AND user_rating IS NULL");
        }
        "10" => {
            sql.push_str(" AND user_rating = 10");
        }
        "9+" | "9" => {
            sql.push_str(" AND user_rating >= 9");
        }
        "8+" | "8" => {
            sql.push_str(" AND user_rating >= 8");
        }
        "7+" | "7" => {
            sql.push_str(" AND user_rating >= 7");
        }
        "6+" | "6" => {
            sql.push_str(" AND user_rating >= 6");
        }
        "5+" | "5" => {
            sql.push_str(" AND user_rating >= 5");
        }
        _ => {}
    }

    if watch_status_filter != "all" && !watch_status_filter.is_empty() {
        sql.push_str(" AND watch_status = ?");
        bind_params.push(watch_status_filter.to_string());
    }

    sql.push_str(" ORDER BY watched_at DESC LIMIT ? OFFSET ?");

    let mut fetch_query = sqlx::query_as::<_, Scrobble>(&sql);
    for p in &bind_params {
        fetch_query = fetch_query.bind(p);
    }
    fetch_query = fetch_query.bind(limit).bind(offset);

    let scrobbles = fetch_query
        .fetch_all(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Database error fetching timeline: {}", e) })),
            )
        })?;

    // Group items by day YYYY-MM-DD preserving descending order
    let mut day_map: BTreeMap<String, Vec<Scrobble>> = BTreeMap::new();
    let mut day_order: Vec<String> = Vec::new();

    for scrobble in scrobbles {
        // Parse date prefix from watched_at (handles ISO8601 or SQLite format)
        let day_str = scrobble
            .watched_at
            .split('T')
            .next()
            .unwrap_or(&scrobble.watched_at)
            .split(' ')
            .next()
            .unwrap_or("Unknown Date")
            .to_string();

        if !day_map.contains_key(&day_str) {
            day_order.push(day_str.clone());
        }
        day_map.entry(day_str).or_default().push(scrobble);
    }

    let mut result: Vec<TimelineDay> = Vec::new();
    for day in day_order {
        if let Some(items) = day_map.remove(&day) {
            let total_count = items.len();
            let total_minutes = items
                .iter()
                .filter_map(|s| s.duration_ms)
                .map(|ms| ms / 60_000)
                .sum();

            result.push(TimelineDay {
                date: day,
                total_count,
                total_minutes,
                items,
            });
        }
    }

    Ok(Json(result))
}
