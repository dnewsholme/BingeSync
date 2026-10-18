use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
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
    #[serde(default, alias = "all_results")]
    pub all: Option<bool>,
    #[serde(default)]
    pub all_dates: Option<bool>,
}

pub async fn list_history(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<HistoryResponse>, (StatusCode, Json<serde_json::Value>)> {
    let all_results = query.all.unwrap_or(false);
    let all_dates = query.all_dates.unwrap_or(false);

    let page = query.page.unwrap_or(1).max(1);
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * limit;

    let media_type_filter = query.media_type.as_deref().unwrap_or("all");
    let search_filter = query.search.as_deref().unwrap_or("").trim();
    let date_filter = if all_dates {
        ""
    } else {
        query.date.as_deref().unwrap_or("").trim()
    };
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

    // Data query with ordering and optional pagination
    if all_results {
        data_sql.push_str(" ORDER BY watched_at DESC");
    } else {
        data_sql.push_str(" ORDER BY watched_at DESC LIMIT ? OFFSET ?");
    }

    let mut fetch_query = sqlx::query_as::<_, Scrobble>(&data_sql);
    for p in &bind_params {
        fetch_query = fetch_query.bind(p);
    }
    if !all_results {
        fetch_query = fetch_query.bind(limit).bind(offset);
    }

    let items = fetch_query
        .fetch_all(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Database fetch error: {}", e) })),
            )
        })?;

    let (final_page, total_pages, final_limit) = if all_results {
        (1, 1, total)
    } else if total == 0 {
        (1, 1, limit)
    } else {
        (page, (total + limit - 1) / limit, limit)
    };

    Ok(Json(HistoryResponse {
        items,
        total,
        page: final_page,
        limit: final_limit,
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
            genres, directors, actors, watch_status, progress_percent, view_offset_ms,
            watched_at, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
    .bind(payload.progress_percent)
    .bind(payload.view_offset_ms)
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
    state.metadata_notify.notify_one();

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
pub struct RateScrobbleRequest {
    pub rating: Option<i32>,
}

pub async fn rate_scrobble(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(payload): Json<RateScrobbleRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if let Some(r) = payload.rating {
        if !(1..=10).contains(&r) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Rating must be between 1 and 10" })),
            ));
        }
    }

    let res = sqlx::query("UPDATE scrobbles SET user_rating = ? WHERE id = ? AND user_id = ?")
        .bind(payload.rating)
        .bind(&id)
        .bind(&auth.user_id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Database error updating rating: {}", e) })),
            )
        })?;

    if res.rows_affected() == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Scrobble not found" })),
        ));
    }

    Ok(Json(serde_json::json!({
        "status": "success",
        "id": id,
        "rating": payload.rating
    })))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemWatchesResponse {
    pub current_id: String,
    pub total_watches: usize,
    pub other_watches: Vec<Scrobble>,
}

pub async fn get_item_watches(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<ItemWatchesResponse>, (StatusCode, Json<serde_json::Value>)> {
    // 1. Fetch current target scrobble for the authenticated user
    let current = sqlx::query_as::<_, Scrobble>(
        "SELECT * FROM scrobbles WHERE id = ? AND user_id = ?"
    )
    .bind(&id)
    .bind(&auth.user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Database error fetching scrobble: {}", e) })),
        )
    })?
    .ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Scrobble not found" })),
        )
    })?;

    // 2. Query other watches strictly matching this same item
    // For TV shows, this MUST be the exact same episode (same show, season, and episode)
    let other_watches = if current.media_type == "episode" {
        let gp = current.grandparent_title.as_deref().unwrap_or("").trim();
        if let (Some(s), Some(e)) = (current.season_number, current.episode_number) {
            if !gp.is_empty() {
                sqlx::query_as::<_, Scrobble>(
                    r#"
                    SELECT * FROM scrobbles
                    WHERE user_id = ?
                      AND id != ?
                      AND media_type = 'episode'
                      AND (
                        (TRIM(LOWER(COALESCE(grandparent_title, ''))) = TRIM(LOWER(?))
                         AND season_number = ?
                         AND episode_number = ?)
                        OR (? IS NOT NULL AND guid IS NOT NULL AND guid = ?)
                      )
                    ORDER BY watched_at DESC
                    "#
                )
                .bind(&auth.user_id)
                .bind(&current.id)
                .bind(gp)
                .bind(s)
                .bind(e)
                .bind(&current.guid)
                .bind(&current.guid)
                .fetch_all(&state.pool)
                .await
            } else {
                sqlx::query_as::<_, Scrobble>(
                    r#"
                    SELECT * FROM scrobbles
                    WHERE user_id = ?
                      AND id != ?
                      AND media_type = 'episode'
                      AND (
                        (TRIM(LOWER(title)) = TRIM(LOWER(?))
                         AND season_number = ?
                         AND episode_number = ?)
                        OR (? IS NOT NULL AND guid IS NOT NULL AND guid = ?)
                      )
                    ORDER BY watched_at DESC
                    "#
                )
                .bind(&auth.user_id)
                .bind(&current.id)
                .bind(&current.title)
                .bind(s)
                .bind(e)
                .bind(&current.guid)
                .bind(&current.guid)
                .fetch_all(&state.pool)
                .await
            }
        } else {
            // Season / episode numbers are missing, match by show title & episode title
            if !gp.is_empty() {
                sqlx::query_as::<_, Scrobble>(
                    r#"
                    SELECT * FROM scrobbles
                    WHERE user_id = ?
                      AND id != ?
                      AND media_type = 'episode'
                      AND (
                        (TRIM(LOWER(COALESCE(grandparent_title, ''))) = TRIM(LOWER(?))
                         AND TRIM(LOWER(title)) = TRIM(LOWER(?)))
                        OR (? IS NOT NULL AND guid IS NOT NULL AND guid = ?)
                      )
                    ORDER BY watched_at DESC
                    "#
                )
                .bind(&auth.user_id)
                .bind(&current.id)
                .bind(gp)
                .bind(&current.title)
                .bind(&current.guid)
                .bind(&current.guid)
                .fetch_all(&state.pool)
                .await
            } else {
                sqlx::query_as::<_, Scrobble>(
                    r#"
                    SELECT * FROM scrobbles
                    WHERE user_id = ?
                      AND id != ?
                      AND media_type = 'episode'
                      AND (
                        TRIM(LOWER(title)) = TRIM(LOWER(?))
                        OR (? IS NOT NULL AND guid IS NOT NULL AND guid = ?)
                      )
                    ORDER BY watched_at DESC
                    "#
                )
                .bind(&auth.user_id)
                .bind(&current.id)
                .bind(&current.title)
                .bind(&current.guid)
                .bind(&current.guid)
                .fetch_all(&state.pool)
                .await
            }
        }
    } else if current.media_type == "movie" {
        let title = current.title.trim();
        sqlx::query_as::<_, Scrobble>(
            r#"
            SELECT * FROM scrobbles
            WHERE user_id = ?
              AND id != ?
              AND media_type = 'movie'
              AND (
                (TRIM(LOWER(title)) = TRIM(LOWER(?))
                 AND (? IS NULL OR year IS NULL OR year = ?))
                OR (? IS NOT NULL AND guid IS NOT NULL AND guid = ?)
              )
            ORDER BY watched_at DESC
            "#
        )
        .bind(&auth.user_id)
        .bind(&current.id)
        .bind(title)
        .bind(current.year)
        .bind(current.year)
        .bind(&current.guid)
        .bind(&current.guid)
        .fetch_all(&state.pool)
        .await
    } else {
        let title = current.title.trim();
        sqlx::query_as::<_, Scrobble>(
            r#"
            SELECT * FROM scrobbles
            WHERE user_id = ?
              AND id != ?
              AND media_type = ?
              AND TRIM(LOWER(title)) = TRIM(LOWER(?))
            ORDER BY watched_at DESC
            "#
        )
        .bind(&auth.user_id)
        .bind(&current.id)
        .bind(&current.media_type)
        .bind(title)
        .fetch_all(&state.pool)
        .await
    }
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Database error fetching other watches: {}", e) })),
        )
    })?;

    let total_watches = other_watches.len() + 1;

    Ok(Json(ItemWatchesResponse {
        current_id: current.id,
        total_watches,
        other_watches,
    }))
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

/// Refresh metadata and poster artwork for a specific scrobble on user request
pub async fn refresh_scrobble_metadata(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let scrobble: Scrobble = match sqlx::query_as::<_, Scrobble>(
        "SELECT * FROM scrobbles WHERE id = ? AND (user_id = ? OR ? = 'admin')"
    )
    .bind(&id)
    .bind(&auth.user_id)
    .bind(&auth.role)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(Some(s)) => s,
        Ok(None) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Scrobble not found or unauthorized" })),
            ));
        }
        Err(e) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Database query error: {}", e) })),
            ));
        }
    };

    let is_episode = scrobble.media_type == "episode";
    let raw_search_title = if is_episode {
        scrobble.grandparent_title.as_deref().unwrap_or(&scrobble.title)
    } else {
        &scrobble.title
    };

    let (search_title, detected_year) = crate::api::tvdb::normalize_search_query(raw_search_title);
    let effective_year = scrobble.year.or(detected_year);

    // 1. Fetch metadata from TheTVDB if configured
    let tvdb_key = state.get_effective_tvdb_api_key().await;
    if let Some(ref key) = tvdb_key {
        if !key.trim().is_empty() {
            let base_url = state.get_effective_tvdb_base_url().await;
            let pin = state.get_effective_tvdb_pin().await;
            let client = crate::api::tvdb::TvdbClient::new(base_url, key.clone(), pin);

            if let Ok(Some(search_match)) = client.search(&search_title, if is_episode { "series" } else { "movie" }, effective_year).await {
                let match_is_series = search_match.entity_type.as_deref() == Some("series") || is_episode;
                let mut meta = if match_is_series {
                    match client.get_series_extended(&search_match.id).await {
                        Ok(m) => m,
                        Err(_) => client.get_movie_extended(&search_match.id).await.unwrap_or_default(),
                    }
                } else {
                    match client.get_movie_extended(&search_match.id).await {
                        Ok(m) => m,
                        Err(_) => client.get_series_extended(&search_match.id).await.unwrap_or_default(),
                    }
                };

                if meta.overview.is_none() || meta.overview.as_deref() == Some("") {
                    meta.overview = search_match.overview.clone();
                }
                if meta.image_url.is_none() || meta.image_url.as_deref() == Some("") {
                    meta.image_url = search_match.image_url.clone();
                }

                let now_str = Utc::now().to_rfc3339();
                if is_episode {
                    let show_name = scrobble.grandparent_title.as_deref().unwrap_or(&scrobble.title);
                    let _ = sqlx::query(
                        r#"
                        UPDATE scrobbles
                        SET genres = COALESCE(NULLIF(?, ''), NULLIF(genres, '')),
                            actors = COALESCE(NULLIF(?, ''), NULLIF(actors, '')),
                            directors = COALESCE(NULLIF(?, ''), NULLIF(directors, '')),
                            overview = COALESCE(NULLIF(?, ''), NULLIF(overview, '')),
                            thumb_url = CASE 
                                WHEN (thumb_url IS NULL OR thumb_url = '' OR thumb_url LIKE '/library/%' OR thumb_url LIKE '/photo/%') AND ? IS NOT NULL 
                                THEN ? 
                                ELSE thumb_url 
                            END,
                            metadata_updated_at = ?
                        WHERE id = ? OR (grandparent_title IS NOT NULL AND LOWER(TRIM(grandparent_title)) = LOWER(TRIM(?))) OR (media_type = 'episode' AND LOWER(TRIM(title)) = LOWER(TRIM(?)))
                        "#,
                    )
                    .bind(&meta.genres)
                    .bind(&meta.actors)
                    .bind(&meta.directors)
                    .bind(&meta.overview)
                    .bind(&meta.image_url)
                    .bind(&meta.image_url)
                    .bind(&now_str)
                    .bind(&scrobble.id)
                    .bind(show_name)
                    .bind(show_name)
                    .execute(&state.pool)
                    .await;
                } else {
                    let _ = sqlx::query(
                        r#"
                        UPDATE scrobbles
                        SET genres = COALESCE(NULLIF(?, ''), NULLIF(genres, '')),
                            actors = COALESCE(NULLIF(?, ''), NULLIF(actors, '')),
                            directors = COALESCE(NULLIF(?, ''), NULLIF(directors, '')),
                            overview = COALESCE(NULLIF(?, ''), NULLIF(overview, '')),
                            thumb_url = CASE 
                                WHEN (thumb_url IS NULL OR thumb_url = '' OR thumb_url LIKE '/library/%' OR thumb_url LIKE '/photo/%') AND ? IS NOT NULL 
                                THEN ? 
                                ELSE thumb_url 
                            END,
                            metadata_updated_at = ?
                        WHERE id = ?
                        "#,
                    )
                    .bind(&meta.genres)
                    .bind(&meta.actors)
                    .bind(&meta.directors)
                    .bind(&meta.overview)
                    .bind(&meta.image_url)
                    .bind(&meta.image_url)
                    .bind(&now_str)
                    .bind(&scrobble.id)
                    .execute(&state.pool)
                    .await;
                }

                if meta.image_url.is_some() {
                    state.image_notify.notify_waiters();
                }
            }
        }
    }

    // 2. Fetch/update poster if missing or Plex relative path
    let needs_poster = scrobble.thumb_url.as_deref()
        .map(|u| u.is_empty() || u.starts_with("/library/") || u.starts_with("/photo/"))
        .unwrap_or(true);

    if needs_poster {
        let _ = sqlx::query("UPDATE scrobbles SET thumb_url = NULL WHERE id = ? AND (thumb_url LIKE '/library/%' OR thumb_url LIKE '/photo/%')")
            .bind(&scrobble.id)
            .execute(&state.pool)
            .await;
        crate::api::images::process_missing_images_once(&state).await;
    }

    // 3. Mark metadata_updated_at
    let _ = sqlx::query("UPDATE scrobbles SET metadata_updated_at = datetime('now') WHERE id = ?")
        .bind(&scrobble.id)
        .execute(&state.pool)
        .await;

    // Fetch and return the updated scrobble
    let refreshed = sqlx::query_as::<_, Scrobble>("SELECT * FROM scrobbles WHERE id = ?")
        .bind(&scrobble.id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to retrieve refreshed scrobble: {}", e) })),
            )
        })?;

    Ok(Json(serde_json::json!({
        "status": "refreshed",
        "scrobble": refreshed,
        "message": "Metadata and artwork refreshed successfully"
    })))
}
