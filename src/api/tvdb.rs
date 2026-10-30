use reqwest::Client;
use serde::{Deserialize, Serialize};
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
    pub director: Option<String>,
    pub cinematographer: Option<String>,
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TvdbSearchResult {
    pub id: String,
    pub name: Option<String>,
    pub overview: Option<String>,
    pub image_url: Option<String>,
    pub entity_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
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
        tracing::info!(base_url = %self.base_url, "Authenticating with TheTVDB API");
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
            .map_err(|e| {
                tracing::warn!(error = %e, "Failed to send login request to TVDB");
                format!("Failed to send login request to TVDB: {}", e)
            })?;

        if !resp.status().is_success() {
            tracing::warn!(status = %resp.status(), "TVDB login failed");
            return Err(format!("TVDB login failed with status {}", resp.status()));
        }

        let json_val: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, "Failed to parse TVDB login response");
                format!("Failed to parse TVDB login response: {}", e)
            })?;

        let token = json_val
            .get("data")
            .and_then(|d| d.get("token"))
            .and_then(|t| t.as_str())
            .ok_or_else(|| "TVDB login response missing data.token".to_string())?
            .to_string();

        *write_guard = Some((token.clone(), Instant::now()));
        tracing::info!("Successfully obtained authentication token from TheTVDB");
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
        tracing::info!(query = %query, entity_type = ?entity_type, year = ?year, "Querying TheTVDB search API");

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
            .map_err(|e| {
                tracing::warn!(query = %query, error = %e, "Failed to send search request to TVDB");
                format!("Failed to search TVDB: {}", e)
            })?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            let mut guard = self.token.write().await;
            *guard = None;
            drop(guard);
            tracing::warn!(query = %query, "TVDB token expired or unauthorized during search");
            return Err("TVDB token expired or unauthorized".to_string());
        }

        if !resp.status().is_success() {
            tracing::warn!(query = %query, status = %resp.status(), "TVDB search returned error status");
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

        tracing::info!(query = %query, results_count = candidates.len(), "Received TheTVDB search results");
        Ok(candidates)
    }

    /// Search TVDB returning a ranked list of candidates
    pub async fn search_candidates(
        &self,
        query: &str,
        media_type: &str,
        year: Option<i32>,
    ) -> Result<Vec<TvdbSearchResult>, String> {
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
            return Ok(Vec::new());
        }

        // Score candidates and rank
        candidates.sort_by_key(|c| std::cmp::Reverse(score_candidate(c, &clean_query, target_year)));

        Ok(candidates
            .into_iter()
            .take(10)
            .map(|c| TvdbSearchResult {
                id: c.id,
                name: c.name,
                overview: c.overview,
                image_url: c.image_url,
                entity_type: c.entity_type,
                year: c.year,
                provider: Some("tvdb".to_string()),
            })
            .collect())
    }

    /// Search TVDB by query and entity type ("series" or "movie") with multi-tier fallback
    pub async fn search(
        &self,
        query: &str,
        media_type: &str,
        year: Option<i32>,
    ) -> Result<Option<TvdbSearchResult>, String> {
        let results = self.search_candidates(query, media_type, year).await?;
        Ok(results.into_iter().next())
    }

    /// Fetch extended series information from /series/{id}/extended with fallback to base /series/{id}
    pub async fn get_series_extended(&self, series_id: &str) -> Result<TvdbMetadata, String> {
        let clean_id = series_id
            .strip_prefix("series-")
            .or_else(|| series_id.strip_prefix("movie-"))
            .unwrap_or(series_id);

        tracing::info!(series_id = %clean_id, "Fetching series extended metadata from TheTVDB");
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
                        tracing::info!(series_id = %clean_id, "Successfully fetched series extended metadata from TheTVDB");
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
            .map_err(|e| {
                tracing::warn!(series_id = %clean_id, error = %e, "Failed to fetch TVDB series base");
                format!("Failed to fetch TVDB series base: {}", e)
            })?;

        if resp2.status().is_success() {
            if let Ok(json_val) = resp2.json::<serde_json::Value>().await {
                if let Some(data) = json_val.get("data") {
                    tracing::info!(series_id = %clean_id, "Successfully fetched series base metadata from TheTVDB");
                    return Ok(parse_extended_metadata(data));
                }
            }
        }

        tracing::warn!(series_id = %clean_id, "TVDB series metadata lookup failed");
        Err(format!("TVDB series extended failed for ID {}", clean_id))
    }

    /// Fetch extended movie information from /movies/{id}/extended with fallback to base /movies/{id}
    pub async fn get_movie_extended(&self, movie_id: &str) -> Result<TvdbMetadata, String> {
        let clean_id = movie_id
            .strip_prefix("movie-")
            .or_else(|| movie_id.strip_prefix("series-"))
            .unwrap_or(movie_id);

        tracing::info!(movie_id = %clean_id, "Fetching movie extended metadata from TheTVDB");
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
                        tracing::info!(movie_id = %clean_id, "Successfully fetched movie extended metadata from TheTVDB");
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
            .map_err(|e| {
                tracing::warn!(movie_id = %clean_id, error = %e, "Failed to fetch TVDB movie base");
                format!("Failed to fetch TVDB movie base: {}", e)
            })?;

        if resp2.status().is_success() {
            if let Ok(json_val) = resp2.json::<serde_json::Value>().await {
                if let Some(data) = json_val.get("data") {
                    tracing::info!(movie_id = %clean_id, "Successfully fetched movie base metadata from TheTVDB");
                    return Ok(parse_extended_metadata(data));
                }
            }
        }

        tracing::warn!(movie_id = %clean_id, "TVDB movie metadata lookup failed");
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
    let mut cinematographers_list: Vec<String> = Vec::new();

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
            let role_clean = role_type.trim().to_lowercase();
            let is_cinematographer = (role_clean == "cinematographer"
                || role_clean == "director of photography"
                || role_clean == "cinematography"
                || role_clean == "dop"
                || (role_clean.contains("director of photography") && !role_clean.contains("assistant"))
                || (role_clean.contains("cinematograph") && !role_clean.contains("assistant") && !role_clean.contains("associate")))
                && !role_clean.contains("operator")
                && !role_clean.contains("assistant")
                && !role_clean.contains("technician")
                && !role_clean.contains("loader")
                && !role_clean.contains("grip")
                && !role_clean.contains("photographer")
                && !role_clean.contains("puller")
                && !role_clean.contains("trainee")
                && !role_clean.contains("electrician")
                && !role_clean.contains("gaffer")
                && !role_clean.contains("clapper")
                && !role_clean.contains("drone")
                && !role_clean.contains("aerial")
                && !role_clean.contains("underwater");

            if is_cinematographer {
                if !cinematographers_list.contains(&name) && cinematographers_list.len() < 3 {
                    cinematographers_list.push(name.clone());
                }
            } else if (role_clean == "director"
                || role_clean == "co-director"
                || role_clean == "series director"
                || role_clean == "creator"
                || (role_clean.contains("director")
                    && !role_clean.contains("photography")
                    && !role_clean.contains("assistant")
                    && !role_clean.contains("associate")
                    && !role_clean.contains("casting")
                    && !role_clean.contains("art")
                    && !role_clean.contains("technical")
                    && !role_clean.contains("lighting")
                    && !role_clean.contains("second unit")))
                && !role_clean.contains("assistant")
                && !role_clean.contains("associate")
            {
                if !directors_list.contains(&name) && directors_list.len() < 5 {
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

    let director = directors_list.first().cloned();

    let cinematographer = if cinematographers_list.is_empty() {
        None
    } else {
        Some(cinematographers_list.join(", "))
    };

    let mut image_url = data
        .get("image")
        .or_else(|| data.get("image_url"))
        .or_else(|| data.get("poster"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if image_url.is_none() {
        if let Some(artworks) = data.get("artworks").and_then(|v| v.as_array()) {
            for art in artworks {
                let art_type = art.get("type").and_then(|v| v.as_i64()).unwrap_or(0);
                if art_type == 2 || art_type == 14 || art_type == 3 {
                    if let Some(img) = art.get("image").and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()) {
                        image_url = Some(img.trim().to_string());
                        break;
                    }
                }
            }
            if image_url.is_none() {
                for art in artworks {
                    if let Some(img) = art.get("image").and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()) {
                        image_url = Some(img.trim().to_string());
                        break;
                    }
                }
            }
        }
    }

    TvdbMetadata {
        overview,
        genres,
        actors,
        directors,
        director,
        cinematographer,
        image_url,
    }
}

pub enum MetadataProvider<'a> {
    Tvdb(&'a TvdbClient),
    Tmdb(&'a crate::api::tmdb::TmdbClient),
}

impl<'a> MetadataProvider<'a> {
    pub async fn search_candidates(
        &self,
        query: &str,
        media_type: &str,
        year: Option<i32>,
    ) -> Result<Vec<TvdbSearchResult>, String> {
        match self {
            MetadataProvider::Tvdb(c) => c.search_candidates(query, media_type, year).await,
            MetadataProvider::Tmdb(c) => c.search_candidates(query, media_type, year).await,
        }
    }

    pub async fn search(
        &self,
        query: &str,
        media_type: &str,
        year: Option<i32>,
    ) -> Result<Option<TvdbSearchResult>, String> {
        match self {
            MetadataProvider::Tvdb(c) => c.search(query, media_type, year).await,
            MetadataProvider::Tmdb(c) => c.search(query, media_type, year).await,
        }
    }

    pub async fn get_series_extended(&self, id: &str) -> Result<TvdbMetadata, String> {
        match self {
            MetadataProvider::Tvdb(c) => c.get_series_extended(id).await,
            MetadataProvider::Tmdb(c) => c.get_series_details(id).await,
        }
    }

    pub async fn get_movie_extended(&self, id: &str) -> Result<TvdbMetadata, String> {
        match self {
            MetadataProvider::Tvdb(c) => c.get_movie_extended(id).await,
            MetadataProvider::Tmdb(c) => c.get_movie_details(id).await,
        }
    }
}

pub enum MetadataProviderOwned {
    Tvdb(TvdbClient),
    Tmdb(crate::api::tmdb::TmdbClient),
}

impl MetadataProviderOwned {
    pub fn as_ref(&self) -> MetadataProvider<'_> {
        match self {
            MetadataProviderOwned::Tvdb(ref c) => MetadataProvider::Tvdb(c),
            MetadataProviderOwned::Tmdb(ref c) => MetadataProvider::Tmdb(c),
        }
    }
}

/// Spawns the background worker for metadata enrichment via TheTVDB or TMDB.
pub fn start_metadata_worker(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut failed_lookups: HashSet<String> = HashSet::new();

        loop {
            let metadata_source = state.get_effective_metadata_source().await;
            let (provider_opt, provider_name) = if metadata_source == "tmdb" {
                let tmdb_key = state.get_effective_tmdb_api_key().await;
                if let Some(key) = tmdb_key.filter(|k| !k.trim().is_empty()) {
                    let base_url = state.get_effective_tmdb_base_url().await;
                    let client = crate::api::tmdb::TmdbClient::new(base_url, key);
                    (Some(MetadataProviderOwned::Tmdb(client)), "The Movie Database (TMDB)")
                } else {
                    (None, "The Movie Database (TMDB)")
                }
            } else {
                let tvdb_key = state.get_effective_tvdb_api_key().await;
                if let Some(key) = tvdb_key.filter(|k| !k.trim().is_empty()) {
                    let pin = state.get_effective_tvdb_pin().await;
                    let base_url = state.get_effective_tvdb_base_url().await;
                    let client = TvdbClient::new(base_url, key, pin);
                    (Some(MetadataProviderOwned::Tvdb(client)), "TheTVDB")
                } else {
                    (None, "TheTVDB")
                }
            };

            if let Some(provider) = provider_opt {
                state.task_tracker.set_tvdb_running(Some(&state.pool), &format!("Searching {} for missing metadata...", provider_name)).await;
                let processed = process_missing_metadata_batch_provider(&state, provider.as_ref(), &mut failed_lookups).await;
                state.task_tracker.set_tvdb_idle(Some(&state.pool), processed, &format!("Processed {} item(s)", processed)).await;

                if processed > 0 {
                    // Throttle slightly between batches to respect API limits
                    tokio::time::sleep(Duration::from_millis(400)).await;
                    continue;
                }
            } else {
                state.task_tracker.set_tvdb_idle_unconfigured(&format!("{} API key not configured", provider_name)).await;
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
    tvdb_id: Option<String>,
    tmdb_id: Option<String>,
}

/// Process a single batch of scrobbles requiring metadata enrichment with TvdbClient (for backward compatibility).
pub async fn process_missing_metadata_batch(
    state: &AppState,
    client: &TvdbClient,
    failed_lookups: &mut HashSet<String>,
) -> usize {
    process_missing_metadata_batch_provider(state, MetadataProvider::Tvdb(client), failed_lookups).await
}

/// Process a single batch of scrobbles requiring metadata enrichment with any MetadataProvider.
pub async fn process_missing_metadata_batch_provider(
    state: &AppState,
    client: MetadataProvider<'_>,
    failed_lookups: &mut HashSet<String>,
) -> usize {
    let rows: Vec<MissingMetadataItem> = match sqlx::query_as(
        r#"
        SELECT id, title, media_type, grandparent_title, year, overview, genres, actors, directors, thumb_url, tvdb_id, tmdb_id
        FROM scrobbles
        WHERE metadata_updated_at IS NULL
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

    tracing::info!(batch_size = rows.len(), "Processing batch for metadata enrichment");

    let is_tmdb = matches!(client, MetadataProvider::Tmdb(_));
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

        let known_id = if is_tmdb {
            item.tmdb_id.as_deref()
        } else {
            item.tvdb_id.as_deref()
        };

        let mut matched_id: Option<String> = None;

        let meta = if let Some(clean_tid) = known_id.map(|s| s.trim()).filter(|s| !s.is_empty()) {
            if is_episode {
                match client.get_series_extended(clean_tid).await {
                    Ok(m) => m,
                    Err(_) => client.get_movie_extended(clean_tid).await.unwrap_or_default(),
                }
            } else {
                match client.get_movie_extended(clean_tid).await {
                    Ok(m) => m,
                    Err(_) => client.get_series_extended(clean_tid).await.unwrap_or_default(),
                }
            }
        } else {
            let provider_prefix = if is_tmdb { "tmdb" } else { "tvdb" };
            let cache_key = format!("{}:{}:{}:{:?}", provider_prefix, item.media_type, search_title.to_lowercase(), effective_year);
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
                    // Mark metadata_updated_at so we don't query repeatedly on every loop for unmatchable items
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

            matched_id = Some(match_result.id.clone());
            let match_is_series = match_result.entity_type.as_deref() == Some("series") || is_episode;
            let mut m = if match_is_series {
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

            if m.overview.is_none() || m.overview.as_deref() == Some("") {
                m.overview = match_result.overview.clone();
            }
            if m.image_url.is_none() || m.image_url.as_deref() == Some("") {
                m.image_url = match_result.image_url.clone();
            }
            m
        };

        if is_episode {
            // Update all episodes for this series that have missing fields
            let show_name = item.grandparent_title.as_deref().unwrap_or(&item.title);
            if is_tmdb {
                let _ = sqlx::query(
                    r#"
                    UPDATE scrobbles
                    SET genres = COALESCE(NULLIF(?, ''), NULLIF(genres, '')),
                        actors = COALESCE(NULLIF(?, ''), NULLIF(actors, '')),
                        directors = COALESCE(NULLIF(?, ''), NULLIF(directors, '')),
                        director = COALESCE(NULLIF(?, ''), NULLIF(director, '')),
                        cinematographer = COALESCE(NULLIF(?, ''), NULLIF(cinematographer, '')),
                        overview = CASE WHEN overview IS NULL OR overview = '' THEN ? ELSE overview END,
                        thumb_url = CASE 
                            WHEN (thumb_url IS NULL OR thumb_url = '' OR thumb_url LIKE '/library/%' OR thumb_url LIKE '/photo/%') AND ? IS NOT NULL 
                            THEN ? 
                            ELSE thumb_url 
                        END,
                        tmdb_id = COALESCE(NULLIF(?, ''), NULLIF(tmdb_id, '')),
                        metadata_updated_at = datetime('now')
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
                .bind(&meta.image_url)
                .bind(&matched_id)
                .bind(&item.id)
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
                        overview = CASE WHEN overview IS NULL OR overview = '' THEN ? ELSE overview END,
                        thumb_url = CASE 
                            WHEN (thumb_url IS NULL OR thumb_url = '' OR thumb_url LIKE '/library/%' OR thumb_url LIKE '/photo/%') AND ? IS NOT NULL 
                            THEN ? 
                            ELSE thumb_url 
                        END,
                        tvdb_id = COALESCE(NULLIF(?, ''), NULLIF(tvdb_id, '')),
                        metadata_updated_at = datetime('now')
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
                .bind(&meta.image_url)
                .bind(&matched_id)
                .bind(&item.id)
                .bind(show_name)
                .bind(show_name)
                .execute(&state.pool)
                .await;
            }
        } else {
            // Update movie
            if is_tmdb {
                let _ = sqlx::query(
                    r#"
                    UPDATE scrobbles
                    SET genres = COALESCE(NULLIF(?, ''), NULLIF(genres, '')),
                        actors = COALESCE(NULLIF(?, ''), NULLIF(actors, '')),
                        directors = COALESCE(NULLIF(?, ''), NULLIF(directors, '')),
                        director = COALESCE(NULLIF(?, ''), NULLIF(director, '')),
                        cinematographer = COALESCE(NULLIF(?, ''), NULLIF(cinematographer, '')),
                        overview = CASE WHEN overview IS NULL OR overview = '' THEN ? ELSE overview END,
                        thumb_url = CASE 
                            WHEN (thumb_url IS NULL OR thumb_url = '' OR thumb_url LIKE '/library/%' OR thumb_url LIKE '/photo/%') AND ? IS NOT NULL 
                            THEN ? 
                            ELSE thumb_url 
                        END,
                        tmdb_id = COALESCE(NULLIF(?, ''), NULLIF(tmdb_id, '')),
                        metadata_updated_at = datetime('now')
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
                .bind(&meta.image_url)
                .bind(&matched_id)
                .bind(&item.id)
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
                        overview = CASE WHEN overview IS NULL OR overview = '' THEN ? ELSE overview END,
                        thumb_url = CASE 
                            WHEN (thumb_url IS NULL OR thumb_url = '' OR thumb_url LIKE '/library/%' OR thumb_url LIKE '/photo/%') AND ? IS NOT NULL 
                            THEN ? 
                            ELSE thumb_url 
                        END,
                        tvdb_id = COALESCE(NULLIF(?, ''), NULLIF(tvdb_id, '')),
                        metadata_updated_at = datetime('now')
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
                .bind(&meta.image_url)
                .bind(&matched_id)
                .bind(&item.id)
                .execute(&state.pool)
                .await;
            }
        }

        // If scrobble had no poster (or relative internal path) and TVDB returned an image, update or wake image cache worker
        let has_valid_thumb = item.thumb_url.as_deref().map(|u| !u.is_empty() && !u.starts_with("/library/")).unwrap_or(false);
        if !has_valid_thumb && meta.image_url.is_some() {
            state.image_notify.notify_waiters();
        }

        processed_count += 1;
        tracing::info!(
            id = %item.id,
            title = %search_title,
            media_type = %item.media_type,
            "Enriched scrobble metadata"
        );
    }

    processed_count
}

/// Helper to trigger metadata processing once (e.g. for testing)
pub async fn process_missing_metadata_once(state: &AppState) -> usize {
    let source = state.get_effective_metadata_source().await;
    let mut failed = HashSet::new();

    if source == "tmdb" {
        if let Some(tmdb_key) = state.get_effective_tmdb_api_key().await.filter(|k| !k.trim().is_empty()) {
            let base_url = state.get_effective_tmdb_base_url().await;
            let client = crate::api::tmdb::TmdbClient::new(base_url, tmdb_key);
            return process_missing_metadata_batch_provider(state, MetadataProvider::Tmdb(&client), &mut failed).await;
        }
    }

    if let Some(tvdb_key) = state.get_effective_tvdb_api_key().await.filter(|k| !k.trim().is_empty()) {
        let base_url = state.get_effective_tvdb_base_url().await;
        let pin = state.get_effective_tvdb_pin().await;
        let client = TvdbClient::new(base_url, tvdb_key, pin);
        return process_missing_metadata_batch_provider(state, MetadataProvider::Tvdb(&client), &mut failed).await;
    }

    // Fallback to TMDB if TVDB wasn't configured
    if let Some(tmdb_key) = state.get_effective_tmdb_api_key().await.filter(|k| !k.trim().is_empty()) {
        let base_url = state.get_effective_tmdb_base_url().await;
        let client = crate::api::tmdb::TmdbClient::new(base_url, tmdb_key);
        return process_missing_metadata_batch_provider(state, MetadataProvider::Tmdb(&client), &mut failed).await;
    }

    0
}
