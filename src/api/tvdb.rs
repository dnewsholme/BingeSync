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
    pub entity_type: Option<String>,
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

    async fn execute_search_query(
        &self,
        query: &str,
        entity_type: Option<&str>,
        year: Option<i32>,
    ) -> Result<Vec<TvdbSearchCandidate>, String> {
        let token = self.get_token().await?;
        let search_url = format!("{}/search", self.base_url);

        let mut req = self
            .client
            .get(&search_url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/json")
            .query(&[("query", query)]);

        if let Some(t) = entity_type {
            req = req.query(&[("type", t)]);
        }

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
            _ => return Ok(Vec::new()),
        };

        let mut candidates = Vec::new();
        for item in data {
            let raw_id = if let Some(v) = item.get("tvdb_id") {
                v.as_str().map(|s| s.to_string()).or_else(|| v.as_i64().map(|n| n.to_string()))
            } else if let Some(v) = item.get("id") {
                v.as_str().map(|s| s.to_string()).or_else(|| v.as_i64().map(|n| n.to_string()))
            } else if let Some(v) = item.get("objectID") {
                v.as_str().map(|s| s.to_string())
            } else {
                None
            };

            let raw_id = match raw_id {
                Some(s) if !s.trim().is_empty() => s,
                _ => continue,
            };

            let clean_id = raw_id
                .strip_prefix("movie-")
                .or_else(|| raw_id.strip_prefix("series-"))
                .or_else(|| raw_id.strip_prefix("movie_"))
                .or_else(|| raw_id.strip_prefix("series_"))
                .unwrap_or(&raw_id)
                .trim()
                .to_string();

            let detected_type = item
                .get("type")
                .or_else(|| item.get("primary_type"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_lowercase())
                .or_else(|| {
                    if raw_id.starts_with("movie-") || raw_id.starts_with("movie_") {
                        Some("movie".to_string())
                    } else if raw_id.starts_with("series-") || raw_id.starts_with("series_") {
                        Some("series".to_string())
                    } else if let Some(obj) = item.get("objectID").and_then(|v| v.as_str()) {
                        if obj.starts_with("movie-") {
                            Some("movie".to_string())
                        } else if obj.starts_with("series-") {
                            Some("series".to_string())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                });

            let name = item
                .get("name")
                .or_else(|| item.get("title"))
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string());

            let overview = item
                .get("overview")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    item.get("overviews").and_then(|ov| {
                        ov.get("eng")
                            .or_else(|| ov.get("en"))
                            .or_else(|| ov.as_object().and_then(|m| m.values().next()))
                            .and_then(|v| v.as_str())
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                    })
                })
                .or_else(|| {
                    item.get("translations").and_then(|tr| {
                        tr.get("eng")
                            .or_else(|| tr.get("en"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                    })
                });

            let image_url = item
                .get("image_url")
                .or_else(|| item.get("image"))
                .or_else(|| item.get("poster"))
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());

            let item_year = item.get("year").and_then(|v| {
                v.as_i64().map(|n| n as i32).or_else(|| {
                    v.as_str().and_then(|s| s.trim().parse::<i32>().ok())
                })
            });

            let aliases: Vec<String> = item
                .get("aliases")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
                        .collect()
                })
                .unwrap_or_default();

            candidates.push(TvdbSearchCandidate {
                id: clean_id,
                name,
                overview,
                image_url,
                year: item_year,
                entity_type: detected_type,
                aliases,
            });
        }

        Ok(candidates)
    }

    /// Search TVDB by query and entity type ("series" or "movie") with multi-tier fallback
    pub async fn search(
        &self,
        query: &str,
        media_type: &str,
        year: Option<i32>,
    ) -> Result<Option<TvdbSearchResult>, String> {
        let (clean_query, detected_year) = normalize_search_query(query);
        let target_year = year.or(detected_year);
        let primary_entity_type = if media_type == "movie" { "movie" } else { "series" };
        let secondary_entity_type = if primary_entity_type == "movie" { "series" } else { "movie" };

        let mut candidates = Vec::new();

        // Tier 1: Search with cleaned query, primary entity type, and year (if present)
        if let Some(y) = target_year {
            if let Ok(results) = self.execute_search_query(&clean_query, Some(primary_entity_type), Some(y)).await {
                if !results.is_empty() {
                    candidates = results;
                }
            }
        }

        // Tier 2: Search with cleaned query and primary entity type, but WITHOUT year
        if candidates.is_empty() {
            if let Ok(results) = self.execute_search_query(&clean_query, Some(primary_entity_type), None).await {
                if !results.is_empty() {
                    candidates = results;
                }
            }
        }

        // Tier 3: Search with secondary entity type (cross-type fallback e.g. miniseries/movie)
        if candidates.is_empty() {
            if let Ok(results) = self.execute_search_query(&clean_query, Some(secondary_entity_type), None).await {
                if !results.is_empty() {
                    candidates = results;
                }
            }
        }

        // Tier 4: Search without entity type restriction
        if candidates.is_empty() {
            if let Ok(results) = self.execute_search_query(&clean_query, None, None).await {
                if !results.is_empty() {
                    candidates = results;
                }
            }
        }

        if candidates.is_empty() {
            return Ok(None);
        }

        // Score candidates and pick highest scoring result
        candidates.sort_by_key(|c| std::cmp::Reverse(score_candidate(c, &clean_query, target_year)));
        let best = candidates.into_iter().next().unwrap();

        Ok(Some(TvdbSearchResult {
            id: best.id,
            name: best.name,
            overview: best.overview,
            image_url: best.image_url,
            entity_type: best.entity_type,
        }))
    }

    /// Fetch extended series information from /series/{id}/extended with fallback to base /series/{id}
    pub async fn get_series_extended(&self, series_id: &str) -> Result<TvdbMetadata, String> {
        let clean_id = series_id
            .strip_prefix("series-")
            .or_else(|| series_id.strip_prefix("movie-"))
            .unwrap_or(series_id);

        let token = self.get_token().await?;
        let url = format!("{}/series/{}/extended", self.base_url, clean_id);

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/json")
            .send()
            .await;

        if let Ok(r) = resp {
            if r.status().is_success() {
                if let Ok(json_val) = r.json::<serde_json::Value>().await {
                    if let Some(data) = json_val.get("data") {
                        return Ok(parse_extended_metadata(data));
                    }
                }
            }
        }

        // Fallback: try base series endpoint /series/{id}
        let base_url = format!("{}/series/{}", self.base_url, clean_id);
        let resp2 = self
            .client
            .get(&base_url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| format!("Failed to fetch TVDB series base: {}", e))?;

        if resp2.status().is_success() {
            if let Ok(json_val) = resp2.json::<serde_json::Value>().await {
                if let Some(data) = json_val.get("data") {
                    return Ok(parse_extended_metadata(data));
                }
            }
        }

        Err(format!("TVDB series extended failed for ID {}", clean_id))
    }

    /// Fetch extended movie information from /movies/{id}/extended with fallback to base /movies/{id}
    pub async fn get_movie_extended(&self, movie_id: &str) -> Result<TvdbMetadata, String> {
        let clean_id = movie_id
            .strip_prefix("movie-")
            .or_else(|| movie_id.strip_prefix("series-"))
            .unwrap_or(movie_id);

        let token = self.get_token().await?;
        let url = format!("{}/movies/{}/extended", self.base_url, clean_id);

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/json")
            .send()
            .await;

        if let Ok(r) = resp {
            if r.status().is_success() {
                if let Ok(json_val) = r.json::<serde_json::Value>().await {
                    if let Some(data) = json_val.get("data") {
                        return Ok(parse_extended_metadata(data));
                    }
                }
            }
        }

        // Fallback: try base movie endpoint /movies/{id}
        let base_url = format!("{}/movies/{}", self.base_url, clean_id);
        let resp2 = self
            .client
            .get(&base_url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| format!("Failed to fetch TVDB movie base: {}", e))?;

        if resp2.status().is_success() {
            if let Ok(json_val) = resp2.json::<serde_json::Value>().await {
                if let Some(data) = json_val.get("data") {
                    return Ok(parse_extended_metadata(data));
                }
            }
        }

        Err(format!("TVDB movie extended failed for ID {}", clean_id))
    }
}

/// Normalizes search query by extracting embedded years like (1976) and stripping common suffixes
pub fn normalize_search_query(query: &str) -> (String, Option<i32>) {
    let mut cleaned = query.trim().to_string();
    let mut detected_year = None;

    let trimmed = cleaned.trim();
    if (trimmed.ends_with(')') && trimmed.contains('(')) || (trimmed.ends_with(']') && trimmed.contains('[')) {
        let open_char = if trimmed.ends_with(')') { '(' } else { '[' };
        if let Some(idx) = trimmed.rfind(open_char) {
            let inner = trimmed[idx + 1..trimmed.len() - 1].trim();
            if inner.len() == 4 && inner.chars().all(|c| c.is_ascii_digit()) {
                if let Ok(y) = inner.parse::<i32>() {
                    if (1880..=2100).contains(&y) {
                        detected_year = Some(y);
                        cleaned = trimmed[..idx].trim().to_string();
                    }
                }
            }
        }
    }

    let suffixes = [
        " - Extended Cut", " (Extended Cut)", " [Extended Cut]",
        " - Director's Cut", " (Director's Cut)", " [Director's Cut]",
        " - Remastered", " (Remastered)", " [Remastered]",
        " - Unrated", " (Unrated)", " [Unrated]",
    ];
    for s in &suffixes {
        if cleaned.to_lowercase().ends_with(&s.to_lowercase()) {
            let new_len = cleaned.len() - s.len();
            cleaned = cleaned[..new_len].trim().to_string();
        }
    }

    (cleaned, detected_year)
}

#[derive(Debug, Clone)]
struct TvdbSearchCandidate {
    id: String,
    name: Option<String>,
    overview: Option<String>,
    image_url: Option<String>,
    year: Option<i32>,
    entity_type: Option<String>,
    aliases: Vec<String>,
}

fn score_candidate(candidate: &TvdbSearchCandidate, target_title: &str, target_year: Option<i32>) -> i32 {
    let mut score = 0;
    let target_norm = target_title.trim().to_lowercase();

    if let Some(ref name) = candidate.name {
        let name_norm = name.trim().to_lowercase();
        if name_norm == target_norm {
            score += 100;
        } else if name_norm.starts_with(&target_norm) || target_norm.starts_with(&name_norm) {
            score += 50;
        } else if name_norm.contains(&target_norm) {
            score += 30;
        }
    }

    for alias in &candidate.aliases {
        let alias_norm = alias.trim().to_lowercase();
        if alias_norm == target_norm {
            score += 90;
            break;
        }
    }

    if let (Some(cand_year), Some(tgt_year)) = (candidate.year, target_year) {
        let diff = (cand_year - tgt_year).abs();
        if diff == 0 {
            score += 100;
        } else if diff == 1 {
            score += 60;
        } else if diff == 2 {
            score += 30;
        } else {
            score -= (diff * 5).min(50);
        }
    }

    if candidate.overview.is_some() {
        score += 5;
    }
    if candidate.image_url.is_some() {
        score += 5;
    }

    score
}

fn parse_extended_metadata(data: &serde_json::Value) -> TvdbMetadata {
    let overview = data
        .get("overview")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            data.get("overviews").and_then(|ov| {
                ov.get("eng")
                    .or_else(|| ov.get("en"))
                    .or_else(|| ov.as_object().and_then(|m| m.values().next()))
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            })
        })
        .or_else(|| {
            data.get("translations").and_then(|tr| {
                if let Some(arr) = tr.get("overviewTranslations").and_then(|v| v.as_array()) {
                    for item in arr {
                        let lang = item.get("language").and_then(|v| v.as_str()).unwrap_or("");
                        if lang == "eng" || lang == "en" {
                            if let Some(ov) = item.get("overview").and_then(|v| v.as_str()) {
                                if !ov.trim().is_empty() {
                                    return Some(ov.trim().to_string());
                                }
                            }
                        }
                    }
                }
                tr.get("eng")
                    .or_else(|| tr.get("en"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            })
        });

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

    let mut extract_person = |c: &serde_json::Value| {
        let person_name = c
            .get("personName")
            .or_else(|| c.get("peopleName"))
            .or_else(|| c.get("name"))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let role_type = c
            .get("peopleType")
            .or_else(|| c.get("type"))
            .or_else(|| c.get("role"))
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
    };

    if let Some(chars) = data.get("characters").and_then(|v| v.as_array()) {
        for c in chars {
            extract_person(c);
        }
    }
    if let Some(people) = data.get("people").and_then(|v| v.as_array()) {
        for c in people {
            extract_person(c);
        }
    }
    if let Some(crew) = data.get("crew").and_then(|v| v.as_array()) {
        for c in crew {
            extract_person(c);
        }
    }
    if let Some(actors) = data.get("actors").and_then(|v| v.as_array()) {
        for c in actors {
            extract_person(c);
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
        .or_else(|| data.get("poster"))
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

            let interval_hours = state.get_task_interval_hours("tvdb_metadata").await;
            let next_run = (chrono::Utc::now() + chrono::Duration::hours(interval_hours as i64)).to_rfc3339();
            state.task_tracker.set_tvdb_next_run(Some(next_run)).await;

            // Wait for new scrobbles or configurable periodic timer
            tokio::select! {
                _ = state.metadata_notify.notified() => {
                    failed_lookups.clear();
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                _ = tokio::time::sleep(Duration::from_secs(interval_hours * 3600)) => {
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
        let raw_title = if is_episode {
            item.grandparent_title.as_deref().unwrap_or(&item.title)
        } else {
            &item.title
        };

        let (search_title, detected_year) = normalize_search_query(raw_title);
        let effective_year = item.year.or(detected_year);

        let cache_key = format!("{}:{}:{:?}", item.media_type, search_title.to_lowercase(), effective_year);
        if failed_lookups.contains(&cache_key) {
            let _ = sqlx::query("UPDATE scrobbles SET metadata_updated_at = datetime('now') WHERE id = ?")
                .bind(&item.id)
                .execute(&state.pool)
                .await;
            processed_count += 1;
            continue;
        }

        let search_res = client
            .search(&search_title, if is_episode { "series" } else { "movie" }, effective_year)
            .await;

        let match_result = match search_res {
            Ok(Some(res)) => res,
            _ => {
                // Mark metadata_updated_at so we don't query TVDB on every loop for unmatchable items
                if is_episode {
                    let show_name = item.grandparent_title.as_deref().unwrap_or(&item.title);
                    let _ = sqlx::query(
                        "UPDATE scrobbles SET metadata_updated_at = datetime('now') WHERE id = ? OR (grandparent_title IS NOT NULL AND LOWER(TRIM(grandparent_title)) = LOWER(TRIM(?)))"
                    )
                    .bind(&item.id)
                    .bind(show_name)
                    .execute(&state.pool)
                    .await;
                } else {
                    let _ = sqlx::query("UPDATE scrobbles SET metadata_updated_at = datetime('now') WHERE id = ?")
                        .bind(&item.id)
                        .execute(&state.pool)
                        .await;
                }
                failed_lookups.insert(cache_key);
                processed_count += 1;
                continue;
            }
        };

        let match_is_series = match_result.entity_type.as_deref() == Some("series") || is_episode;
        let mut meta = if match_is_series {
            match client.get_series_extended(&match_result.id).await {
                Ok(m) => m,
                Err(_) => client.get_movie_extended(&match_result.id).await.unwrap_or_default(),
            }
        } else {
            match client.get_movie_extended(&match_result.id).await {
                Ok(m) => m,
                Err(_) => client.get_series_extended(&match_result.id).await.unwrap_or_default(),
            }
        };

        if meta.overview.is_none() || meta.overview.as_deref() == Some("") {
            meta.overview = match_result.overview.clone();
        }
        if meta.image_url.is_none() || meta.image_url.as_deref() == Some("") {
            meta.image_url = match_result.image_url.clone();
        }

        if is_episode {
            // Update all episodes for this series that have missing fields
            let show_name = item.grandparent_title.as_deref().unwrap_or(&item.title);
            let _ = sqlx::query(
                r#"
                UPDATE scrobbles
                SET genres = COALESCE(NULLIF(?, ''), NULLIF(genres, '')),
                    actors = COALESCE(NULLIF(?, ''), NULLIF(actors, '')),
                    directors = COALESCE(NULLIF(?, ''), NULLIF(directors, '')),
                    overview = CASE WHEN overview IS NULL OR overview = '' THEN ? ELSE overview END,
                    thumb_url = CASE 
                        WHEN (thumb_url IS NULL OR thumb_url = '' OR thumb_url LIKE '/library/%' OR thumb_url LIKE '/photo/%') AND ? IS NOT NULL 
                        THEN ? 
                        ELSE thumb_url 
                    END,
                    metadata_updated_at = datetime('now')
                WHERE id = ? OR (grandparent_title IS NOT NULL AND LOWER(TRIM(grandparent_title)) = LOWER(TRIM(?))) OR (media_type = 'episode' AND LOWER(TRIM(title)) = LOWER(TRIM(?)))
                "#
            )
            .bind(&meta.genres)
            .bind(&meta.actors)
            .bind(&meta.directors)
            .bind(&meta.overview)
            .bind(&meta.image_url)
            .bind(&meta.image_url)
            .bind(&item.id)
            .bind(show_name)
            .bind(show_name)
            .execute(&state.pool)
            .await;
        } else {
            // Update movie
            let _ = sqlx::query(
                r#"
                UPDATE scrobbles
                SET genres = COALESCE(NULLIF(?, ''), NULLIF(genres, '')),
                    actors = COALESCE(NULLIF(?, ''), NULLIF(actors, '')),
                    directors = COALESCE(NULLIF(?, ''), NULLIF(directors, '')),
                    overview = CASE WHEN overview IS NULL OR overview = '' THEN ? ELSE overview END,
                    thumb_url = CASE 
                        WHEN (thumb_url IS NULL OR thumb_url = '' OR thumb_url LIKE '/library/%' OR thumb_url LIKE '/photo/%') AND ? IS NOT NULL 
                        THEN ? 
                        ELSE thumb_url 
                    END,
                    metadata_updated_at = datetime('now')
                WHERE id = ?
                "#
            )
            .bind(&meta.genres)
            .bind(&meta.actors)
            .bind(&meta.directors)
            .bind(&meta.overview)
            .bind(&meta.image_url)
            .bind(&meta.image_url)
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
