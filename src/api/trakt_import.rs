use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use uuid::Uuid;

use crate::{api::AppState, auth::AuthUser};

#[derive(Debug, Serialize, Deserialize)]
pub struct ImportSummary {
    pub status: String,
    pub imported: usize,
    pub updated: usize,
    pub skipped: usize,
    pub total_processed: usize,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct TraktWatchItem {
    pub title: String,
    pub media_type: String, // "movie" or "episode"
    pub grandparent_title: Option<String>,
    pub parent_title: Option<String>,
    pub season_number: Option<i32>,
    pub episode_number: Option<i32>,
    pub year: Option<i32>,
    pub user_rating: Option<i32>,
    pub genres: Option<String>,
    pub directors: Option<String>,
    pub actors: Option<String>,
    pub guid: Option<String>,
    pub watched_at: String,
}

#[derive(Default, Debug)]
struct RatingLookups {
    movies: HashMap<String, i32>,
    episodes: HashMap<String, i32>,
    shows: HashMap<String, i32>,
}

pub async fn import_trakt_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    body_bytes: axum::body::Bytes,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if body_bytes.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Import payload is empty" })),
        ));
    }

    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let mut items = if is_zip_payload(&body_bytes)
        || content_type.contains("application/zip")
        || content_type.contains("application/x-zip-compressed")
    {
        parse_trakt_zip(&body_bytes).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Failed to parse Trakt ZIP archive: {}", e) })),
            )
        })?
    } else if content_type.starts_with("multipart/form-data") {
        parse_trakt_multipart(content_type, body_bytes).await.map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Failed to parse multipart import: {}", e) })),
            )
        })?
    } else {
        let body_str = String::from_utf8(body_bytes.to_vec()).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Invalid UTF-8 payload: {}", e) })),
            )
        })?;

        let trimmed = body_str.trim();
        if trimmed.starts_with('[') || trimmed.starts_with('{') {
            parse_trakt_json(trimmed).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("Failed to parse Trakt JSON: {}", e) })),
                )
            })?
        } else {
            parse_trakt_csv(trimmed).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("Failed to parse Trakt CSV: {}", e) })),
                )
            })?
        }
    };

    if items.is_empty() {
        return Ok(Json(ImportSummary {
            status: "success".to_string(),
            imported: 0,
            updated: 0,
            skipped: 0,
            total_processed: 0,
            message: "No valid Trakt watch items found in the imported file".to_string(),
        }));
    }

    // Automatically sort all items chronologically (oldest to newest)
    items.sort_by(|a, b| a.watched_at.cmp(&b.watched_at));

    #[derive(sqlx::FromRow)]
    struct ExistingScrobble {
        id: String,
        media_type: String,
        title: String,
        grandparent_title: Option<String>,
        season_number: Option<i32>,
        episode_number: Option<i32>,
        watched_at: String,
        user_rating: Option<i32>,
    }

    let existing_records = sqlx::query_as::<_, ExistingScrobble>(
        r#"
        SELECT id, media_type, title, grandparent_title, season_number, episode_number, watched_at, user_rating
        FROM scrobbles
        WHERE user_id = ?
        "#,
    )
    .bind(&auth.user_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut existing_map: HashMap<String, (String, Option<i32>)> = HashMap::new();
    for rec in existing_records {
        let key = if rec.media_type == "episode" {
            format!(
                "ep:{}:{}:{}:{}",
                rec.grandparent_title.as_deref().unwrap_or("").to_lowercase().trim(),
                rec.season_number.unwrap_or(0),
                rec.episode_number.unwrap_or(0),
                rec.watched_at
            )
        } else {
            format!("mv:{}:{}", rec.title.to_lowercase().trim(), rec.watched_at)
        };
        existing_map.insert(key, (rec.id, rec.user_rating));
    }

    // Execute in a single atomic database transaction for lightning-fast batch insertion
    let mut tx = state.pool.begin().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Database transaction error: {}", e) })),
        )
    })?;

    let mut imported_count = 0;
    let mut updated_count = 0;
    let mut skipped_count = 0;
    let now = Utc::now().to_rfc3339();

    for item in &items {
        let key = if item.media_type == "episode" {
            format!(
                "ep:{}:{}:{}:{}",
                item.grandparent_title.as_deref().unwrap_or("").to_lowercase().trim(),
                item.season_number.unwrap_or(0),
                item.episode_number.unwrap_or(0),
                item.watched_at
            )
        } else {
            format!("mv:{}:{}", item.title.to_lowercase().trim(), item.watched_at)
        };

        if let Some((existing_id, existing_rating)) = existing_map.get(&key) {
            // If item exists but new rating is provided, update rating
            if item.user_rating.is_some() && *existing_rating != item.user_rating {
                let _ = sqlx::query("UPDATE scrobbles SET user_rating = ? WHERE id = ?")
                    .bind(item.user_rating)
                    .bind(existing_id)
                    .execute(&mut *tx)
                    .await;
                updated_count += 1;
                existing_map.insert(key, (existing_id.clone(), item.user_rating));
            } else {
                skipped_count += 1;
            }
            continue;
        }

        // Insert new record with exact Trakt watched_at timestamp and rating
        let scrobble_id = Uuid::new_v4().to_string();
        let item_director = item.directors.as_ref().and_then(|d| d.split(',').next().map(|s| s.trim().to_string()));
        let source_name = Some("trakt".to_string());
        let source_user = Some(auth.email.clone());
        let imported_from = Some("trakt".to_string());
        let insert_res = sqlx::query(
            r#"
            INSERT INTO scrobbles (
                id, user_id, title, media_type, grandparent_title, parent_title,
                season_number, episode_number, year, overview, thumb_url,
                rating_key, guid, duration_ms, user_rating,
                genres, directors, director, cinematographer, actors, watch_status,
                source, source_user, imported_from,
                watched_at, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&scrobble_id)
        .bind(&auth.user_id)
        .bind(&item.title)
        .bind(&item.media_type)
        .bind(&item.grandparent_title)
        .bind(&item.parent_title)
        .bind(item.season_number)
        .bind(item.episode_number)
        .bind(item.year)
        .bind(None::<String>) // overview
        .bind(None::<String>) // thumb_url
        .bind(None::<String>) // rating_key
        .bind(&item.guid)     // guid (e.g. imdb://tt1234567)
        .bind(None::<i64>)    // duration_ms
        .bind(item.user_rating)
        .bind(&item.genres)
        .bind(&item.directors)
        .bind(&item_director)
        .bind(None::<String>) // cinematographer
        .bind(&item.actors)
        .bind("completed")
        .bind(&source_name)
        .bind(&source_user)
        .bind(&imported_from)
        .bind(&item.watched_at)
        .bind(&now)
            .execute(&mut *tx)
            .await;

        match insert_res {
            Ok(_) => {
                imported_count += 1;
                existing_map.insert(key, (scrobble_id, item.user_rating));
            }
            Err(e) => {
                tracing::error!(error = %e, title = %item.title, "Database error inserting Trakt scrobble during import");
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("Failed to insert scrobble '{}': {}", item.title, e) })),
                ));
            }
        }
    }

    tx.commit().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to commit import transaction: {}", e) })),
        )
    })?;

    // Trigger background image cache and metadata enrichment workers
    if imported_count > 0 {
        state.image_notify.notify_one();
        state.metadata_notify.notify_one();
    }

    Ok(Json(ImportSummary {
        status: "success".to_string(),
        imported: imported_count,
        updated: updated_count,
        skipped: skipped_count,
        total_processed: items.len(),
        message: format!(
            "Successfully imported {} scrobbles and updated {} ratings ({} skipped).",
            imported_count, updated_count, skipped_count
        ),
    }))
}

fn is_zip_payload(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x50, 0x4B, 0x03, 0x04])
        || bytes.starts_with(&[0x50, 0x4B, 0x05, 0x06])
        || bytes.starts_with(&[0x50, 0x4B, 0x07, 0x08])
}

async fn parse_trakt_multipart(
    content_type: &str,
    bytes: axum::body::Bytes,
) -> Result<Vec<TraktWatchItem>, String> {
    let boundary = multer::parse_boundary(content_type)
        .map_err(|e| format!("Failed to parse boundary: {}", e))?;

    let cursor = std::io::Cursor::new(bytes);
    let stream = tokio_util::io::ReaderStream::new(cursor);
    let mut multipart = multer::Multipart::new(stream, boundary);

    while let Ok(Some(field)) = multipart.next_field().await {
        let file_name = field.file_name().map(|s| s.to_string());
        let field_bytes = field
            .bytes()
            .await
            .map_err(|e| format!("Failed to read multipart field: {}", e))?;

        if field_bytes.is_empty() {
            continue;
        }

        let is_zip = is_zip_payload(&field_bytes)
            || file_name
                .as_deref()
                .map(|n| n.to_lowercase().ends_with(".zip"))
                .unwrap_or(false);

        if is_zip {
            return parse_trakt_zip(&field_bytes);
        } else {
            let text = String::from_utf8(field_bytes.to_vec())
                .map_err(|e| format!("Invalid UTF-8 in uploaded file: {}", e))?;
            let trimmed = text.trim();
            if trimmed.starts_with('[') || trimmed.starts_with('{') {
                return parse_trakt_json(trimmed);
            } else {
                return parse_trakt_csv(trimmed);
            }
        }
    }

    Err("No valid file found in multipart upload".to_string())
}

pub fn parse_trakt_zip(zip_bytes: &[u8]) -> Result<Vec<TraktWatchItem>, String> {
    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|e| format!("Invalid or corrupted ZIP archive: {}", e))?;

    let mut lookups = RatingLookups::default();
    let mut history_files: Vec<(String, String)> = Vec::new();
    let mut watched_movies_files: Vec<(String, String)> = Vec::new();
    let mut watched_shows_files: Vec<(String, String)> = Vec::new();
    let mut csv_files: Vec<(String, String)> = Vec::new();
    let mut other_json_files: Vec<(String, String)> = Vec::new();

    const MAX_FILE_DECOMPRESSED_BYTES: u64 = 25 * 1024 * 1024; // 25 MB per file
    const MAX_TOTAL_DECOMPRESSED_BYTES: usize = 100 * 1024 * 1024; // 100 MB total archive
    let mut total_decompressed_bytes: usize = 0;

    for i in 0..archive.len() {
        let mut file = match archive.by_index(i) {
            Ok(f) => f,
            Err(_) => continue,
        };

        if file.is_dir() {
            continue;
        }

        let full_name = match file.name() {
            Ok(n) => n.to_string(),
            Err(_) => continue,
        };
        if full_name.starts_with("__MACOSX") || full_name.contains("/.") {
            continue;
        }

        let base_name = std::path::Path::new(&full_name)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(&full_name)
            .to_lowercase();

        // ZIP bomb / memory exhaustion defense
        if file.size() > MAX_FILE_DECOMPRESSED_BYTES {
            tracing::warn!("Skipping ZIP entry {}: uncompressed size {} exceeds 25 MB limit", full_name, file.size());
            continue;
        }

        let mut content = String::new();
        let mut limited = (&mut file).take(MAX_FILE_DECOMPRESSED_BYTES + 1);
        if limited.read_to_string(&mut content).is_err() {
            continue;
        }
        if content.len() as u64 > MAX_FILE_DECOMPRESSED_BYTES {
            tracing::warn!("Skipping ZIP entry {}: decompressed size exceeds 25 MB limit", full_name);
            continue;
        }

        total_decompressed_bytes = total_decompressed_bytes.saturating_add(content.len());
        if total_decompressed_bytes > MAX_TOTAL_DECOMPRESSED_BYTES {
            return Err("Decompressed archive size exceeds maximum limit of 100 MB".to_string());
        }

        if base_name.contains("rating") && base_name.ends_with(".json") && !base_name.starts_with("notes-") {
            ingest_ratings_json(&content, &mut lookups);
        } else if (base_name.starts_with("watched-history") || base_name.contains("history")) && base_name.ends_with(".json") {
            history_files.push((base_name, content));
        } else if base_name.starts_with("watched-movies") && base_name.ends_with(".json") {
            watched_movies_files.push((base_name, content));
        } else if base_name.starts_with("watched-shows") && base_name.ends_with(".json") {
            watched_shows_files.push((base_name, content));
        } else if base_name.ends_with(".csv") {
            csv_files.push((base_name, content));
        } else if base_name.ends_with(".json")
            && !base_name.starts_with("user-")
            && !base_name.starts_with("notes-")
            && !base_name.starts_with("hidden-")
            && !base_name.starts_with("network-")
            && !base_name.starts_with("likes-")
            && !base_name.starts_with("comments-")
            && !base_name.starts_with("lists-")
        {
            other_json_files.push((base_name, content));
        }
    }

    let mut all_items: Vec<TraktWatchItem> = Vec::new();

    // 1. If history files exist, parse all of them
    if !history_files.is_empty() {
        for (_name, json_str) in history_files {
            if let Ok(mut items) = parse_trakt_json_with_ratings(&json_str, Some(&lookups)) {
                all_items.append(&mut items);
            }
        }
    } else {
        // Fallback: parse watched-movies and watched-shows if no history files present
        for (_name, json_str) in watched_movies_files {
            if let Ok(mut items) = parse_trakt_watched_movies_json(&json_str, Some(&lookups)) {
                all_items.append(&mut items);
            }
        }
        for (_name, json_str) in watched_shows_files {
            if let Ok(mut items) = parse_trakt_watched_shows_json(&json_str, Some(&lookups)) {
                all_items.append(&mut items);
            }
        }
    }

    // 2. CSV files in zip
    for (_name, csv_str) in csv_files {
        if let Ok(mut items) = parse_trakt_csv(&csv_str) {
            all_items.append(&mut items);
        }
    }

    // 3. Fallback to other json files if still empty
    if all_items.is_empty() {
        for (_name, json_str) in other_json_files {
            if let Ok(mut items) = parse_trakt_json_with_ratings(&json_str, Some(&lookups)) {
                all_items.append(&mut items);
            }
        }
    }

    // Automatically sort chronologically (oldest to newest)
    all_items.sort_by(|a, b| a.watched_at.cmp(&b.watched_at));

    Ok(all_items)
}

fn ingest_ratings_json(json_str: &str, lookups: &mut RatingLookups) {
    let parsed: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return,
    };

    let items = match parsed.as_array() {
        Some(arr) => arr,
        None => return,
    };

    for item in items {
        let rating = match item
            .get("rating")
            .and_then(|r| r.as_i64().or_else(|| r.as_str().and_then(|s| s.parse().ok())))
        {
            Some(r) => r as i32,
            None => continue,
        };

        let media_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");

        if media_type == "movie" || item.get("movie").is_some() {
            if let Some(m) = item.get("movie") {
                if let Some(ids) = m.get("ids") {
                    if let Some(imdb) = ids.get("imdb").and_then(|i| i.as_str()) {
                        lookups.movies.insert(format!("imdb:{}", imdb.to_lowercase()), rating);
                    }
                    if let Some(tmdb) = ids.get("tmdb").and_then(|i| i.as_i64().or_else(|| i.as_str().and_then(|s| s.parse().ok()))) {
                        lookups.movies.insert(format!("tmdb:{}", tmdb), rating);
                    }
                    if let Some(trakt) = ids.get("trakt").and_then(|i| i.as_i64().or_else(|| i.as_str().and_then(|s| s.parse().ok()))) {
                        lookups.movies.insert(format!("trakt:{}", trakt), rating);
                    }
                }
                if let Some(title) = m.get("title").and_then(|t| t.as_str()) {
                    let year = m.get("year").and_then(|y| y.as_i64()).unwrap_or(0);
                    let title_norm = title.trim().to_lowercase();
                    lookups.movies.insert(format!("title:{}:{}", title_norm, year), rating);
                    lookups.movies.insert(format!("title:{}", title_norm), rating);
                }
            }
        } else if media_type == "episode" || item.get("episode").is_some() {
            if let Some(ep) = item.get("episode") {
                if let Some(ids) = ep.get("ids") {
                    if let Some(imdb) = ids.get("imdb").and_then(|i| i.as_str()) {
                        lookups.episodes.insert(format!("imdb:{}", imdb.to_lowercase()), rating);
                    }
                    if let Some(tmdb) = ids.get("tmdb").and_then(|i| i.as_i64().or_else(|| i.as_str().and_then(|s| s.parse().ok()))) {
                        lookups.episodes.insert(format!("tmdb:{}", tmdb), rating);
                    }
                    if let Some(tvdb) = ids.get("tvdb").and_then(|i| i.as_i64().or_else(|| i.as_str().and_then(|s| s.parse().ok()))) {
                        lookups.episodes.insert(format!("tvdb:{}", tvdb), rating);
                    }
                    if let Some(trakt) = ids.get("trakt").and_then(|i| i.as_i64().or_else(|| i.as_str().and_then(|s| s.parse().ok()))) {
                        lookups.episodes.insert(format!("trakt:{}", trakt), rating);
                    }
                }
                let season = ep.get("season").and_then(|s| s.as_i64()).unwrap_or(0);
                let number = ep.get("number").and_then(|n| n.as_i64()).unwrap_or(0);
                if let Some(show) = item.get("show") {
                    if let Some(show_title) = show.get("title").and_then(|t| t.as_str()) {
                        let show_norm = show_title.trim().to_lowercase();
                        lookups.episodes.insert(format!("ep:{}:{}:{}", show_norm, season, number), rating);
                    }
                }
            }
        } else if media_type == "show" || item.get("show").is_some() {
            if let Some(show) = item.get("show") {
                if let Some(ids) = show.get("ids") {
                    if let Some(imdb) = ids.get("imdb").and_then(|i| i.as_str()) {
                        lookups.shows.insert(format!("imdb:{}", imdb.to_lowercase()), rating);
                    }
                    if let Some(tmdb) = ids.get("tmdb").and_then(|i| i.as_i64().or_else(|| i.as_str().and_then(|s| s.parse().ok()))) {
                        lookups.shows.insert(format!("tmdb:{}", tmdb), rating);
                    }
                    if let Some(tvdb) = ids.get("tvdb").and_then(|i| i.as_i64().or_else(|| i.as_str().and_then(|s| s.parse().ok()))) {
                        lookups.shows.insert(format!("tvdb:{}", tvdb), rating);
                    }
                    if let Some(trakt) = ids.get("trakt").and_then(|i| i.as_i64().or_else(|| i.as_str().and_then(|s| s.parse().ok()))) {
                        lookups.shows.insert(format!("trakt:{}", trakt), rating);
                    }
                }
                if let Some(title) = show.get("title").and_then(|t| t.as_str()) {
                    lookups.shows.insert(format!("show:{}", title.trim().to_lowercase()), rating);
                }
            }
        }
    }
}

pub fn parse_trakt_json(json_str: &str) -> Result<Vec<TraktWatchItem>, String> {
    parse_trakt_json_with_ratings(json_str, None)
}

fn parse_trakt_json_with_ratings(
    json_str: &str,
    ratings: Option<&RatingLookups>,
) -> Result<Vec<TraktWatchItem>, String> {
    let parsed_val: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("JSON syntax error: {}", e))?;

    let array = if let Some(arr) = parsed_val.as_array() {
        arr
    } else if let Some(items) = parsed_val.get("items").and_then(|i| i.as_array()) {
        items
    } else if let Some(history) = parsed_val.get("history").and_then(|h| h.as_array()) {
        history
    } else {
        return Err("Expected JSON array of Trakt history items".to_string());
    };

    let mut result = Vec::new();

    for item in array {
        let media_type = item
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or_else(|| {
                if item.get("episode").is_some() || item.get("show").is_some() {
                    "episode"
                } else {
                    "movie"
                }
            });

        // Parse watched_at date/time
        let watched_at = item
            .get("watched_at")
            .or_else(|| item.get("last_watched_at"))
            .or_else(|| item.get("rated_at"))
            .and_then(|w| w.as_str())
            .map(normalize_timestamp)
            .unwrap_or_else(|| Utc::now().to_rfc3339());

        // Parse rating directly from item if present
        let mut user_rating = item
            .get("rating")
            .or_else(|| item.get("user_rating"))
            .and_then(|r| r.as_i64().or_else(|| r.as_str().and_then(|s| s.parse().ok())))
            .map(|r| r as i32);

        let genres = item.get("genres").and_then(|g| {
            if let Some(arr) = g.as_array() {
                let tags: Vec<String> = arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();
                if tags.is_empty() { None } else { Some(tags.join(", ")) }
            } else {
                g.as_str().map(|s| s.to_string())
            }
        });
        let directors = item.get("directors").or_else(|| item.get("director")).and_then(|d| {
            if let Some(arr) = d.as_array() {
                let tags: Vec<String> = arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();
                if tags.is_empty() { None } else { Some(tags.join(", ")) }
            } else {
                d.as_str().map(|s| s.to_string())
            }
        });
        let actors = item.get("actors").or_else(|| item.get("cast")).and_then(|a| {
            if let Some(arr) = a.as_array() {
                let tags: Vec<String> = arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();
                if tags.is_empty() { None } else { Some(tags.join(", ")) }
            } else {
                a.as_str().map(|s| s.to_string())
            }
        });

        if media_type == "episode" {
            let ep_obj = item.get("episode");
            let show_obj = item.get("show");

            let ep_ids = ep_obj.and_then(|e| e.get("ids"));
            let show_ids = show_obj.and_then(|s| s.get("ids"));

            let grandparent_title = show_obj
                .and_then(|s| s.get("title"))
                .and_then(|t| t.as_str())
                .or_else(|| item.get("show_title").and_then(|s| s.as_str()))
                .map(|s| s.to_string());

            let season_number = ep_obj
                .and_then(|e| e.get("season"))
                .and_then(|s| s.as_i64().or_else(|| s.as_str().and_then(|x| x.parse().ok())))
                .or_else(|| item.get("season").and_then(|s| s.as_i64()))
                .map(|s| s as i32);

            let episode_number = ep_obj
                .and_then(|e| e.get("number"))
                .and_then(|n| n.as_i64().or_else(|| n.as_str().and_then(|x| x.parse().ok())))
                .or_else(|| item.get("episode").and_then(|e| e.as_i64()))
                .map(|e| e as i32);

            let title = ep_obj
                .and_then(|e| e.get("title"))
                .and_then(|t| t.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    episode_number.map(|num| format!("Episode {}", num))
                })
                .unwrap_or_else(|| "Unknown Episode".to_string());

            let year = show_obj
                .and_then(|s| s.get("year"))
                .and_then(|y| y.as_i64())
                .or_else(|| item.get("year").and_then(|y| y.as_i64()))
                .map(|y| y as i32);

            let parent_title = season_number.map(|s| format!("Season {}", s));

            // Extract GUID (e.g. imdb://tt1234567 or plex guid)
            let guid = ep_ids
                .and_then(|i| i.get("imdb"))
                .and_then(|v| v.as_str())
                .map(|imdb| format!("imdb://{}", imdb))
                .or_else(|| {
                    ep_ids
                        .and_then(|i| i.get("plex"))
                        .and_then(|p| p.get("guid"))
                        .and_then(|v| v.as_str())
                        .map(|g| format!("plex://{}", g))
                })
                .or_else(|| {
                    show_ids
                        .and_then(|i| i.get("imdb"))
                        .and_then(|v| v.as_str())
                        .map(|imdb| format!("imdb://{}", imdb))
                });

            // Match rating from lookup tables if not embedded directly in item
            if user_rating.is_none() {
                if let Some(r) = ratings {
                    user_rating = find_episode_rating(ep_ids, show_ids, grandparent_title.as_deref(), season_number, episode_number, r);
                }
            }

            result.push(TraktWatchItem {
                title,
                media_type: "episode".to_string(),
                grandparent_title,
                parent_title,
                season_number,
                episode_number,
                year,
                user_rating,
                genres,
                directors,
                actors,
                guid,
                watched_at,
            });
        } else {
            // Movie
            let movie_obj = item.get("movie");
            let movie_ids = movie_obj.and_then(|m| m.get("ids"));

            let title = movie_obj
                .and_then(|m| m.get("title"))
                .and_then(|t| t.as_str())
                .or_else(|| item.get("title").and_then(|t| t.as_str()))
                .unwrap_or("Unknown Movie")
                .to_string();

            let year = movie_obj
                .and_then(|m| m.get("year"))
                .and_then(|y| y.as_i64())
                .or_else(|| item.get("year").and_then(|y| y.as_i64()))
                .map(|y| y as i32);

            let guid = movie_ids
                .and_then(|i| i.get("imdb"))
                .and_then(|v| v.as_str())
                .map(|imdb| format!("imdb://{}", imdb))
                .or_else(|| {
                    movie_ids
                        .and_then(|i| i.get("plex"))
                        .and_then(|p| p.get("guid"))
                        .and_then(|v| v.as_str())
                        .map(|g| format!("plex://{}", g))
                });

            if user_rating.is_none() {
                if let Some(r) = ratings {
                    user_rating = find_movie_rating(movie_ids, &title, year, r);
                }
            }

            result.push(TraktWatchItem {
                title,
                media_type: "movie".to_string(),
                grandparent_title: None,
                parent_title: None,
                season_number: None,
                episode_number: None,
                year,
                user_rating,
                genres,
                directors,
                actors,
                guid,
                watched_at,
            });
        }
    }

    Ok(result)
}

fn find_movie_rating(
    ids: Option<&serde_json::Value>,
    title: &str,
    year: Option<i32>,
    lookups: &RatingLookups,
) -> Option<i32> {
    if let Some(ids) = ids {
        if let Some(imdb) = ids.get("imdb").and_then(|i| i.as_str()) {
            if let Some(r) = lookups.movies.get(&format!("imdb:{}", imdb.to_lowercase())) {
                return Some(*r);
            }
        }
        if let Some(tmdb) = ids.get("tmdb").and_then(|i| i.as_i64().or_else(|| i.as_str().and_then(|s| s.parse().ok()))) {
            if let Some(r) = lookups.movies.get(&format!("tmdb:{}", tmdb)) {
                return Some(*r);
            }
        }
        if let Some(trakt) = ids.get("trakt").and_then(|i| i.as_i64().or_else(|| i.as_str().and_then(|s| s.parse().ok()))) {
            if let Some(r) = lookups.movies.get(&format!("trakt:{}", trakt)) {
                return Some(*r);
            }
        }
    }

    let title_norm = title.trim().to_lowercase();
    if let Some(r) = lookups.movies.get(&format!("title:{}:{}", title_norm, year.unwrap_or(0))) {
        return Some(*r);
    }
    if let Some(r) = lookups.movies.get(&format!("title:{}", title_norm)) {
        return Some(*r);
    }

    None
}

fn find_episode_rating(
    ep_ids: Option<&serde_json::Value>,
    show_ids: Option<&serde_json::Value>,
    show_title: Option<&str>,
    season: Option<i32>,
    number: Option<i32>,
    lookups: &RatingLookups,
) -> Option<i32> {
    if let Some(ids) = ep_ids {
        if let Some(imdb) = ids.get("imdb").and_then(|i| i.as_str()) {
            if let Some(r) = lookups.episodes.get(&format!("imdb:{}", imdb.to_lowercase())) {
                return Some(*r);
            }
        }
        if let Some(tmdb) = ids.get("tmdb").and_then(|i| i.as_i64().or_else(|| i.as_str().and_then(|s| s.parse().ok()))) {
            if let Some(r) = lookups.episodes.get(&format!("tmdb:{}", tmdb)) {
                return Some(*r);
            }
        }
        if let Some(tvdb) = ids.get("tvdb").and_then(|i| i.as_i64().or_else(|| i.as_str().and_then(|s| s.parse().ok()))) {
            if let Some(r) = lookups.episodes.get(&format!("tvdb:{}", tvdb)) {
                return Some(*r);
            }
        }
        if let Some(trakt) = ids.get("trakt").and_then(|i| i.as_i64().or_else(|| i.as_str().and_then(|s| s.parse().ok()))) {
            if let Some(r) = lookups.episodes.get(&format!("trakt:{}", trakt)) {
                return Some(*r);
            }
        }
    }

    if let (Some(st), Some(sn), Some(en)) = (show_title, season, number) {
        let ep_key = format!("ep:{}:{}:{}", st.trim().to_lowercase(), sn, en);
        if let Some(r) = lookups.episodes.get(&ep_key) {
            return Some(*r);
        }
    }

    // Fallback: check show rating
    if let Some(ids) = show_ids {
        if let Some(imdb) = ids.get("imdb").and_then(|i| i.as_str()) {
            if let Some(r) = lookups.shows.get(&format!("imdb:{}", imdb.to_lowercase())) {
                return Some(*r);
            }
        }
        if let Some(tmdb) = ids.get("tmdb").and_then(|i| i.as_i64().or_else(|| i.as_str().and_then(|s| s.parse().ok()))) {
            if let Some(r) = lookups.shows.get(&format!("tmdb:{}", tmdb)) {
                return Some(*r);
            }
        }
        if let Some(trakt) = ids.get("trakt").and_then(|i| i.as_i64().or_else(|| i.as_str().and_then(|s| s.parse().ok()))) {
            if let Some(r) = lookups.shows.get(&format!("trakt:{}", trakt)) {
                return Some(*r);
            }
        }
    }
    if let Some(st) = show_title {
        if let Some(r) = lookups.shows.get(&format!("show:{}", st.trim().to_lowercase())) {
            return Some(*r);
        }
    }

    None
}

fn parse_trakt_watched_movies_json(
    json_str: &str,
    ratings: Option<&RatingLookups>,
) -> Result<Vec<TraktWatchItem>, String> {
    let parsed: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("JSON syntax error: {}", e))?;

    let array = parsed.as_array().ok_or_else(|| "Expected JSON array in watched-movies".to_string())?;
    let mut result = Vec::new();

    for item in array {
        let movie_obj = match item.get("movie") {
            Some(m) => m,
            None => continue,
        };
        let title = movie_obj
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("Unknown Movie")
            .to_string();

        let year = movie_obj.get("year").and_then(|y| y.as_i64()).map(|y| y as i32);
        let watched_at = item
            .get("last_watched_at")
            .or_else(|| item.get("last_updated_at"))
            .and_then(|w| w.as_str())
            .map(normalize_timestamp)
            .unwrap_or_else(|| Utc::now().to_rfc3339());

        let movie_ids = movie_obj.get("ids");
        let guid = movie_ids
            .and_then(|i| i.get("imdb"))
            .and_then(|v| v.as_str())
            .map(|imdb| format!("imdb://{}", imdb));

        let user_rating = ratings.and_then(|r| find_movie_rating(movie_ids, &title, year, r));

        result.push(TraktWatchItem {
            title,
            media_type: "movie".to_string(),
            grandparent_title: None,
            parent_title: None,
            season_number: None,
            episode_number: None,
            year,
            user_rating,
            genres: None,
            directors: None,
            actors: None,
            guid,
            watched_at,
        });
    }

    Ok(result)
}

fn parse_trakt_watched_shows_json(
    json_str: &str,
    ratings: Option<&RatingLookups>,
) -> Result<Vec<TraktWatchItem>, String> {
    let parsed: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("JSON syntax error: {}", e))?;

    let array = parsed.as_array().ok_or_else(|| "Expected JSON array in watched-shows".to_string())?;
    let mut result = Vec::new();

    for item in array {
        let show_obj = match item.get("show") {
            Some(s) => s,
            None => continue,
        };
        let show_title = show_obj
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("Unknown Show")
            .to_string();
        let show_year = show_obj.get("year").and_then(|y| y.as_i64()).map(|y| y as i32);
        let show_ids = show_obj.get("ids");

        if let Some(seasons) = item.get("seasons").and_then(|s| s.as_array()) {
            for season_item in seasons {
                let season_num = season_item.get("number").and_then(|n| n.as_i64()).map(|n| n as i32).unwrap_or(1);
                if let Some(episodes) = season_item.get("episodes").and_then(|e| e.as_array()) {
                    for ep_item in episodes {
                        let ep_num = ep_item.get("number").and_then(|n| n.as_i64()).map(|n| n as i32).unwrap_or(1);
                        let watched_at = ep_item
                            .get("last_watched_at")
                            .and_then(|w| w.as_str())
                            .map(normalize_timestamp)
                            .unwrap_or_else(|| Utc::now().to_rfc3339());

                        let guid = show_ids
                            .and_then(|i| i.get("imdb"))
                            .and_then(|v| v.as_str())
                            .map(|imdb| format!("imdb://{}", imdb));

                        let user_rating = ratings.and_then(|r| {
                            find_episode_rating(None, show_ids, Some(&show_title), Some(season_num), Some(ep_num), r)
                        });

                        result.push(TraktWatchItem {
                            title: format!("Episode {}", ep_num),
                            media_type: "episode".to_string(),
                            grandparent_title: Some(show_title.clone()),
                            parent_title: Some(format!("Season {}", season_num)),
                            season_number: Some(season_num),
                            episode_number: Some(ep_num),
                            year: show_year,
                            user_rating,
                            genres: None,
                            directors: None,
                            actors: None,
                            guid,
                            watched_at,
                        });
                    }
                }
            }
        }
    }

    Ok(result)
}

pub fn parse_trakt_csv(csv_str: &str) -> Result<Vec<TraktWatchItem>, String> {
    let mut lines = csv_str.lines();
    let header_line = lines
        .next()
        .ok_or_else(|| "CSV file is empty".to_string())?;

    let headers: Vec<String> = parse_csv_line(header_line)
        .into_iter()
        .map(|h| h.trim().to_lowercase().replace([' ', '_', '-'], ""))
        .collect();

    let title_idx = headers
        .iter()
        .position(|h| h == "title" || h == "movietitle" || h == "episodetitle");
    let show_idx = headers
        .iter()
        .position(|h| h == "show" || h == "showtitle" || h == "series");
    let season_idx = headers
        .iter()
        .position(|h| h == "season" || h == "seasonnumber");
    let ep_idx = headers
        .iter()
        .position(|h| h == "episode" || h == "episodenumber");
    let year_idx = headers
        .iter()
        .position(|h| h == "year" || h == "releaseyear");
    let watched_idx = headers
        .iter()
        .position(|h| h == "watchedat" || h == "watched" || h == "date" || h == "watcheddate" || h == "lastwatchedat");
    let rating_idx = headers
        .iter()
        .position(|h| h == "rating" || h == "userrating");
    let type_idx = headers
        .iter()
        .position(|h| h == "type" || h == "mediatype");

    let mut result = Vec::new();

    for line in lines {
        if line.trim().is_empty() {
            continue;
        }

        let fields = parse_csv_line(line);
        let get_val = |opt: Option<usize>| -> Option<String> {
            opt.and_then(|idx| fields.get(idx).map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
        };

        let raw_type = get_val(type_idx).map(|t| t.to_lowercase());
        let show_val = get_val(show_idx);
        let is_episode = raw_type.as_deref() == Some("episode")
            || show_val.is_some()
            || season_idx.is_some();

        let watched_at = get_val(watched_idx)
            .map(|w| normalize_timestamp(&w))
            .unwrap_or_else(|| Utc::now().to_rfc3339());

        let user_rating = get_val(rating_idx).and_then(|r| r.parse::<i32>().ok());
        let year = get_val(year_idx).and_then(|y| y.parse::<i32>().ok());

        if is_episode {
            let grandparent_title = show_val;
            let season_number = get_val(season_idx).and_then(|s| s.parse::<i32>().ok());
            let episode_number = get_val(ep_idx).and_then(|e| e.parse::<i32>().ok());
            let title = get_val(title_idx).unwrap_or_else(|| {
                episode_number
                    .map(|n| format!("Episode {}", n))
                    .unwrap_or_else(|| "Episode".to_string())
            });
            let parent_title = season_number.map(|s| format!("Season {}", s));

            result.push(TraktWatchItem {
                title,
                media_type: "episode".to_string(),
                grandparent_title,
                parent_title,
                season_number,
                episode_number,
                year,
                user_rating,
                genres: None,
                directors: None,
                actors: None,
                guid: None,
                watched_at,
            });
        } else {
            let title = get_val(title_idx).unwrap_or_else(|| "Unknown Movie".to_string());
            result.push(TraktWatchItem {
                title,
                media_type: "movie".to_string(),
                grandparent_title: None,
                parent_title: None,
                season_number: None,
                episode_number: None,
                year,
                user_rating,
                genres: None,
                directors: None,
                actors: None,
                guid: None,
                watched_at,
            });
        }
    }

    Ok(result)
}

fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '"' => {
                if in_quotes && chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = !in_quotes;
                }
            }
            ',' if !in_quotes => {
                fields.push(cur.trim().to_string());
                cur = String::new();
            }
            _ => {
                cur.push(c);
            }
        }
    }
    fields.push(cur.trim().to_string());
    fields
}

/// Normalizes various timestamp formats (ISO8601, RFC3339, YYYY-MM-DD HH:MM:SS, YYYY-MM-DD)
fn normalize_timestamp(raw: &str) -> String {
    let trimmed = raw.trim();

    // 1. Try ISO8601 / RFC3339 parse
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return dt.with_timezone(&Utc).to_rfc3339();
    }

    // 2. Try "%Y-%m-%d %H:%M:%S"
    if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S") {
        return DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc).to_rfc3339();
    }

    // 3. Try "%Y-%m-%dT%H:%M:%S"
    if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S") {
        return DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc).to_rfc3339();
    }

    // 4. Try "%Y-%m-%d" date only -> set to midday 12:00:00Z
    if let Ok(date) = chrono::NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        if let Some(naive) = date.and_hms_opt(12, 0, 0) {
            return DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc).to_rfc3339();
        }
    }

    // Fallback: return as-is
    trimmed.to_string()
}
