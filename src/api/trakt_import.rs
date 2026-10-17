use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
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
struct TraktWatchItem {
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
    pub watched_at: String,
}

pub async fn import_trakt_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    _headers: HeaderMap,
    body_bytes: axum::body::Bytes,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let body_str = String::from_utf8(body_bytes.to_vec()).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("Invalid UTF-8 payload: {}", e) })),
        )
    })?;

    let trimmed = body_str.trim();
    if trimmed.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Import payload is empty" })),
        ));
    }

    let items = if trimmed.starts_with('[') || trimmed.starts_with('{') {
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

    let mut imported_count = 0;
    let mut updated_count = 0;
    let mut skipped_count = 0;
    let now = Utc::now().to_rfc3339();

    for item in &items {
        // Check for existing record to preserve history and update rating if newly provided
        let existing: Option<(String, Option<i32>)> = if item.media_type == "episode" {
            sqlx::query_as(
                r#"
                SELECT id, user_rating FROM scrobbles
                WHERE user_id = ? AND media_type = 'episode'
                  AND grandparent_title = ? AND season_number = ? AND episode_number = ?
                  AND watched_at = ?
                LIMIT 1
                "#,
            )
            .bind(&auth.user_id)
            .bind(&item.grandparent_title)
            .bind(item.season_number)
            .bind(item.episode_number)
            .bind(&item.watched_at)
            .fetch_optional(&state.pool)
            .await
            .unwrap_or(None)
        } else {
            sqlx::query_as(
                r#"
                SELECT id, user_rating FROM scrobbles
                WHERE user_id = ? AND media_type = 'movie'
                  AND title = ? AND watched_at = ?
                LIMIT 1
                "#,
            )
            .bind(&auth.user_id)
            .bind(&item.title)
            .bind(&item.watched_at)
            .fetch_optional(&state.pool)
            .await
            .unwrap_or(None)
        };

        if let Some((existing_id, existing_rating)) = existing {
            // If item exists but new rating is provided, update rating
            if item.user_rating.is_some() && existing_rating != item.user_rating {
                let _ = sqlx::query("UPDATE scrobbles SET user_rating = ? WHERE id = ?")
                    .bind(item.user_rating)
                    .bind(&existing_id)
                    .execute(&state.pool)
                    .await;
                updated_count += 1;
            } else {
                skipped_count += 1;
            }
            continue;
        }

        // Insert new record with exact Trakt watched_at timestamp and rating
        let scrobble_id = Uuid::new_v4().to_string();
        let insert_res = sqlx::query(
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
        .bind(None::<String>) // guid
        .bind(None::<i64>)    // duration_ms
        .bind(item.user_rating)
        .bind(&item.genres)
        .bind(&item.directors)
        .bind(&item.actors)
        .bind("completed")
        .bind(&item.watched_at)
        .bind(&now)
        .execute(&state.pool)
        .await;

        if insert_res.is_ok() {
            imported_count += 1;
        } else {
            skipped_count += 1;
        }
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

fn parse_trakt_json(json_str: &str) -> Result<Vec<TraktWatchItem>, String> {
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

        // Parse rating
        let user_rating = item
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
                watched_at,
            });
        } else {
            // Movie
            let movie_obj = item.get("movie");
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
                watched_at,
            });
        }
    }

    Ok(result)
}

fn parse_trakt_csv(csv_str: &str) -> Result<Vec<TraktWatchItem>, String> {
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
