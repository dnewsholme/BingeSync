use reqwest::Client;
use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;

use crate::api::AppState;

#[derive(Debug, Clone, Default)]
pub struct TvdbMetadata {
    pub overview: Option<String>,
    pub genres: Option<String>,
    pub actors: Option<String>,
    pub directors: Option<String>,
    pub image_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TvdbSearchResult {
    pub id: String,
    pub name: Option<String>,
    pub overview: Option<String>,
    pub image_url: Option<String>,
}

#[derive(Clone)]
pub struct TvdbClient {
    client: Client,
    base_url: String,
    api_key: String,
    pin: Option<String>,
    token: Arc<RwLock<Option<(String, Instant)>>>,
}

impl TvdbClient {
    pub fn new(base_url: String, api_key: String, pin: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_default();
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            pin,
            token: Arc::new(RwLock::new(None)),
        }
    }

    /// Obtain or refresh JWT Bearer token via /login
    pub async fn get_token(&self) -> Result<String, String> {
        // 1. Check cached token (valid up to 25 days)
        {
            let guard = self.token.read().await;
            if let Some((ref tok, exp)) = *guard {
                if exp.elapsed() < Duration::from_secs(25 * 86400) {
                    return Ok(tok.clone());
                }
            }
        }

        // 2. Fetch new token from /login
        let mut write_guard = self.token.write().await;
        if let Some((ref tok, exp)) = *write_guard {
            if exp.elapsed() < Duration::from_secs(25 * 86400) {
                return Ok(tok.clone());
            }
        }

        let login_url = format!("{}/login", self.base_url);
        let mut login_body = serde_json::json!({
            "apikey": self.api_key
        });
        if let Some(ref pin) = self.pin {
            login_body["pin"] = serde_json::json!(pin);
        }

        let resp = self
            .client
            .post(&login_url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&login_body)
            .send()
            .await
            .map_err(|e| format!("Failed to send login request to TVDB: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("TVDB login failed with status {}", resp.status()));
        }

        let json_val: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse TVDB login response: {}", e))?;

        let token = json_val
            .get("data")
            .and_then(|d| d.get("token"))
            .and_then(|t| t.as_str())
            .ok_or_else(|| "TVDB login response missing data.token".to_string())?
            .to_string();

        *write_guard = Some((token.clone(), Instant::now()));
        Ok(token)
    }

    /// Search TVDB by query and entity type ("series" or "movie")
    pub async fn search(
        &self,
        query: &str,
        media_type: &str,
        year: Option<i32>,
    ) -> Result<Option<TvdbSearchResult>, String> {
        let token = self.get_token().await?;
        let search_url = format!("{}/search", self.base_url);

        let entity_type = if media_type == "movie" {
            "movie"
        } else {
            "series"
        };

        let mut req = self
            .client
            .get(&search_url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/json")
            .query(&[("query", query), ("type", entity_type)]);

        let year_str;
        if let Some(y) = year {
            year_str = y.to_string();
            req = req.query(&[("year", &year_str)]);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| format!("Failed to search TVDB: {}", e))?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            // Token expired, clear and retry once
            let mut guard = self.token.write().await;
            *guard = None;
            drop(guard);
            return Err("TVDB token expired or unauthorized".to_string());
        }

        if !resp.status().is_success() {
            return Err(format!("TVDB search returned status {}", resp.status()));
        }

        let json_val: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse TVDB search JSON: {}", e))?;

        let data = match json_val.get("data").and_then(|d| d.as_array()) {
            Some(arr) if !arr.is_empty() => arr,
            _ => return Ok(None),
        };

        // Pick best matching result
        for item in data {
            let id = if let Some(tvdb_id) = item.get("tvdb_id").and_then(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .or_else(|| v.as_i64().map(|n| n.to_string()))
            }) {
                tvdb_id
            } else if let Some(id_val) = item.get("id").and_then(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .or_else(|| v.as_i64().map(|n| n.to_string()))
            }) {
                id_val
            } else if let Some(obj_id) = item.get("objectID").and_then(|v| v.as_str()) {
                obj_id
                    .strip_prefix("series-")
                    .or_else(|| obj_id.strip_prefix("movie-"))
                    .unwrap_or(obj_id)
                    .to_string()
            } else {
                continue;
            };

            let name = item
                .get("name")
                .or_else(|| item.get("title"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let overview = item
                .get("overview")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let image_url = item
                .get("image_url")
                .or_else(|| item.get("image"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            return Ok(Some(TvdbSearchResult {
                id,
                name,
                overview,
                image_url,
            }));
        }

        Ok(None)
    }

    /// Fetch extended series information from /series/{id}/extended
    pub async fn get_series_extended(&self, series_id: &str) -> Result<TvdbMetadata, String> {
        let token = self.get_token().await?;
        let url = format!("{}/series/{}/extended", self.base_url, series_id);

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| format!("Failed to fetch TVDB series extended: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("TVDB series extended status: {}", resp.status()));
        }

        let json_val: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse TVDB series JSON: {}", e))?;

        let data = json_val
            .get("data")
            .ok_or_else(|| "Missing data in TVDB series response".to_string())?;

        Ok(parse_extended_metadata(data))
    }

    /// Fetch extended movie information from /movies/{id}/extended
    pub async fn get_movie_extended(&self, movie_id: &str) -> Result<TvdbMetadata, String> {
        let token = self.get_token().await?;
        let url = format!("{}/movies/{}/extended", self.base_url, movie_id);

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| format!("Failed to fetch TVDB movie extended: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("TVDB movie extended status: {}", resp.status()));
        }

        let json_val: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse TVDB movie JSON: {}", e))?;

        let data = json_val
            .get("data")
            .ok_or_else(|| "Missing data in TVDB movie response".to_string())?;

        Ok(parse_extended_metadata(data))
    }
}

fn parse_extended_metadata(data: &serde_json::Value) -> TvdbMetadata {
    let overview = data
        .get("overview")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let genres = data
        .get("genres")
        .and_then(|v| v.as_array())
        .map(|arr| {
            let names: Vec<String> = arr
                .iter()
                .filter_map(|item| {
                    if let Some(s) = item.as_str() {
                        Some(s.trim().to_string())
                    } else if let Some(n) = item.get("name").and_then(|v| v.as_str()) {
                        Some(n.trim().to_string())
                    } else {
                        None
                    }
                })
                .collect();
            names.join(", ")
        })
        .filter(|s| !s.is_empty());

    let mut actors_list: Vec<String> = Vec::new();
    let mut directors_list: Vec<String> = Vec::new();

    if let Some(chars) = data.get("characters").and_then(|v| v.as_array()) {
        for c in chars {
            let person_name = c
                .get("personName")
                .or_else(|| c.get("peopleName"))
                .or_else(|| c.get("name"))
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());

            let role_type = c
                .get("peopleType")
                .and_then(|v| v.as_str())
                .map(|s| s.to_lowercase())
                .unwrap_or_else(|| "actor".to_string());

            if let Some(name) = person_name {
                if role_type.contains("director") || role_type.contains("creator") {
                    if !directors_list.contains(&name) {
                        directors_list.push(name);
                    }
                } else if role_type.contains("actor")
                    || role_type.contains("cast")
                    || role_type.contains("voice")
                    || role_type.contains("guest")
                {
                    if !actors_list.contains(&name) && actors_list.len() < 12 {
                        actors_list.push(name);
                    }
                } else if actors_list.len() < 12 && !actors_list.contains(&name) {
                    actors_list.push(name);
                }
            }
        }
    }

    let actors = if actors_list.is_empty() {
        None
    } else {
        Some(actors_list.join(", "))
    };

    let directors = if directors_list.is_empty() {
        None
    } else {
        Some(directors_list.join(", "))
    };

    let image_url = data
        .get("image")
        .or_else(|| data.get("image_url"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    TvdbMetadata {
        overview,
        genres,
        actors,
        directors,
        image_url,
    }
}

/// Spawns the background worker for metadata enrichment via TheTVDB.
pub fn start_metadata_worker(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut failed_lookups: HashSet<String> = HashSet::new();

        loop {
            let api_key_opt = state.get_effective_tvdb_api_key().await;
            if let Some(api_key) = api_key_opt {
                if !api_key.trim().is_empty() {
                    let pin = state.get_effective_tvdb_pin().await;
                    let base_url = state.get_effective_tvdb_base_url().await;
                    let tvdb_client = TvdbClient::new(base_url, api_key, pin);

                    state.task_tracker.set_tvdb_running("Searching TheTVDB for missing metadata...").await;
                    let processed = process_missing_metadata_batch(&state, &tvdb_client, &mut failed_lookups).await;
                    state.task_tracker.set_tvdb_idle(processed, &format!("Processed {} item(s)", processed)).await;

                    if processed > 0 {
                        // Throttle slightly between batches to respect TVDB limits
                        tokio::time::sleep(Duration::from_millis(400)).await;
                        continue;
                    }
                } else {
                    state.task_tracker.set_tvdb_idle(0, "TVDB API key not configured").await;
                }
            } else {
                state.task_tracker.set_tvdb_idle(0, "TVDB API key not configured").await;
            }

            // Wait for new scrobbles or 4-hour periodic timer
            tokio::select! {
                _ = state.metadata_notify.notified() => {
                    failed_lookups.clear();
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                _ = tokio::time::sleep(Duration::from_secs(4 * 3600)) => {
                    failed_lookups.clear();
                }
            }
        }
    })
}

#[derive(sqlx::FromRow, Debug)]
#[allow(dead_code)]
struct MissingMetadataItem {
    id: String,
    title: String,
    media_type: String,
    grandparent_title: Option<String>,
    year: Option<i32>,
    overview: Option<String>,
    genres: Option<String>,
    actors: Option<String>,
    directors: Option<String>,
    thumb_url: Option<String>,
}

/// Process a single batch of scrobbles missing genres, actors, or plot overview.
pub async fn process_missing_metadata_batch(
    state: &AppState,
    client: &TvdbClient,
    failed_lookups: &mut HashSet<String>,
) -> usize {
    let rows: Vec<MissingMetadataItem> = match sqlx::query_as(
        r#"
        SELECT id, title, media_type, grandparent_title, year, overview, genres, actors, directors, thumb_url
        FROM scrobbles
        WHERE metadata_updated_at IS NULL
          AND (
            (genres IS NULL OR genres = '')
            OR (actors IS NULL OR actors = '')
            OR (overview IS NULL OR overview = '')
          )
        ORDER BY watched_at DESC
        LIMIT 10
        "#
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return 0,
    };

    if rows.is_empty() {
        return 0;
    }

    let mut processed_count = 0;

    for item in rows {
        let is_episode = item.media_type == "episode";
        let search_title = if is_episode {
            item.grandparent_title.as_deref().unwrap_or(&item.title)
        } else {
            &item.title
        };

        let cache_key = format!("{}:{}:{:?}", item.media_type, search_title.to_lowercase(), item.year);
        if failed_lookups.contains(&cache_key) {
            continue;
        }

        let search_res = client
            .search(search_title, if is_episode { "series" } else { "movie" }, item.year)
            .await;

        let match_result = match search_res {
            Ok(Some(res)) => res,
            _ => {
                // Mark metadata_updated_at so we don't query TVDB on every loop for unmatchable items
                let _ = sqlx::query("UPDATE scrobbles SET metadata_updated_at = datetime('now') WHERE id = ?")
                    .bind(&item.id)
                    .execute(&state.pool)
                    .await;
                failed_lookups.insert(cache_key);
                continue;
            }
        };

        let metadata_res = if is_episode {
            client.get_series_extended(&match_result.id).await
        } else {
            client.get_movie_extended(&match_result.id).await
        };

        let meta = match metadata_res {
            Ok(m) => m,
            Err(_) => {
                // Fallback to overview or image from search result if available
                TvdbMetadata {
                    overview: match_result.overview,
                    genres: None,
                    actors: None,
                    directors: None,
                    image_url: match_result.image_url,
                }
            }
        };

        if is_episode {
            // Update all episodes for this series that have missing fields
            let show_name = item.grandparent_title.as_deref().unwrap_or(&item.title);
            let _ = sqlx::query(
                r#"
                UPDATE scrobbles
                SET genres = COALESCE(NULLIF(genres, ''), ?),
                    actors = COALESCE(NULLIF(actors, ''), ?),
                    directors = COALESCE(NULLIF(directors, ''), ?),
                    overview = CASE WHEN overview IS NULL OR overview = '' THEN ? ELSE overview END,
                    metadata_updated_at = datetime('now')
                WHERE grandparent_title = ? OR (media_type = 'episode' AND title = ?)
                "#
            )
            .bind(&meta.genres)
            .bind(&meta.actors)
            .bind(&meta.directors)
            .bind(&meta.overview)
            .bind(show_name)
            .bind(show_name)
            .execute(&state.pool)
            .await;
        } else {
            // Update movie
            let _ = sqlx::query(
                r#"
                UPDATE scrobbles
                SET genres = COALESCE(NULLIF(genres, ''), ?),
                    actors = COALESCE(NULLIF(actors, ''), ?),
                    directors = COALESCE(NULLIF(directors, ''), ?),
                    overview = CASE WHEN overview IS NULL OR overview = '' THEN ? ELSE overview END,
                    metadata_updated_at = datetime('now')
                WHERE id = ?
                "#
            )
            .bind(&meta.genres)
            .bind(&meta.actors)
            .bind(&meta.directors)
            .bind(&meta.overview)
            .bind(&item.id)
            .execute(&state.pool)
            .await;
        }

        // If scrobble had no poster (or relative internal path) and TVDB returned an image, update or wake image cache worker
        let has_valid_thumb = item.thumb_url.as_deref().map(|u| !u.is_empty() && !u.starts_with("/library/")).unwrap_or(false);
        if !has_valid_thumb && meta.image_url.is_some() {
            state.image_notify.notify_waiters();
        }

        processed_count += 1;
    }

    processed_count
}

/// Helper to trigger metadata processing once (e.g. for testing)
pub async fn process_missing_metadata_once(state: &AppState) -> usize {
    let api_key = match state.get_effective_tvdb_api_key().await {
        Some(k) if !k.trim().is_empty() => k,
        _ => return 0,
    };

    let base_url = state.get_effective_tvdb_base_url().await;
    let pin = state.get_effective_tvdb_pin().await;

    let client = TvdbClient::new(
        base_url,
        api_key,
        pin,
    );
    let mut failed = HashSet::new();
    process_missing_metadata_batch(state, &client, &mut failed).await
}
