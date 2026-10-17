use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    api::AppState,
    auth::AuthUser,
    models::{HistoryResponse, NewScrobbleRequest, Scrobble},
};

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub page: Option<i64>,
    pub limit: Option<i64>,
    pub media_type: Option<String>,
    pub search: Option<String>,
    pub date: Option<String>,
    pub genre: Option<String>,
    pub person: Option<String>,
    pub rating: Option<String>,
    pub watch_status: Option<String>,
}

pub async fn list_history(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<HistoryResponse>, (StatusCode, Json<serde_json::Value>)> {
    let page = query.page.unwrap_or(1).max(1);
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * limit;

    let media_type_filter = query.media_type.as_deref().unwrap_or("all");
    let search_filter = query.search.as_deref().unwrap_or("").trim();
    let date_filter = query.date.as_deref().unwrap_or("").trim();
    let genre_filter = query.genre.as_deref().unwrap_or("").trim();
    let person_filter = query.person.as_deref().unwrap_or("").trim();
    let rating_filter = query.rating.as_deref().unwrap_or("all").trim();
    let watch_status_filter = query.watch_status.as_deref().unwrap_or("all").trim();

    let mut count_sql = "SELECT COUNT(*) FROM scrobbles WHERE user_id = ?".to_string();
    let mut data_sql = "SELECT * FROM scrobbles WHERE user_id = ?".to_string();

    let mut bind_params: Vec<String> = vec![auth.user_id.clone()];

    if media_type_filter != "all" && !media_type_filter.is_empty() {
        count_sql.push_str(" AND media_type = ?");
        data_sql.push_str(" AND media_type = ?");
        bind_params.push(media_type_filter.to_string());
    }

    if !search_filter.is_empty() {
        let pattern = format!("%{}%", search_filter);
        count_sql.push_str(" AND (title LIKE ? OR grandparent_title LIKE ? OR overview LIKE ?)");
        data_sql.push_str(" AND (title LIKE ? OR grandparent_title LIKE ? OR overview LIKE ?)");
        bind_params.push(pattern.clone());
        bind_params.push(pattern.clone());
        bind_params.push(pattern);
    }

    if !date_filter.is_empty() {
        let pattern = format!("{}%", date_filter);
        count_sql.push_str(" AND (watched_at LIKE ? OR substr(watched_at, 1, 10) = ?)");
        data_sql.push_str(" AND (watched_at LIKE ? OR substr(watched_at, 1, 10) = ?)");
        bind_params.push(pattern);
        bind_params.push(date_filter.to_string());
    }

    if !genre_filter.is_empty() {
        let pattern = format!("%{}%", genre_filter);
        count_sql.push_str(" AND genres LIKE ?");
        data_sql.push_str(" AND genres LIKE ?");
        bind_params.push(pattern);
    }

    if !person_filter.is_empty() {
        let pattern = format!("%{}%", person_filter);
        count_sql.push_str(" AND (directors LIKE ? OR actors LIKE ?)");
        data_sql.push_str(" AND (directors LIKE ? OR actors LIKE ?)");
        bind_params.push(pattern.clone());
        bind_params.push(pattern);
    }

    match rating_filter {
        "rated" => {
            count_sql.push_str(" AND user_rating IS NOT NULL");
            data_sql.push_str(" AND user_rating IS NOT NULL");
        }
        "unrated" => {
            count_sql.push_str(" AND user_rating IS NULL");
            data_sql.push_str(" AND user_rating IS NULL");
        }
        "10" => {
            count_sql.push_str(" AND user_rating = 10");
            data_sql.push_str(" AND user_rating = 10");
        }
        "9+" | "9" => {
            count_sql.push_str(" AND user_rating >= 9");
            data_sql.push_str(" AND user_rating >= 9");
        }
        "8+" | "8" => {
            count_sql.push_str(" AND user_rating >= 8");
            data_sql.push_str(" AND user_rating >= 8");
        }
        "7+" | "7" => {
            count_sql.push_str(" AND user_rating >= 7");
            data_sql.push_str(" AND user_rating >= 7");
        }
        "6+" | "6" => {
            count_sql.push_str(" AND user_rating >= 6");
            data_sql.push_str(" AND user_rating >= 6");
        }
        "5+" | "5" => {
            count_sql.push_str(" AND user_rating >= 5");
            data_sql.push_str(" AND user_rating >= 5");
        }
        _ => {}
    }

    if watch_status_filter != "all" && !watch_status_filter.is_empty() {
        count_sql.push_str(" AND watch_status = ?");
        data_sql.push_str(" AND watch_status = ?");
        bind_params.push(watch_status_filter.to_string());
    }

    // Count query
    let mut count_query = sqlx::query_scalar::<_, i64>(&count_sql);
    for p in &bind_params {
        count_query = count_query.bind(p);
    }
    let total = count_query
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Database count error: {}", e) })),
            )
        })?;

    // Data query with ordering and pagination
    data_sql.push_str(" ORDER BY watched_at DESC LIMIT ? OFFSET ?");

    let mut fetch_query = sqlx::query_as::<_, Scrobble>(&data_sql);
    for p in &bind_params {
        fetch_query = fetch_query.bind(p);
    }
    fetch_query = fetch_query.bind(limit).bind(offset);

    let items = fetch_query
        .fetch_all(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Database fetch error: {}", e) })),
            )
        })?;

    let total_pages = if total == 0 {
        1
    } else {
        (total + limit - 1) / limit
    };

    Ok(Json(HistoryResponse {
        items,
        total,
        page,
        limit,
        total_pages,
    }))
}

pub async fn create_scrobble(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(payload): Json<NewScrobbleRequest>,
) -> Result<(StatusCode, Json<Scrobble>), (StatusCode, Json<serde_json::Value>)> {
    let scrobble_id = Uuid::new_v4().to_string();
    let watched_at = payload.watched_at.unwrap_or_else(|| Utc::now().to_rfc3339());
    let now = Utc::now().to_rfc3339();
    let watch_status = payload.watch_status.unwrap_or_else(|| "completed".to_string());

    sqlx::query(
        r#"
        INSERT INTO scrobbles (
            id, user_id, title, media_type, grandparent_title, parent_title,
            season_number, episode_number, year, overview, thumb_url,
            rating_key, guid, duration_ms, user_rating,
            genres, directors, actors, watch_status,
            watched_at, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&scrobble_id)
    .bind(&auth.user_id)
    .bind(&payload.title)
    .bind(&payload.media_type)
    .bind(&payload.grandparent_title)
    .bind(&payload.parent_title)
    .bind(payload.season_number)
    .bind(payload.episode_number)
    .bind(payload.year)
    .bind(&payload.overview)
    .bind(&payload.thumb_url)
    .bind(None::<String>)
    .bind(None::<String>)
    .bind(payload.duration_ms)
    .bind(payload.user_rating)
    .bind(&payload.genres)
    .bind(&payload.directors)
    .bind(&payload.actors)
    .bind(&watch_status)
    .bind(&watched_at)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Database error inserting scrobble: {}", e) })),
        )
    })?;

    let scrobble: Scrobble = sqlx::query_as::<_, Scrobble>(
        "SELECT * FROM scrobbles WHERE id = ?"
    )
    .bind(&scrobble_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Database error fetching created scrobble: {}", e) })),
        )
    })?;

    if scrobble.thumb_url.is_none() {
        state.image_notify.notify_one();
    }

    Ok((StatusCode::CREATED, Json(scrobble)))
}

pub async fn delete_scrobble(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let res = sqlx::query("DELETE FROM scrobbles WHERE id = ? AND user_id = ?")
        .bind(&id)
        .bind(&auth.user_id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Database error deleting scrobble: {}", e) })),
            )
        })?;

    if res.rows_affected() == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Scrobble not found" })),
        ));
    }

    Ok(Json(serde_json::json!({ "status": "deleted", "id": id })))
}

#[derive(Debug, Deserialize)]
pub struct DeleteShowQuery {
    pub show: String,
}

#[derive(Debug, Deserialize)]
pub struct DeleteDateQuery {
    pub date: String,
}

#[derive(Debug, Deserialize)]
pub struct DeleteBatchRequest {
    pub ids: Vec<String>,
}

pub async fn delete_by_show(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<DeleteShowQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let res = sqlx::query("DELETE FROM scrobbles WHERE user_id = ? AND grandparent_title = ?")
        .bind(&auth.user_id)
        .bind(&query.show)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Database error deleting show scrobbles: {}", e) })),
            )
        })?;

    Ok(Json(serde_json::json!({
        "status": "deleted",
        "show": query.show,
        "deleted_count": res.rows_affected()
    })))
}

pub async fn delete_batch(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(payload): Json<DeleteBatchRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if payload.ids.is_empty() {
        return Ok(Json(serde_json::json!({ "status": "deleted", "deleted_count": 0 })));
    }

    let mut deleted = 0;
    for id in &payload.ids {
        if let Ok(res) = sqlx::query("DELETE FROM scrobbles WHERE id = ? AND user_id = ?")
            .bind(id)
            .bind(&auth.user_id)
            .execute(&state.pool)
            .await
        {
            deleted += res.rows_affected();
        }
    }

    Ok(Json(serde_json::json!({
        "status": "deleted",
        "deleted_count": deleted
    })))
}

pub async fn delete_by_date(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<DeleteDateQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let date_prefix = query.date.trim();
    let pattern = format!("{}%", date_prefix);

    let res = sqlx::query("DELETE FROM scrobbles WHERE user_id = ? AND (substr(watched_at, 1, 10) = ? OR watched_at LIKE ?)")
        .bind(&auth.user_id)
        .bind(date_prefix)
        .bind(&pattern)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Database error deleting by date: {}", e) })),
            )
        })?;

    Ok(Json(serde_json::json!({
        "status": "deleted",
        "date": query.date,
        "deleted_count": res.rows_affected()
    })))
}
