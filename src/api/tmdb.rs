use reqwest::Client;
use std::time::Duration;

use crate::api::tvdb::{normalize_search_query, TvdbMetadata, TvdbSearchResult};

#[derive(Clone)]
pub struct TmdbClient {
    client: Client,
    base_url: String,
    api_key: String,
}

#[derive(Debug, Clone)]
struct TmdbSearchCandidate {
    id: String,
    name: Option<String>,
    overview: Option<String>,
    image_url: Option<String>,
    year: Option<i32>,
    entity_type: Option<String>,
}

impl TmdbClient {
    pub fn new(base_url: String, api_key: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_default();
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.trim().to_string(),
        }
    }

    fn apply_auth(&self, mut req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.api_key.starts_with("ey") || self.api_key.len() > 60 {
            req = req.header("Authorization", format!("Bearer {}", self.api_key));
        } else {
            req = req.query(&[("api_key", &self.api_key)]);
        }
        req.header("Accept", "application/json")
    }

    async fn execute_search(
        &self,
        endpoint: &str,
        query: &str,
        year: Option<i32>,
        entity_type: &str,
    ) -> Result<Vec<TmdbSearchCandidate>, String> {
        let url = format!("{}/search/{}", self.base_url, endpoint);
        tracing::info!(endpoint = %endpoint, query = %query, year = ?year, "Querying TMDB search API");
        let mut req = self.client.get(&url);
        req = self.apply_auth(req);
        req = req.query(&[("query", query)]);

        if let Some(y) = year {
            let year_str = y.to_string();
            if endpoint == "movie" {
                req = req.query(&[("primary_release_year", &year_str)]);
            } else {
                req = req.query(&[("first_air_date_year", &year_str)]);
            }
        }

        let resp = req
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(endpoint = %endpoint, query = %query, error = %e, "Failed to send search request to TMDB");
                format!("Failed to send search request to TMDB: {}", e)
            })?;

        if !resp.status().is_success() {
            tracing::warn!(endpoint = %endpoint, query = %query, status = %resp.status(), "TMDB search returned error status");
            return Err(format!("TMDB search returned status {}", resp.status()));
        }

        let json_val: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse TMDB search response: {}", e))?;

        let results = match json_val.get("results").and_then(|r| r.as_array()) {
            Some(arr) if !arr.is_empty() => arr,
            _ => return Ok(Vec::new()),
        };

        let mut candidates = Vec::new();
        for item in results {
            let id = match item.get("id") {
                Some(v) if v.is_i64() => v.as_i64().unwrap().to_string(),
                Some(v) if v.is_string() => v.as_str().unwrap().to_string(),
                _ => continue,
            };

            let name = item
                .get("title")
                .or_else(|| item.get("name"))
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string());

            let overview = item
                .get("overview")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());

            let image_url = item
                .get("poster_path")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(|p| {
                    if p.starts_with("http") {
                        p.to_string()
                    } else {
                        format!("https://image.tmdb.org/t/p/w500{}", p)
                    }
                });

            let date_str = item
                .get("release_date")
                .or_else(|| item.get("first_air_date"))
                .and_then(|v| v.as_str());

            let item_year = date_str.and_then(|d| {
                if d.len() >= 4 {
                    d[..4].parse::<i32>().ok()
                } else {
                    None
                }
            });

            candidates.push(TmdbSearchCandidate {
                id,
                name,
                overview,
                image_url,
                year: item_year,
                entity_type: Some(entity_type.to_string()),
            });
        }

        tracing::info!(query = %query, results_count = candidates.len(), "Received TMDB search results");
        Ok(candidates)
    }

    /// Search TMDB by query, media_type ("movie" or "series"), and optional year
    pub async fn search(
        &self,
        query: &str,
        media_type: &str,
        year: Option<i32>,
    ) -> Result<Option<TvdbSearchResult>, String> {
        let (clean_query, detected_year) = normalize_search_query(query);
        let target_year = year.or(detected_year);
        let is_movie = media_type == "movie";

        let primary_endpoint = if is_movie { "movie" } else { "tv" };
        let primary_entity = if is_movie { "movie" } else { "series" };
        let secondary_endpoint = if is_movie { "tv" } else { "movie" };
        let secondary_entity = if is_movie { "series" } else { "movie" };

        let mut candidates = Vec::new();

        // 1. Primary search with year
        if let Some(y) = target_year {
            if let Ok(results) = self.execute_search(primary_endpoint, &clean_query, Some(y), primary_entity).await {
                if !results.is_empty() {
                    candidates = results;
                }
            }
        }

        // 2. Primary search without year
        if candidates.is_empty() {
            if let Ok(results) = self.execute_search(primary_endpoint, &clean_query, None, primary_entity).await {
                if !results.is_empty() {
                    candidates = results;
                }
            }
        }

        // 3. Secondary search (cross-type fallback e.g. miniseries/movie)
        if candidates.is_empty() {
            if let Ok(results) = self.execute_search(secondary_endpoint, &clean_query, target_year, secondary_entity).await {
                if !results.is_empty() {
                    candidates = results;
                }
            }
        }

        if candidates.is_empty() {
            return Ok(None);
        }

        // Rank candidates
        let target_norm = clean_query.trim().to_lowercase();
        candidates.sort_by_key(|c| {
            let mut score = 0;
            if let Some(ref name) = c.name {
                let name_norm = name.trim().to_lowercase();
                if name_norm == target_norm {
                    score += 100;
                } else if name_norm.starts_with(&target_norm) || target_norm.starts_with(&name_norm) {
                    score += 50;
                } else if name_norm.contains(&target_norm) {
                    score += 30;
                }
            }
            if let (Some(cy), Some(ty)) = (c.year, target_year) {
                let diff = (cy - ty).abs();
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
            if c.overview.is_some() {
                score += 5;
            }
            if c.image_url.is_some() {
                score += 5;
            }
            std::cmp::Reverse(score)
        });

        let best = candidates.into_iter().next().unwrap();
        Ok(Some(TvdbSearchResult {
            id: best.id,
            name: best.name,
            overview: best.overview,
            image_url: best.image_url,
            entity_type: best.entity_type,
        }))
    }

    /// Fetch movie details and credits from TMDB: /movie/{id}?append_to_response=credits
    pub async fn get_movie_details(&self, movie_id: &str) -> Result<TvdbMetadata, String> {
        let clean_id = movie_id
            .strip_prefix("movie-")
            .or_else(|| movie_id.strip_prefix("series-"))
            .unwrap_or(movie_id)
            .trim();

        tracing::info!(movie_id = %clean_id, "Fetching movie details from TMDB");
        let url = format!("{}/movie/{}", self.base_url, clean_id);
        let mut req = self.client.get(&url);
        req = self.apply_auth(req);
        req = req.query(&[("append_to_response", "credits")]);

        let resp = req
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(movie_id = %clean_id, error = %e, "Failed to fetch TMDB movie details");
                format!("Failed to fetch TMDB movie details: {}", e)
            })?;

        if !resp.status().is_success() {
            tracing::warn!(movie_id = %clean_id, status = %resp.status(), "TMDB movie details returned error status");
            return Err(format!("TMDB movie details returned status {}", resp.status()));
        }

        let json_val: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse TMDB movie JSON: {}", e))?;

        tracing::info!(movie_id = %clean_id, "Successfully fetched movie details from TMDB");
        Ok(parse_tmdb_metadata(&json_val))
    }

    /// Fetch series details and credits from TMDB: /tv/{id}?append_to_response=credits
    pub async fn get_series_details(&self, series_id: &str) -> Result<TvdbMetadata, String> {
        let clean_id = series_id
            .strip_prefix("series-")
            .or_else(|| series_id.strip_prefix("movie-"))
            .unwrap_or(series_id)
            .trim();

        tracing::info!(series_id = %clean_id, "Fetching series details from TMDB");
        let url = format!("{}/tv/{}", self.base_url, clean_id);
        let mut req = self.client.get(&url);
        req = self.apply_auth(req);
        req = req.query(&[("append_to_response", "credits")]);

        let resp = req
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(series_id = %clean_id, error = %e, "Failed to fetch TMDB tv details");
                format!("Failed to fetch TMDB tv details: {}", e)
            })?;

        if !resp.status().is_success() {
            tracing::warn!(series_id = %clean_id, status = %resp.status(), "TMDB tv details returned error status");
            return Err(format!("TMDB tv details returned status {}", resp.status()));
        }

        let json_val: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse TMDB tv JSON: {}", e))?;

        tracing::info!(series_id = %clean_id, "Successfully fetched series details from TMDB");
        Ok(parse_tmdb_metadata(&json_val))
    }

    pub async fn get_recommendations_for_movie(
        &self,
        tmdb_id: &str,
        reason: &str,
    ) -> Result<Vec<RecommendationItem>, String> {
        let clean_id = tmdb_id.trim();
        let url = format!("{}/movie/{}/recommendations", self.base_url, clean_id);
        let mut req = self.client.get(&url);
        req = self.apply_auth(req);

        let resp = req.send().await.map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("TMDB recommendations returned status {}", resp.status()));
        }
        let json_val: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        let results = match json_val.get("results").and_then(|r| r.as_array()) {
            Some(arr) => arr,
            None => return Ok(Vec::new()),
        };
        let items: Vec<RecommendationItem> = results
            .iter()
            .filter_map(|it| parse_recommendation_item(it, "movie", reason))
            .take(6)
            .collect();
        Ok(items)
    }

    pub async fn get_recommendations_for_tv(
        &self,
        tmdb_id: &str,
        reason: &str,
    ) -> Result<Vec<RecommendationItem>, String> {
        let clean_id = tmdb_id.trim();
        let url = format!("{}/tv/{}/recommendations", self.base_url, clean_id);
        let mut req = self.client.get(&url);
        req = self.apply_auth(req);

        let resp = req.send().await.map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("TMDB recommendations returned status {}", resp.status()));
        }
        let json_val: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        let results = match json_val.get("results").and_then(|r| r.as_array()) {
            Some(arr) => arr,
            None => return Ok(Vec::new()),
        };
        let items: Vec<RecommendationItem> = results
            .iter()
            .filter_map(|it| parse_recommendation_item(it, "tv", reason))
            .take(6)
            .collect();
        Ok(items)
    }

    pub async fn get_trending(&self) -> Result<Vec<RecommendationItem>, String> {
        let url = format!("{}/trending/all/week", self.base_url);
        let mut req = self.client.get(&url);
        req = self.apply_auth(req);

        let resp = req.send().await.map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("TMDB trending returned status {}", resp.status()));
        }
        let json_val: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        let results = match json_val.get("results").and_then(|r| r.as_array()) {
            Some(arr) => arr,
            None => return Ok(Vec::new()),
        };
        let reason = "Popular & Trending this week".to_string();
        let items: Vec<RecommendationItem> = results
            .iter()
            .filter_map(|it| {
                let m = it.get("media_type").and_then(|v| v.as_str()).unwrap_or("movie");
                parse_recommendation_item(it, m, &reason)
            })
            .take(12)
            .collect();
        Ok(items)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecommendationItem {
    pub id: String,
    pub tmdb_id: i64,
    pub title: String,
    pub media_type: String,
    pub overview: Option<String>,
    pub thumb_url: Option<String>,
    pub backdrop_url: Option<String>,
    pub year: Option<i32>,
    pub vote_average: Option<f64>,
    pub reason: String,
}

fn parse_recommendation_item(
    item: &serde_json::Value,
    default_media_type: &str,
    reason: &str,
) -> Option<RecommendationItem> {
    let id_num = item.get("id").and_then(|v| v.as_i64())?;
    let title = item
        .get("title")
        .or_else(|| item.get("name"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())?;

    let media_type = item
        .get("media_type")
        .and_then(|v| v.as_str())
        .unwrap_or(default_media_type)
        .to_string();

    let overview = item
        .get("overview")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let thumb_url = item
        .get("poster_path")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|p| {
            if p.starts_with("http") {
                p.to_string()
            } else {
                format!("https://image.tmdb.org/t/p/w500{}", p)
            }
        });

    let backdrop_url = item
        .get("backdrop_path")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|p| {
            if p.starts_with("http") {
                p.to_string()
            } else {
                format!("https://image.tmdb.org/t/p/w780{}", p)
            }
        });

    let year = item
        .get("release_date")
        .or_else(|| item.get("first_air_date"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.split('-').next())
        .and_then(|y| y.parse::<i32>().ok());

    let vote_average = item.get("vote_average").and_then(|v| v.as_f64());

    Some(RecommendationItem {
        id: id_num.to_string(),
        tmdb_id: id_num,
        title,
        media_type,
        overview,
        thumb_url,
        backdrop_url,
        year,
        vote_average,
        reason: reason.to_string(),
    })
}

fn parse_tmdb_metadata(data: &serde_json::Value) -> TvdbMetadata {
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
                .filter_map(|item| item.get("name").and_then(|n| n.as_str()).map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect();
            names.join(", ")
        })
        .filter(|s| !s.is_empty());

    let mut actors_list: Vec<String> = Vec::new();
    let mut directors_list: Vec<String> = Vec::new();
    let mut cinematographers_list: Vec<String> = Vec::new();

    // Check created_by (for TV shows)
    if let Some(created) = data.get("created_by").and_then(|v| v.as_array()) {
        for c in created {
            if let Some(name) = c.get("name").and_then(|v| v.as_str()).map(|s| s.trim().to_string()) {
                if !directors_list.contains(&name) {
                    directors_list.push(name);
                }
            }
        }
    }

    // Check credits
    if let Some(credits) = data.get("credits") {
        if let Some(cast) = credits.get("cast").and_then(|v| v.as_array()) {
            for c in cast {
                if let Some(name) = c.get("name").and_then(|v| v.as_str()).map(|s| s.trim().to_string()) {
                    if !actors_list.contains(&name) && actors_list.len() < 12 {
                        actors_list.push(name);
                    }
                }
            }
        }

        if let Some(crew) = credits.get("crew").and_then(|v| v.as_array()) {
            for c in crew {
                let name = match c.get("name").and_then(|v| v.as_str()).map(|s| s.trim().to_string()) {
                    Some(n) if !n.is_empty() => n,
                    _ => continue,
                };

                let job = c.get("job").and_then(|v| v.as_str()).map(|s| s.to_lowercase()).unwrap_or_default();
                let dept = c.get("department").and_then(|v| v.as_str()).map(|s| s.to_lowercase()).unwrap_or_default();

                if job == "director" || dept == "directing" {
                    if !directors_list.contains(&name) {
                        directors_list.push(name.clone());
                    }
                }

                if job.contains("cinematograph")
                    || job.contains("director of photography")
                    || dept == "camera"
                    || job.contains("camera operator")
                {
                    if !cinematographers_list.contains(&name) {
                        cinematographers_list.push(name);
                    }
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

    let director = directors_list.first().cloned();

    let cinematographer = if cinematographers_list.is_empty() {
        None
    } else {
        Some(cinematographers_list.join(", "))
    };

    let image_url = data
        .get("poster_path")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|p| {
            if p.starts_with("http") {
                p.to_string()
            } else {
                format!("https://image.tmdb.org/t/p/w500{}", p)
            }
        });

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
