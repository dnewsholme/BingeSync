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
    models::{
        CleanDuplicatesRequest, CleanDuplicatesResponse, DuplicateGroup, DuplicateScanResponse,
        HistoryResponse, NewScrobbleRequest, Scrobble,
    },
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
    pub director: Option<String>,
    pub cinematographer: Option<String>,
    pub rating: Option<String>,
    pub watch_status: Option<String>,
    pub tag: Option<String>,
    #[serde(default, alias = "all_results")]
    pub all: Option<bool>,
    #[serde(default)]
    pub all_dates: Option<bool>,
    #[serde(default)]
    pub missing_metadata: Option<bool>,
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
    let is_missing_metadata_keyword = search_filter.eq_ignore_ascii_case("missing:metadata")
        || search_filter.eq_ignore_ascii_case("missing_metadata")
        || search_filter.eq_ignore_ascii_case("missing");
    let missing_metadata_filter = query.missing_metadata.unwrap_or(false) || is_missing_metadata_keyword;

    let date_filter = if all_dates {
        ""
    } else {
        query.date.as_deref().unwrap_or("").trim()
    };
    let genre_filter = query.genre.as_deref().unwrap_or("").trim();
    let person_filter = query.person.as_deref().unwrap_or("").trim();
    let director_filter = query.director.as_deref().unwrap_or("").trim();
    let cinematographer_filter = query.cinematographer.as_deref().unwrap_or("").trim();
    let rating_filter = query.rating.as_deref().unwrap_or("all").trim();
    let watch_status_filter = query.watch_status.as_deref().unwrap_or("all").trim();
    let tag_filter = query.tag.as_deref().unwrap_or("").trim();

    let mut count_sql = "SELECT COUNT(*) FROM scrobbles WHERE user_id = ?".to_string();
    let mut data_sql = "SELECT * FROM scrobbles WHERE user_id = ?".to_string();

    let mut bind_params: Vec<String> = vec![auth.user_id.clone()];

    if media_type_filter != "all" && !media_type_filter.is_empty() {
        count_sql.push_str(" AND media_type = ?");
        data_sql.push_str(" AND media_type = ?");
        bind_params.push(media_type_filter.to_string());
    }

    if missing_metadata_filter {
        count_sql.push_str(" AND (thumb_url IS NULL OR thumb_url = '' OR overview IS NULL OR overview = '' OR genres IS NULL OR genres = '')");
        data_sql.push_str(" AND (thumb_url IS NULL OR thumb_url = '' OR overview IS NULL OR overview = '' OR genres IS NULL OR genres = '')");
    }

    if !search_filter.is_empty() && !is_missing_metadata_keyword {
        let pattern = format!("%{}%", search_filter);
        count_sql.push_str(" AND (title LIKE ? OR grandparent_title LIKE ? OR overview LIKE ? OR tags LIKE ? OR notes LIKE ?)");
        data_sql.push_str(" AND (title LIKE ? OR grandparent_title LIKE ? OR overview LIKE ? OR tags LIKE ? OR notes LIKE ?)");
        bind_params.push(pattern.clone());
        bind_params.push(pattern.clone());
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

    if !director_filter.is_empty() {
        let pattern = format!("%{}%", director_filter);
        count_sql.push_str(" AND (director LIKE ? OR directors LIKE ?)");
        data_sql.push_str(" AND (director LIKE ? OR directors LIKE ?)");
        bind_params.push(pattern.clone());
        bind_params.push(pattern);
    }

    if !cinematographer_filter.is_empty() {
        let pattern = format!("%{}%", cinematographer_filter);
        count_sql.push_str(" AND cinematographer LIKE ?");
        data_sql.push_str(" AND cinematographer LIKE ?");
        bind_params.push(pattern);
    }

    if !person_filter.is_empty() {
        let pattern = format!("%{}%", person_filter);
        count_sql.push_str(" AND (director LIKE ? OR directors LIKE ? OR cinematographer LIKE ? OR actors LIKE ?)");
        data_sql.push_str(" AND (director LIKE ? OR directors LIKE ? OR cinematographer LIKE ? OR actors LIKE ?)");
        bind_params.push(pattern.clone());
        bind_params.push(pattern.clone());
        bind_params.push(pattern.clone());
        bind_params.push(pattern);
    }

    if !tag_filter.is_empty() {
        let pattern = format!("%{}%", tag_filter);
        count_sql.push_str(" AND tags LIKE ?");
        data_sql.push_str(" AND tags LIKE ?");
        bind_params.push(pattern);
    }

    if rating_filter == "rated" {
        count_sql.push_str(" AND user_rating IS NOT NULL");
        data_sql.push_str(" AND user_rating IS NOT NULL");
    } else if rating_filter == "unrated" {
        count_sql.push_str(" AND user_rating IS NULL");
        data_sql.push_str(" AND user_rating IS NULL");
    } else if let Some(thresh) = rating_filter
        .strip_suffix('+')
        .and_then(|s| s.parse::<i32>().ok())
        .or_else(|| rating_filter.parse::<i32>().ok())
    {
        if (1..=10).contains(&thresh) {
            let clause = format!(" AND user_rating >= {}", thresh);
            count_sql.push_str(&clause);
            data_sql.push_str(&clause);
        }
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
    let director = payload.director.clone().or_else(|| payload.directors.clone());
    let directors = payload.directors.or_else(|| director.clone());
    let cinematographer = payload.cinematographer;
    let tags = payload.tags;
    let notes = payload.notes;

    sqlx::query(
        r#"
        INSERT INTO scrobbles (
            id, user_id, title, media_type, grandparent_title, parent_title,
            season_number, episode_number, year, overview, thumb_url,
            rating_key, guid, duration_ms, user_rating,
            genres, directors, director, cinematographer, actors, watch_status, progress_percent, view_offset_ms,
            tags, notes, watched_at, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
    .bind(&directors)
    .bind(&director)
    .bind(&cinematographer)
    .bind(&payload.actors)
    .bind(&watch_status)
    .bind(payload.progress_percent)
    .bind(payload.view_offset_ms)
    .bind(&tags)
    .bind(&notes)
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

#[derive(Debug, Deserialize)]
pub struct UpdateUserDataRequest {
    pub tags: Option<String>,
    pub notes: Option<String>,
}

pub async fn update_scrobble_user_data(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(payload): Json<UpdateUserDataRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let clean_tags = payload.tags.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()).map(|s| s.to_string());
    let clean_notes = payload.notes.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()).map(|s| s.to_string());

    let res = sqlx::query("UPDATE scrobbles SET tags = ?, notes = ? WHERE id = ? AND user_id = ?")
        .bind(&clean_tags)
        .bind(&clean_notes)
        .bind(&id)
        .bind(&auth.user_id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Database error updating user data: {}", e) })),
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
        "tags": clean_tags,
        "notes": clean_notes,
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
pub struct DeleteMediaHistoryQuery {
    pub media_type: String,
    pub title: String,
    pub year: Option<i32>,
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
    let res = sqlx::query("DELETE FROM scrobbles WHERE user_id = ? AND (grandparent_title = ? OR (media_type = 'episode' AND title = ?))")
        .bind(&auth.user_id)
        .bind(&query.show)
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

pub async fn delete_by_media(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<DeleteMediaHistoryQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let deleted_count = if query.media_type.eq_ignore_ascii_case("movie") {
        let mut sql = "DELETE FROM scrobbles WHERE user_id = ? AND media_type = 'movie' AND LOWER(TRIM(title)) = LOWER(TRIM(?))".to_string();
        if query.year.is_some() {
            sql.push_str(" AND (year = ? OR year IS NULL)");
            let res = sqlx::query(&sql)
                .bind(&auth.user_id)
                .bind(&query.title)
                .bind(query.year.unwrap())
                .execute(&state.pool)
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": format!("Database error deleting movie scrobbles: {}", e) })),
                    )
                })?;
            res.rows_affected()
        } else {
            let res = sqlx::query(&sql)
                .bind(&auth.user_id)
                .bind(&query.title)
                .execute(&state.pool)
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": format!("Database error deleting movie scrobbles: {}", e) })),
                    )
                })?;
            res.rows_affected()
        }
    } else {
        let res = sqlx::query(
            r#"
            DELETE FROM scrobbles
            WHERE user_id = ?
              AND media_type = 'episode'
              AND (
                  LOWER(TRIM(COALESCE(grandparent_title, title))) = LOWER(TRIM(?))
                  OR grandparent_title = ?
                  OR title = ?
              )
            "#,
        )
        .bind(&auth.user_id)
        .bind(&query.title)
        .bind(&query.title)
        .bind(&query.title)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Database error deleting series scrobbles: {}", e) })),
            )
        })?;
        res.rows_affected()
    };

    Ok(Json(serde_json::json!({
        "status": "deleted",
        "media_type": query.media_type,
        "title": query.title,
        "deleted_count": deleted_count
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

#[derive(Debug, Deserialize)]
pub struct DuplicatesQuery {
    pub window_hours: Option<i64>,
}

fn parse_timestamp_epoch(raw: &str) -> Option<i64> {
    let trimmed = raw.trim();
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(trimmed) {
        return Some(dt.timestamp());
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S") {
        return Some(chrono::DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc).timestamp());
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S") {
        return Some(chrono::DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc).timestamp());
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        if let Some(naive) = date.and_hms_opt(12, 0, 0) {
            return Some(chrono::DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc).timestamp());
        }
    }
    None
}

fn format_time_difference(seconds: i64) -> String {
    if seconds < 60 {
        "Recorded seconds apart".to_string()
    } else if seconds < 3600 {
        let m = seconds / 60;
        format!("Recorded {}m apart", m)
    } else if seconds < 86400 {
        let h = seconds / 3600;
        let m = (seconds % 3600) / 60;
        if m > 0 {
            format!("Recorded {}h {}m apart", h, m)
        } else {
            format!("Recorded {}h apart", h)
        }
    } else {
        let d = seconds / 86400;
        let h = (seconds % 86400) / 3600;
        if h > 0 {
            format!("Recorded {}d {}h apart", d, h)
        } else {
            format!("Recorded {}d apart", d)
        }
    }
}

pub fn find_duplicate_groups(scrobbles: &[Scrobble], window_hours: i64) -> Vec<DuplicateGroup> {
    use std::collections::BTreeMap;

    let window_hours = window_hours.clamp(1, 720);
    let window_seconds = window_hours * 3600;

    // Group scrobbles by unique media item key
    let mut media_buckets: BTreeMap<String, Vec<Scrobble>> = BTreeMap::new();
    for s in scrobbles {
        let key = if s.media_type == "episode" {
            let show = s.grandparent_title.as_deref().unwrap_or(&s.title).trim().to_lowercase();
            format!("episode:{}:{}:{}", show, s.season_number.unwrap_or(0), s.episode_number.unwrap_or(0))
        } else {
            format!("movie:{}:{}", s.title.trim().to_lowercase(), s.year.unwrap_or(0))
        };
        media_buckets.entry(key).or_default().push(s.clone());
    }

    let mut result_groups: Vec<DuplicateGroup> = Vec::new();

    for (group_key, mut items) in media_buckets {
        // Sort items chronologically by watched_at, with created_at as fallback
        items.sort_by(|a, b| {
            let t_a = parse_timestamp_epoch(&a.watched_at).unwrap_or(0);
            let t_b = parse_timestamp_epoch(&b.watched_at).unwrap_or(0);
            t_a.cmp(&t_b).then_with(|| a.created_at.cmp(&b.created_at))
        });

        // Cluster items within window_seconds
        let mut current_cluster: Vec<Scrobble> = Vec::new();

        for item in items {
            if let Some(last) = current_cluster.last() {
                let t_last = parse_timestamp_epoch(&last.watched_at).unwrap_or(0);
                let t_curr = parse_timestamp_epoch(&item.watched_at).unwrap_or(0);
                let diff = (t_curr - t_last).abs();

                if diff <= window_seconds {
                    current_cluster.push(item);
                } else {
                    if current_cluster.len() > 1 {
                        let rep = &current_cluster[0];
                        let t_first = parse_timestamp_epoch(&current_cluster.first().unwrap().watched_at).unwrap_or(0);
                        let t_end = parse_timestamp_epoch(&current_cluster.last().unwrap().watched_at).unwrap_or(0);
                        let total_diff = (t_end - t_first).abs();
                        let time_diff_disp = format_time_difference(total_diff);

                        result_groups.push(DuplicateGroup {
                            group_key: group_key.clone(),
                            media_type: rep.media_type.clone(),
                            title: rep.title.clone(),
                            grandparent_title: rep.grandparent_title.clone(),
                            season_number: rep.season_number,
                            episode_number: rep.episode_number,
                            year: rep.year,
                            thumb_url: rep.thumb_url.clone(),
                            time_difference_display: time_diff_disp,
                            scrobbles: current_cluster,
                        });
                    }
                    current_cluster = vec![item];
                }
            } else {
                current_cluster.push(item);
            }
        }

        if current_cluster.len() > 1 {
            let rep = &current_cluster[0];
            let t_first = parse_timestamp_epoch(&current_cluster.first().unwrap().watched_at).unwrap_or(0);
            let t_end = parse_timestamp_epoch(&current_cluster.last().unwrap().watched_at).unwrap_or(0);
            let total_diff = (t_end - t_first).abs();
            let time_diff_disp = format_time_difference(total_diff);

            result_groups.push(DuplicateGroup {
                group_key: group_key.clone(),
                media_type: rep.media_type.clone(),
                title: rep.title.clone(),
                grandparent_title: rep.grandparent_title.clone(),
                season_number: rep.season_number,
                episode_number: rep.episode_number,
                year: rep.year,
                thumb_url: rep.thumb_url.clone(),
                time_difference_display: time_diff_disp,
                scrobbles: current_cluster,
            });
        }
    }

    // Sort result groups so most recent duplicate clusters appear first
    result_groups.sort_by(|a, b| {
        let t_a = a.scrobbles.last().and_then(|s| parse_timestamp_epoch(&s.watched_at)).unwrap_or(0);
        let t_b = b.scrobbles.last().and_then(|s| parse_timestamp_epoch(&s.watched_at)).unwrap_or(0);
        t_b.cmp(&t_a)
    });

    result_groups
}

pub async fn get_duplicates_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<DuplicatesQuery>,
) -> Result<Json<DuplicateScanResponse>, (StatusCode, Json<serde_json::Value>)> {
    let window_hours = query.window_hours.unwrap_or(24).clamp(1, 720);

    let all_scrobbles = sqlx::query_as::<_, Scrobble>(
        "SELECT * FROM scrobbles WHERE user_id = ? ORDER BY watched_at ASC, created_at ASC"
    )
    .bind(&auth.user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Database error fetching scrobbles: {}", e) })),
        )
    })?;

    let groups = find_duplicate_groups(&all_scrobbles, window_hours);
    let total_redundant: usize = groups.iter().map(|g| g.scrobbles.len().saturating_sub(1)).sum();

    Ok(Json(DuplicateScanResponse {
        total_duplicate_groups: groups.len(),
        total_redundant_scrobbles: total_redundant,
        window_hours,
        groups,
    }))
}

pub async fn clean_duplicates_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(payload): Json<CleanDuplicatesRequest>,
) -> Result<Json<CleanDuplicatesResponse>, (StatusCode, Json<serde_json::Value>)> {
    let strategy = payload.strategy.as_deref().unwrap_or("keep_oldest");
    let mut to_delete_ids: Vec<String> = Vec::new();

    if strategy == "custom" || (payload.delete_ids.is_some() && !payload.delete_ids.as_ref().unwrap().is_empty()) {
        if let Some(ids) = payload.delete_ids {
            to_delete_ids = ids;
        }
    } else {
        let window_hours = payload.window_hours.unwrap_or(24).clamp(1, 720);
        let all_scrobbles = sqlx::query_as::<_, Scrobble>(
            "SELECT * FROM scrobbles WHERE user_id = ? ORDER BY watched_at ASC, created_at ASC"
        )
        .bind(&auth.user_id)
        .fetch_all(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Database error fetching scrobbles: {}", e) })),
            )
        })?;

        let groups = find_duplicate_groups(&all_scrobbles, window_hours);

        for mut group in groups {
            if group.scrobbles.len() <= 1 {
                continue;
            }

            if strategy == "keep_newest" {
                // Keep the newest one (index 0 when reversed), delete the older duplicates
                group.scrobbles.sort_by(|a, b| {
                    let t_a = parse_timestamp_epoch(&a.watched_at).unwrap_or(0);
                    let t_b = parse_timestamp_epoch(&b.watched_at).unwrap_or(0);
                    t_b.cmp(&t_a).then_with(|| b.created_at.cmp(&a.created_at))
                });
                for s in &group.scrobbles[1..] {
                    to_delete_ids.push(s.id.clone());
                }
            } else {
                // Default: keep_oldest. Keep index 0, delete the newer duplicates
                group.scrobbles.sort_by(|a, b| {
                    let t_a = parse_timestamp_epoch(&a.watched_at).unwrap_or(0);
                    let t_b = parse_timestamp_epoch(&b.watched_at).unwrap_or(0);
                    t_a.cmp(&t_b).then_with(|| a.created_at.cmp(&b.created_at))
                });
                for s in &group.scrobbles[1..] {
                    to_delete_ids.push(s.id.clone());
                }
            }
        }
    }

    if to_delete_ids.is_empty() {
        return Ok(Json(CleanDuplicatesResponse {
            success: true,
            deleted_count: 0,
            message: "No duplicates found to delete.".to_string(),
        }));
    }

    // Execute atomic deletion strictly scoped to auth.user_id
    let mut deleted_count: usize = 0;
    for id in &to_delete_ids {
        if let Ok(res) = sqlx::query("DELETE FROM scrobbles WHERE id = ? AND user_id = ?")
            .bind(id)
            .bind(&auth.user_id)
            .execute(&state.pool)
            .await
        {
            deleted_count += res.rows_affected() as usize;
        }
    }

    Ok(Json(CleanDuplicatesResponse {
        success: true,
        deleted_count,
        message: format!("Successfully pruned {} duplicate scrobbles.", deleted_count),
    }))
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

    tracing::info!(
        user_id = %auth.user_id,
        scrobble_id = %id,
        title = %search_title,
        year = ?effective_year,
        media_type = %scrobble.media_type,
        "Initiating manual metadata and artwork refresh"
    );

    // 1. Fetch metadata from active provider (TheTVDB or TMDB) if configured
    let metadata_source = state.get_effective_metadata_source().await;
    let provider_owned = if metadata_source == "tmdb" {
        if let Some(key) = state.get_effective_tmdb_api_key().await.filter(|k| !k.trim().is_empty()) {
            let base_url = state.get_effective_tmdb_base_url().await;
            Some(crate::api::tvdb::MetadataProviderOwned::Tmdb(crate::api::tmdb::TmdbClient::new(base_url, key)))
        } else if let Some(key) = state.get_effective_tvdb_api_key().await.filter(|k| !k.trim().is_empty()) {
            let pin = state.get_effective_tvdb_pin().await;
            let base_url = state.get_effective_tvdb_base_url().await;
            Some(crate::api::tvdb::MetadataProviderOwned::Tvdb(crate::api::tvdb::TvdbClient::new(base_url, key, pin)))
        } else {
            None
        }
    } else {
        if let Some(key) = state.get_effective_tvdb_api_key().await.filter(|k| !k.trim().is_empty()) {
            let pin = state.get_effective_tvdb_pin().await;
            let base_url = state.get_effective_tvdb_base_url().await;
            Some(crate::api::tvdb::MetadataProviderOwned::Tvdb(crate::api::tvdb::TvdbClient::new(base_url, key, pin)))
        } else if let Some(key) = state.get_effective_tmdb_api_key().await.filter(|k| !k.trim().is_empty()) {
            let base_url = state.get_effective_tmdb_base_url().await;
            Some(crate::api::tvdb::MetadataProviderOwned::Tmdb(crate::api::tmdb::TmdbClient::new(base_url, key)))
        } else {
            None
        }
    };

    if let Some(ref p_owned) = provider_owned {
        let provider = p_owned.as_ref();
        let mut meta_opt = None;
        if let Some(ref tid) = scrobble.tvdb_id {
            let clean_tid = tid.trim();
            meta_opt = if is_episode {
                match provider.get_series_extended(clean_tid).await {
                    Ok(m) => Some(m),
                    Err(_) => provider.get_movie_extended(clean_tid).await.ok(),
                }
            } else {
                match provider.get_movie_extended(clean_tid).await {
                    Ok(m) => Some(m),
                    Err(_) => provider.get_series_extended(clean_tid).await.ok(),
                }
            };
        }

        if meta_opt.is_none() {
            if let Ok(Some(search_match)) = provider.search(&search_title, if is_episode { "series" } else { "movie" }, effective_year).await {
                let match_is_series = search_match.entity_type.as_deref() == Some("series") || is_episode;
                let mut meta = if match_is_series {
                    match provider.get_series_extended(&search_match.id).await {
                        Ok(m) => m,
                        Err(_) => provider.get_movie_extended(&search_match.id).await.unwrap_or_default(),
                    }
                } else {
                    match provider.get_movie_extended(&search_match.id).await {
                        Ok(m) => m,
                        Err(_) => provider.get_series_extended(&search_match.id).await.unwrap_or_default(),
                    }
                };

                if meta.overview.is_none() || meta.overview.as_deref() == Some("") {
                    meta.overview = search_match.overview.clone();
                }
                if meta.image_url.is_none() || meta.image_url.as_deref() == Some("") {
                    meta.image_url = search_match.image_url.clone();
                }
                meta_opt = Some(meta);
            }
        }

            if let Some(meta) = meta_opt {
                let now_str = Utc::now().to_rfc3339();
                if is_episode {
                    let show_name = scrobble.grandparent_title.as_deref().unwrap_or(&scrobble.title);
                    let _ = sqlx::query(
                        r#"
                        UPDATE scrobbles
                        SET genres = COALESCE(NULLIF(?, ''), NULLIF(genres, '')),
                            actors = COALESCE(NULLIF(?, ''), NULLIF(actors, '')),
                            directors = COALESCE(NULLIF(?, ''), NULLIF(directors, '')),
                            director = COALESCE(NULLIF(?, ''), NULLIF(director, '')),
                            cinematographer = COALESCE(NULLIF(?, ''), NULLIF(cinematographer, '')),
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
                    .bind(&meta.director)
                    .bind(&meta.cinematographer)
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
                            director = COALESCE(NULLIF(?, ''), NULLIF(director, '')),
                            cinematographer = COALESCE(NULLIF(?, ''), NULLIF(cinematographer, '')),
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
                    .bind(&meta.director)
                    .bind(&meta.cinematographer)
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

    tracing::info!(
        user_id = %auth.user_id,
        scrobble_id = %id,
        title = %refreshed.title,
        "Completed manual metadata and artwork refresh"
    );

    Ok(Json(serde_json::json!({
        "status": "refreshed",
        "scrobble": refreshed,
        "message": "Metadata and artwork refreshed successfully"
    })))
}

#[derive(Debug, Deserialize)]
pub struct MatchSearchQuery {
    pub query: Option<String>,
    pub provider: Option<String>,
}

pub async fn search_match_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Query(query_params): Query<MatchSearchQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let scrobble = match sqlx::query_as::<_, Scrobble>(
        "SELECT * FROM scrobbles WHERE id = ? AND (user_id = ? OR ? = 'admin')",
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
    let default_title = if is_episode {
        scrobble.grandparent_title.as_deref().unwrap_or(&scrobble.title)
    } else {
        &scrobble.title
    };

    let search_query = query_params
        .query
        .as_deref()
        .map(|q| q.trim())
        .filter(|q| !q.is_empty())
        .unwrap_or(default_title);

    let requested_provider = query_params
        .provider
        .as_deref()
        .unwrap_or("auto")
        .to_lowercase();

    let mut results: Vec<crate::api::tvdb::TvdbSearchResult> = Vec::new();
    let effective_source = state.get_effective_metadata_source().await;

    let should_query_tmdb = requested_provider == "tmdb" || (requested_provider == "auto" && effective_source == "tmdb");
    let should_query_tvdb = requested_provider == "tvdb" || (requested_provider == "auto" && effective_source == "tvdb");

    if should_query_tmdb {
        if let Some(key) = state.get_effective_tmdb_api_key().await.filter(|k| !k.trim().is_empty()) {
            let base_url = state.get_effective_tmdb_base_url().await;
            let client = crate::api::tmdb::TmdbClient::new(base_url, key);
            if let Ok(cands) = client.search_candidates(search_query, &scrobble.media_type, scrobble.year).await {
                results.extend(cands);
            }
        }
    }

    if should_query_tvdb || (requested_provider == "auto" && results.is_empty()) {
        if let Some(key) = state.get_effective_tvdb_api_key().await.filter(|k| !k.trim().is_empty()) {
            let pin = state.get_effective_tvdb_pin().await;
            let base_url = state.get_effective_tvdb_base_url().await;
            let client = crate::api::tvdb::TvdbClient::new(base_url, key, pin);
            if let Ok(cands) = client.search_candidates(search_query, &scrobble.media_type, scrobble.year).await {
                results.extend(cands);
            }
        }
    }

    if results.is_empty() && requested_provider == "auto" && effective_source == "tvdb" {
        if let Some(key) = state.get_effective_tmdb_api_key().await.filter(|k| !k.trim().is_empty()) {
            let base_url = state.get_effective_tmdb_base_url().await;
            let client = crate::api::tmdb::TmdbClient::new(base_url, key);
            if let Ok(cands) = client.search_candidates(search_query, &scrobble.media_type, scrobble.year).await {
                results.extend(cands);
            }
        }
    }

    Ok(Json(serde_json::json!({
        "query": search_query,
        "media_type": scrobble.media_type,
        "year": scrobble.year,
        "results": results
    })))
}

pub async fn fix_match_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(payload): Json<crate::models::scrobble::FixMatchRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let raw_id = payload
        .id
        .or(payload.tmdb_id.clone())
        .or(payload.tvdb_id.clone())
        .unwrap_or_default();
    let raw_id = raw_id.trim();

    if raw_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Match ID cannot be empty" })),
        ));
    }

    let mut provider_choice = payload.provider.as_deref().unwrap_or("auto").to_lowercase();
    let mut clean_id = raw_id.to_string();

    if clean_id.starts_with("tmdb-") {
        provider_choice = "tmdb".to_string();
        clean_id = clean_id.strip_prefix("tmdb-").unwrap().to_string();
    } else if clean_id.starts_with("tvdb-") {
        provider_choice = "tvdb".to_string();
        clean_id = clean_id.strip_prefix("tvdb-").unwrap().to_string();
    }

    clean_id = clean_id
        .strip_prefix("series-")
        .or_else(|| clean_id.strip_prefix("movie-"))
        .unwrap_or(&clean_id)
        .trim()
        .to_string();

    if provider_choice == "auto" {
        if payload.tmdb_id.is_some() {
            provider_choice = "tmdb".to_string();
        } else if payload.tvdb_id.is_some() {
            provider_choice = "tvdb".to_string();
        } else {
            provider_choice = state.get_effective_metadata_source().await;
        }
    }

    tracing::info!(
        user_id = %auth.user_id,
        scrobble_id = %id,
        provider = %provider_choice,
        target_id = %clean_id,
        "Initiating manual match fix"
    );

    // 1. Verify scrobble ownership
    let scrobble = match sqlx::query_as::<_, Scrobble>(
        "SELECT * FROM scrobbles WHERE id = ? AND (user_id = ? OR ? = 'admin')",
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
    let is_tmdb = provider_choice == "tmdb";

    // 2. Update tvdb_id or tmdb_id on scrobble (and sibling episodes if it's a TV show)
    if is_episode {
        let show_name = scrobble.grandparent_title.as_deref().unwrap_or(&scrobble.title);
        if is_tmdb {
            let _ = sqlx::query(
                "UPDATE scrobbles SET tmdb_id = ? WHERE id = ? OR (grandparent_title IS NOT NULL AND LOWER(TRIM(grandparent_title)) = LOWER(TRIM(?))) OR (media_type = 'episode' AND LOWER(TRIM(title)) = LOWER(TRIM(?)))"
            )
            .bind(&clean_id)
            .bind(&scrobble.id)
            .bind(show_name)
            .bind(show_name)
            .execute(&state.pool)
            .await;
        } else {
            let _ = sqlx::query(
                "UPDATE scrobbles SET tvdb_id = ? WHERE id = ? OR (grandparent_title IS NOT NULL AND LOWER(TRIM(grandparent_title)) = LOWER(TRIM(?))) OR (media_type = 'episode' AND LOWER(TRIM(title)) = LOWER(TRIM(?)))"
            )
            .bind(&clean_id)
            .bind(&scrobble.id)
            .bind(show_name)
            .bind(show_name)
            .execute(&state.pool)
            .await;
        }
    } else {
        if is_tmdb {
            let _ = sqlx::query("UPDATE scrobbles SET tmdb_id = ? WHERE id = ?")
                .bind(&clean_id)
                .bind(&scrobble.id)
                .execute(&state.pool)
                .await;
        } else {
            let _ = sqlx::query("UPDATE scrobbles SET tvdb_id = ? WHERE id = ?")
                .bind(&clean_id)
                .bind(&scrobble.id)
                .execute(&state.pool)
                .await;
        }
    }

    // 3. Immediately fetch metadata from the selected provider
    let mut fetched_meta: Option<crate::api::tvdb::TvdbMetadata> = None;

    if is_tmdb {
        if let Some(key) = state.get_effective_tmdb_api_key().await.filter(|k| !k.trim().is_empty()) {
            let base_url = state.get_effective_tmdb_base_url().await;
            let client = crate::api::tmdb::TmdbClient::new(base_url, key);
            let meta_res = if is_episode {
                match client.get_series_details(&clean_id).await {
                    Ok(m) => Ok(m),
                    Err(_) => client.get_movie_details(&clean_id).await,
                }
            } else {
                match client.get_movie_details(&clean_id).await {
                    Ok(m) => Ok(m),
                    Err(_) => client.get_series_details(&clean_id).await,
                }
            };
            if let Ok(m) = meta_res {
                fetched_meta = Some(m);
            }
        }
    } else {
        if let Some(key) = state.get_effective_tvdb_api_key().await.filter(|k| !k.trim().is_empty()) {
            let pin = state.get_effective_tvdb_pin().await;
            let base_url = state.get_effective_tvdb_base_url().await;
            let client = crate::api::tvdb::TvdbClient::new(base_url, key, pin);
            let meta_res = if is_episode {
                match client.get_series_extended(&clean_id).await {
                    Ok(m) => Ok(m),
                    Err(_) => client.get_movie_extended(&clean_id).await,
                }
            } else {
                match client.get_movie_extended(&clean_id).await {
                    Ok(m) => Ok(m),
                    Err(_) => client.get_series_extended(&clean_id).await,
                }
            };
            if let Ok(m) = meta_res {
                fetched_meta = Some(m);
            }
        }
    }

    let now_str = Utc::now().to_rfc3339();

    if let Some(meta) = fetched_meta {
        let has_image = meta.image_url.is_some();
        if is_episode {
            let show_name = scrobble.grandparent_title.as_deref().unwrap_or(&scrobble.title);
            let _ = sqlx::query(
                r#"
                UPDATE scrobbles
                SET genres = COALESCE(NULLIF(?, ''), NULLIF(genres, '')),
                    actors = COALESCE(NULLIF(?, ''), NULLIF(actors, '')),
                    directors = COALESCE(NULLIF(?, ''), NULLIF(directors, '')),
                    director = COALESCE(NULLIF(?, ''), NULLIF(director, '')),
                    cinematographer = COALESCE(NULLIF(?, ''), NULLIF(cinematographer, '')),
                    overview = COALESCE(NULLIF(?, ''), NULLIF(overview, '')),
                    thumb_url = COALESCE(NULLIF(?, ''), thumb_url),
                    metadata_updated_at = ?
                WHERE id = ? OR (grandparent_title IS NOT NULL AND LOWER(TRIM(grandparent_title)) = LOWER(TRIM(?))) OR (media_type = 'episode' AND LOWER(TRIM(title)) = LOWER(TRIM(?)))
                "#
            )
            .bind(&meta.genres)
            .bind(&meta.actors)
            .bind(&meta.directors)
            .bind(&meta.director)
            .bind(&meta.cinematographer)
            .bind(&meta.overview)
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
                    director = COALESCE(NULLIF(?, ''), NULLIF(director, '')),
                    cinematographer = COALESCE(NULLIF(?, ''), NULLIF(cinematographer, '')),
                    overview = COALESCE(NULLIF(?, ''), NULLIF(overview, '')),
                    thumb_url = COALESCE(NULLIF(?, ''), thumb_url),
                    metadata_updated_at = ?
                WHERE id = ?
                "#
            )
            .bind(&meta.genres)
            .bind(&meta.actors)
            .bind(&meta.directors)
            .bind(&meta.director)
            .bind(&meta.cinematographer)
            .bind(&meta.overview)
            .bind(&meta.image_url)
            .bind(&now_str)
            .bind(&scrobble.id)
            .execute(&state.pool)
            .await;
        }

        if has_image {
            crate::api::images::process_missing_images_once(&state).await;
            state.image_notify.notify_waiters();
        }
    }

    // 4. Notify background workers if poster or metadata needs processing
    let needs_poster = scrobble.thumb_url.as_deref()
        .map(|u| u.is_empty() || u.starts_with("/library/") || u.starts_with("/photo/"))
        .unwrap_or(true);
    if needs_poster {
        state.image_notify.notify_waiters();
        state.image_notify.notify_one();
    }

    // Return the updated scrobble
    let refreshed = sqlx::query_as::<_, Scrobble>("SELECT * FROM scrobbles WHERE id = ?")
        .bind(&scrobble.id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to retrieve updated scrobble: {}", e) })),
            )
        })?;

    let provider_label = if is_tmdb { "TMDB" } else { "TheTVDB" };

    tracing::info!(
        user_id = %auth.user_id,
        scrobble_id = %id,
        provider = %provider_choice,
        target_id = %clean_id,
        "Completed manual match fix"
    );

    Ok(Json(serde_json::json!({
        "status": "fixed",
        "id": clean_id,
        "tvdb_id": refreshed.tvdb_id,
        "tmdb_id": refreshed.tmdb_id,
        "provider": if is_tmdb { "tmdb" } else { "tvdb" },
        "scrobble": refreshed,
        "message": format!("Successfully fixed match to {} ID {}", provider_label, clean_id)
    })))
}
