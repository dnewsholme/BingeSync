use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::{api::AppState, auth::AuthUser};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CleanupSeasonInfo {
    pub season_number: i32,
    pub monitored: bool,
    pub episode_file_count: i64,
    pub watched: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CleanupItem {
    pub id: String,
    pub media_type: String,
    pub service: String,
    pub service_id: i64,
    pub title: String,
    pub year: Option<i32>,
    pub size_on_disk: i64,
    pub size_formatted: String,
    pub has_file: bool,
    pub monitored: bool,
    pub poster_url: Option<String>,
    pub last_watched_at: String,
    pub watch_count: i64,
    pub episode_file_count: Option<i64>,
    pub total_episode_count: Option<i64>,
    pub watched_episodes_count: Option<i64>,
    pub seasons: Vec<CleanupSeasonInfo>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CleanupResponse {
    pub radarr_configured: bool,
    pub sonarr_configured: bool,
    pub radarr_error: Option<String>,
    pub sonarr_error: Option<String>,
    pub total_items: usize,
    pub total_size_bytes: i64,
    pub total_size_formatted: String,
    pub items: Vec<CleanupItem>,
}

pub fn format_bytes(bytes: i64) -> String {
    if bytes <= 0 {
        return "0 B".to_string();
    }
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * KB;
    const GB: f64 = 1024.0 * MB;
    const TB: f64 = 1024.0 * GB;

    let b = bytes as f64;
    if b >= TB {
        format!("{:.1} TB", b / TB)
    } else if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

fn normalize_for_match(s: &str) -> String {
    let lower = s.trim().to_lowercase();
    let cleaned: String = lower
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    let mut words: Vec<&str> = cleaned.split_whitespace().collect();
    if words.first() == Some(&"the") {
        words.remove(0);
    }
    words.join(" ")
}

#[derive(Debug, Deserialize)]
pub struct IntegrationCheckQuery {
    pub title: String,
    pub media_type: String,
    pub year: Option<i32>,
    pub grandparent_title: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RadarrDeleteRequest {
    pub movie_id: i64,
    pub delete_files: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct SonarrDeleteRequest {
    pub series_id: i64,
    pub delete_files: Option<bool>,
    pub action: Option<String>,
    pub season_number: Option<i32>,
}

pub async fn get_integrations_status(
    State(state): State<AppState>,
    auth: AuthUser,
) -> impl IntoResponse {
    let radarr_url = crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "radarr_url").await;
    let radarr_key = crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "radarr_api_key").await;
    let radarr_configured = radarr_url.is_some() && radarr_key.is_some();

    let sonarr_url = crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "sonarr_url").await;
    let sonarr_key = crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "sonarr_api_key").await;
    let sonarr_configured = sonarr_url.is_some() && sonarr_key.is_some();

    let tvdb_key = state.get_effective_tvdb_api_key().await;
    let tvdb_configured = tvdb_key.is_some() && !tvdb_key.as_deref().unwrap_or("").is_empty();

    Json(serde_json::json!({
        "radarr_configured": radarr_configured,
        "sonarr_configured": sonarr_configured,
        "tvdb_configured": tvdb_configured,
    }))
}

pub async fn check_media_in_library(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<IntegrationCheckQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap_or_default();

    if query.media_type == "movie" {
        let user_radarr_url = crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "radarr_url").await;
        let user_radarr_key = crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "radarr_api_key").await;
        if let (Some(ref radarr_url), Some(ref api_key)) = (user_radarr_url, user_radarr_key) {
            if let Ok(safe_url) = crate::api::ssrf::validate_integration_url(radarr_url) {
                let lookup_url = format!("{}/api/v3/movie/lookup", safe_url.as_str().trim_end_matches('/'));
                let mut res = client
                    .get(&lookup_url)
                    .header("X-Api-Key", api_key)
                    .query(&[("term", &query.title)])
                    .send()
                    .await;

                if res.is_err() && safe_url.scheme() == "https" {
                    let mut fallback_url = safe_url.clone();
                    if fallback_url.set_scheme("http").is_ok() {
                        let fallback_req = format!("{}/api/v3/movie/lookup", fallback_url.as_str().trim_end_matches('/'));
                        if let Ok(f_res) = client.get(&fallback_req).header("X-Api-Key", api_key).query(&[("term", &query.title)]).send().await {
                            if f_res.status().is_success() {
                                res = Ok(f_res);
                            }
                        }
                    }
                }

                if let Ok(resp) = res {
                    if resp.status().is_success() {
                        if let Ok(movies) = resp.json::<Vec<serde_json::Value>>().await {
                            for m in movies {
                                let m_id = m.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
                                let title_match = m.get("title").and_then(|v| v.as_str()).map(|t| t.eq_ignore_ascii_case(&query.title)).unwrap_or(false);
                                let year_match = match (query.year, m.get("year").and_then(|y| y.as_i64())) {
                                    (Some(qy), Some(my)) => (qy as i64 - my).abs() <= 1,
                                    _ => true,
                                };

                                if m_id > 0 && (title_match || year_match) {
                                    let has_file = m.get("hasFile").and_then(|v| v.as_bool()).unwrap_or(false);
                                    let size_on_disk = m.get("sizeOnDisk").and_then(|v| v.as_i64()).unwrap_or(0);
                                    return Ok(Json(serde_json::json!({
                                        "found": true,
                                        "service": "radarr",
                                        "movie_id": m_id,
                                        "title": m.get("title"),
                                        "has_file": has_file,
                                        "size_on_disk": size_on_disk,
                                    })));
                                }
                            }
                        }
                    }
                }
            }
        }

        return Ok(Json(serde_json::json!({ "found": false, "service": "radarr" })));
    } else {
        // Episode / TV Series -> check Sonarr
        let user_sonarr_url = crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "sonarr_url").await;
        let user_sonarr_key = crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "sonarr_api_key").await;
        if let (Some(ref sonarr_url), Some(ref api_key)) = (user_sonarr_url, user_sonarr_key) {
            if let Ok(safe_url) = crate::api::ssrf::validate_integration_url(sonarr_url) {
                let search_term = query.grandparent_title.as_deref().unwrap_or(&query.title);
                let lookup_url = format!("{}/api/v3/series/lookup", safe_url.as_str().trim_end_matches('/'));
                let mut res = client
                    .get(&lookup_url)
                    .header("X-Api-Key", api_key)
                    .query(&[("term", search_term)])
                    .send()
                    .await;

                if res.is_err() && safe_url.scheme() == "https" {
                    let mut fallback_url = safe_url.clone();
                    if fallback_url.set_scheme("http").is_ok() {
                        let fallback_req = format!("{}/api/v3/series/lookup", fallback_url.as_str().trim_end_matches('/'));
                        if let Ok(f_res) = client.get(&fallback_req).header("X-Api-Key", api_key).query(&[("term", search_term)]).send().await {
                            if f_res.status().is_success() {
                                res = Ok(f_res);
                            }
                        }
                    }
                }

                if let Ok(resp) = res {
                    if resp.status().is_success() {
                        if let Ok(series_list) = resp.json::<Vec<serde_json::Value>>().await {
                            for s in series_list {
                                let s_id = s.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
                                if s_id > 0 {
                                    return Ok(Json(serde_json::json!({
                                        "found": true,
                                        "service": "sonarr",
                                        "series_id": s_id,
                                        "title": s.get("title"),
                                    })));
                                }
                            }
                        }
                    }
                }
            }
        }

        return Ok(Json(serde_json::json!({ "found": false, "service": "sonarr" })));
    }
}

pub async fn delete_from_radarr(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(payload): Json<RadarrDeleteRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let user_radarr_url = crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "radarr_url").await;
    let user_radarr_key = crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "radarr_api_key").await;
    let (radarr_url, api_key_val) = match (user_radarr_url, user_radarr_key) {
        (Some(u), Some(k)) => (u, k),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Radarr is not configured in your settings" })),
            ));
        }
    };
    let api_key = api_key_val.as_str();
    let safe_url = crate::api::ssrf::validate_integration_url(&radarr_url).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("Invalid Radarr URL: {}", e) })),
        )
    })?;

    let delete_files = payload.delete_files.unwrap_or(true);
    let target_url = format!(
        "{}/api/v3/movie/{}?deleteFiles={}&addImportExclusion=false",
        safe_url.as_str().trim_end_matches('/'),
        payload.movie_id,
        delete_files
    );

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap_or_default();
    let res = client
        .delete(&target_url)
        .header("X-Api-Key", api_key)
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("Failed to communicate with Radarr: {}", e) })),
            )
        })?;

    if !res.status().is_success() {
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("Radarr returned error status: {}", res.status()) })),
        ));
    }

    Ok(Json(serde_json::json!({
        "status": "deleted",
        "service": "radarr",
        "movie_id": payload.movie_id,
        "message": "Movie successfully removed from Radarr"
    })))
}

pub async fn delete_from_sonarr(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(payload): Json<SonarrDeleteRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let user_sonarr_url = crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "sonarr_url").await;
    let user_sonarr_key = crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "sonarr_api_key").await;
    let (sonarr_url, api_key_val) = match (user_sonarr_url, user_sonarr_key) {
        (Some(u), Some(k)) => (u, k),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Sonarr is not configured in your settings" })),
            ));
        }
    };
    let api_key = api_key_val.as_str();
    let safe_url = crate::api::ssrf::validate_integration_url(&sonarr_url).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("Invalid Sonarr URL: {}", e) })),
        )
    })?;

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap_or_default();
    let base_url = safe_url.as_str().trim_end_matches('/');
    let action = payload.action.as_deref().unwrap_or("delete_series");

    match action {
        "delete_season" => {
            let season = match payload.season_number {
                Some(s) => s,
                None => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({ "error": "season_number is required for delete_season action" })),
                    ));
                }
            };

            // 1. Fetch episode files and delete those belonging to this season
            let ep_files_url = format!("{}/api/v3/episodefile?seriesId={}", base_url, payload.series_id);
            let ep_files_res = client
                .get(&ep_files_url)
                .header("X-Api-Key", api_key)
                .send()
                .await;

            let mut deleted_files_count = 0;
            if let Ok(resp) = ep_files_res {
                if resp.status().is_success() {
                    if let Ok(files) = resp.json::<Vec<serde_json::Value>>().await {
                        for f in files {
                            if f.get("seasonNumber").and_then(|v| v.as_i64()) == Some(season as i64) {
                                if let Some(file_id) = f.get("id").and_then(|v| v.as_i64()) {
                                    let del_url = format!("{}/api/v3/episodefile/{}", base_url, file_id);
                                    let _ = client
                                        .delete(&del_url)
                                        .header("X-Api-Key", api_key)
                                        .send()
                                        .await;
                                    deleted_files_count += 1;
                                }
                            }
                        }
                    }
                }
            }

            // 2. Fetch series, update season monitored to false, and PUT back
            let series_url = format!("{}/api/v3/series/{}", base_url, payload.series_id);
            let series_res = client
                .get(&series_url)
                .header("X-Api-Key", api_key)
                .send()
                .await
                .map_err(|e| {
                    (
                        StatusCode::BAD_GATEWAY,
                        Json(serde_json::json!({ "error": format!("Failed to fetch series from Sonarr: {}", e) })),
                    )
                })?;

            if !series_res.status().is_success() {
                return Err((
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({ "error": format!("Sonarr returned error status: {}", series_res.status()) })),
                ));
            }

            let mut series_json: serde_json::Value = series_res.json().await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("Failed to parse Sonarr response: {}", e) })),
                )
            })?;

            if let Some(seasons) = series_json.get_mut("seasons").and_then(|v| v.as_array_mut()) {
                for s in seasons {
                    if s.get("seasonNumber").and_then(|v| v.as_i64()) == Some(season as i64) {
                        s["monitored"] = serde_json::json!(false);
                    }
                }
            }

            let put_res = client
                .put(&series_url)
                .header("X-Api-Key", api_key)
                .json(&series_json)
                .send()
                .await;

            if let Ok(ref pr) = put_res {
                if pr.status() == StatusCode::NOT_FOUND || pr.status() == StatusCode::METHOD_NOT_ALLOWED {
                    let fallback_url = format!("{}/api/v3/series", base_url);
                    let _ = client
                        .put(&fallback_url)
                        .header("X-Api-Key", api_key)
                        .json(&series_json)
                        .send()
                        .await;
                }
            }

            // 3. Unmonitor episodes in this season
            let ep_url = format!("{}/api/v3/episode?seriesId={}&seasonNumber={}", base_url, payload.series_id, season);
            if let Ok(ep_res) = client.get(&ep_url).header("X-Api-Key", api_key).send().await {
                if ep_res.status().is_success() {
                    if let Ok(episodes) = ep_res.json::<Vec<serde_json::Value>>().await {
                        let ep_ids: Vec<i64> = episodes.iter().filter_map(|e| e.get("id").and_then(|v| v.as_i64())).collect();
                        if !ep_ids.is_empty() {
                            let mon_url = format!("{}/api/v3/episode/monitor", base_url);
                            let _ = client
                                .put(&mon_url)
                                .header("X-Api-Key", api_key)
                                .json(&serde_json::json!({
                                    "episodeIds": ep_ids,
                                    "monitored": false
                                }))
                                .send()
                                .await;
                        }
                    }
                }
            }

            Ok(Json(serde_json::json!({
                "status": "deleted",
                "service": "sonarr",
                "series_id": payload.series_id,
                "action": "delete_season",
                "season_number": season,
                "deleted_files_count": deleted_files_count,
                "message": format!("Season {} files deleted and season unmonitored in Sonarr", season)
            })))
        }

        "delete_all_files_unmonitor" => {
            // 1. Fetch all episode files and delete all of them
            let ep_files_url = format!("{}/api/v3/episodefile?seriesId={}", base_url, payload.series_id);
            let ep_files_res = client
                .get(&ep_files_url)
                .header("X-Api-Key", api_key)
                .send()
                .await;

            let mut deleted_files_count = 0;
            if let Ok(resp) = ep_files_res {
                if resp.status().is_success() {
                    if let Ok(files) = resp.json::<Vec<serde_json::Value>>().await {
                        for f in files {
                            if let Some(file_id) = f.get("id").and_then(|v| v.as_i64()) {
                                let del_url = format!("{}/api/v3/episodefile/{}", base_url, file_id);
                                let _ = client
                                    .delete(&del_url)
                                    .header("X-Api-Key", api_key)
                                    .send()
                                    .await;
                                deleted_files_count += 1;
                            }
                        }
                    }
                }
            }

            // 2. Fetch series, update all seasons and series monitored to false, and PUT back
            let series_url = format!("{}/api/v3/series/{}", base_url, payload.series_id);
            let series_res = client
                .get(&series_url)
                .header("X-Api-Key", api_key)
                .send()
                .await
                .map_err(|e| {
                    (
                        StatusCode::BAD_GATEWAY,
                        Json(serde_json::json!({ "error": format!("Failed to fetch series from Sonarr: {}", e) })),
                    )
                })?;

            if !series_res.status().is_success() {
                return Err((
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({ "error": format!("Sonarr returned error status: {}", series_res.status()) })),
                ));
            }

            let mut series_json: serde_json::Value = series_res.json().await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("Failed to parse Sonarr response: {}", e) })),
                )
            })?;

            series_json["monitored"] = serde_json::json!(false);
            if let Some(seasons) = series_json.get_mut("seasons").and_then(|v| v.as_array_mut()) {
                for s in seasons {
                    s["monitored"] = serde_json::json!(false);
                }
            }

            let put_res = client
                .put(&series_url)
                .header("X-Api-Key", api_key)
                .json(&series_json)
                .send()
                .await;

            if let Ok(ref pr) = put_res {
                if pr.status() == StatusCode::NOT_FOUND || pr.status() == StatusCode::METHOD_NOT_ALLOWED {
                    let fallback_url = format!("{}/api/v3/series", base_url);
                    let _ = client
                        .put(&fallback_url)
                        .header("X-Api-Key", api_key)
                        .json(&series_json)
                        .send()
                        .await;
                }
            }

            // 3. Unmonitor all episodes
            let ep_url = format!("{}/api/v3/episode?seriesId={}", base_url, payload.series_id);
            if let Ok(ep_res) = client.get(&ep_url).header("X-Api-Key", api_key).send().await {
                if ep_res.status().is_success() {
                    if let Ok(episodes) = ep_res.json::<Vec<serde_json::Value>>().await {
                        let ep_ids: Vec<i64> = episodes.iter().filter_map(|e| e.get("id").and_then(|v| v.as_i64())).collect();
                        if !ep_ids.is_empty() {
                            let mon_url = format!("{}/api/v3/episode/monitor", base_url);
                            let _ = client
                                .put(&mon_url)
                                .header("X-Api-Key", api_key)
                                .json(&serde_json::json!({
                                    "episodeIds": ep_ids,
                                    "monitored": false
                                }))
                                .send()
                                .await;
                        }
                    }
                }
            }

            Ok(Json(serde_json::json!({
                "status": "deleted",
                "service": "sonarr",
                "series_id": payload.series_id,
                "action": "delete_all_files_unmonitor",
                "deleted_files_count": deleted_files_count,
                "message": "All series files deleted and all seasons unmonitored in Sonarr"
            })))
        }

        _ => {
            let delete_files = payload.delete_files.unwrap_or(true);
            let target_url = format!(
                "{}/api/v3/series/{}?deleteFiles={}&addImportListExclusion=false",
                base_url,
                payload.series_id,
                delete_files
            );

            let res = client
                .delete(&target_url)
                .header("X-Api-Key", api_key)
                .send()
                .await
                .map_err(|e| {
                    (
                        StatusCode::BAD_GATEWAY,
                        Json(serde_json::json!({ "error": format!("Failed to communicate with Sonarr: {}", e) })),
                    )
                })?;

            if !res.status().is_success() {
                return Err((
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({ "error": format!("Sonarr returned error status: {}", res.status()) })),
                ));
            }

            Ok(Json(serde_json::json!({
                "status": "deleted",
                "service": "sonarr",
                "series_id": payload.series_id,
                "action": "delete_series",
                "message": "Series and all files successfully removed from Sonarr"
            })))
        }
    }
}

pub async fn get_cleanup_items(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap_or_default();

    // 1. Resolve Radarr config
    let user_radarr_url = crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "radarr_url").await
        .or_else(|| std::env::var("RADARR_URL").ok());
    let user_radarr_key = crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "radarr_api_key").await
        .or_else(|| std::env::var("RADARR_API_KEY").ok());
    let radarr_configured = user_radarr_url.is_some() && user_radarr_key.is_some();

    // 2. Resolve Sonarr config
    let user_sonarr_url = crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "sonarr_url").await
        .or_else(|| std::env::var("SONARR_URL").ok());
    let user_sonarr_key = crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "sonarr_api_key").await
        .or_else(|| std::env::var("SONARR_API_KEY").ok());
    let sonarr_configured = user_sonarr_url.is_some() && user_sonarr_key.is_some();

    let mut items: Vec<CleanupItem> = Vec::new();
    let mut radarr_error = None;
    let mut sonarr_error = None;

    // 3. Process Radarr (Movies) if configured
    if let (Some(ref r_url), Some(ref r_key)) = (&user_radarr_url, &user_radarr_key) {
        match crate::api::ssrf::validate_integration_url(r_url) {
            Ok(safe_url) => {
                #[derive(sqlx::FromRow)]
                struct WatchedMovieRow {
                    title: String,
                    year: Option<i32>,
                    last_watched: String,
                    watch_count: i64,
                    thumb_url: Option<String>,
                }

                let watched_movies = sqlx::query_as::<_, WatchedMovieRow>(
                    r#"
                    SELECT title, year, MAX(watched_at) as last_watched, COUNT(*) as watch_count, MAX(thumb_url) as thumb_url
                    FROM scrobbles
                    WHERE user_id = ? AND media_type = 'movie'
                    GROUP BY LOWER(TRIM(title)), year
                    "#
                )
                .bind(&auth.user_id)
                .fetch_all(&state.pool)
                .await
                .unwrap_or_default();

                if !watched_movies.is_empty() {
                    let req_url = format!("{}/api/v3/movie", safe_url.as_str().trim_end_matches('/'));
                    let mut res = client.get(&req_url).header("X-Api-Key", r_key.as_str()).send().await;

                    if res.is_err() && safe_url.scheme() == "https" {
                        let mut fallback_url = safe_url.clone();
                        if fallback_url.set_scheme("http").is_ok() {
                            let fallback_req = format!("{}/api/v3/movie", fallback_url.as_str().trim_end_matches('/'));
                            if let Ok(fallback_res) = client.get(&fallback_req).header("X-Api-Key", r_key.as_str()).send().await {
                                if fallback_res.status().is_success() {
                                    res = Ok(fallback_res);
                                }
                            }
                        }
                    }

                    match res {
                        Ok(resp) if resp.status().is_success() => {
                            if let Ok(radarr_movies) = resp.json::<Vec<serde_json::Value>>().await {
                                for rm in radarr_movies {
                                    let rm_id = rm.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
                                    if rm_id <= 0 { continue; }
                                    let rm_title = rm.get("title").and_then(|v| v.as_str()).unwrap_or("");
                                    let rm_year = rm.get("year").and_then(|v| v.as_i64());
                                    let has_file = rm.get("hasFile").and_then(|v| v.as_bool()).unwrap_or(false);
                                    let size_on_disk = rm.get("sizeOnDisk").and_then(|v| v.as_i64()).unwrap_or(0);
                                    let monitored = rm.get("monitored").and_then(|v| v.as_bool()).unwrap_or(true);

                                    let norm_rm = normalize_for_match(rm_title);
                                    let matched = watched_movies.iter().find(|wm| {
                                        let norm_wm = normalize_for_match(&wm.title);
                                        if norm_rm == norm_wm {
                                            match (rm_year, wm.year) {
                                                (Some(ry), Some(wy)) => (ry - wy as i64).abs() <= 1,
                                                _ => true,
                                            }
                                        } else {
                                            false
                                        }
                                    });

                                    if let Some(wm) = matched {
                                        let poster_url = rm.get("images")
                                            .and_then(|imgs| imgs.as_array())
                                            .and_then(|arr| arr.iter().find(|img| img.get("coverType").and_then(|c| c.as_str()) == Some("poster")))
                                            .and_then(|p| p.get("remoteUrl").or_else(|| p.get("url")))
                                            .and_then(|u| u.as_str())
                                            .map(|s| s.to_string())
                                            .or_else(|| wm.thumb_url.clone());

                                        items.push(CleanupItem {
                                            id: format!("radarr:{}", rm_id),
                                            media_type: "movie".to_string(),
                                            service: "radarr".to_string(),
                                            service_id: rm_id,
                                            title: rm_title.to_string(),
                                            year: rm_year.map(|y| y as i32),
                                            size_on_disk,
                                            size_formatted: format_bytes(size_on_disk),
                                            has_file,
                                            monitored,
                                            poster_url,
                                            last_watched_at: wm.last_watched.clone(),
                                            watch_count: wm.watch_count,
                                            episode_file_count: None,
                                            total_episode_count: None,
                                            watched_episodes_count: None,
                                            seasons: Vec::new(),
                                        });
                                    }
                                }
                            }
                        }
                        Ok(resp) => {
                            radarr_error = Some(format!("Radarr returned HTTP status {}", resp.status()));
                        }
                        Err(e) => {
                            let hint = if safe_url.scheme() == "https" {
                                " (Tip: if Radarr is running without SSL on this port, use http:// instead of https://)"
                            } else {
                                ""
                            };
                            radarr_error = Some(format!("Failed to connect to Radarr: {}{}", e, hint));
                        }
                    }
                }
            }
            Err(e) => {
                radarr_error = Some(format!("Invalid Radarr URL: {}", e));
            }
        }
    }

    // 4. Process Sonarr (TV Shows) if configured
    if let (Some(ref s_url), Some(ref s_key)) = (&user_sonarr_url, &user_sonarr_key) {
        match crate::api::ssrf::validate_integration_url(s_url) {
            Ok(safe_url) => {
                #[derive(sqlx::FromRow)]
                struct WatchedEpisodeRow {
                    show_title: String,
                    season_number: Option<i32>,
                    last_watched: String,
                    thumb_url: Option<String>,
                }

                let watched_episodes = sqlx::query_as::<_, WatchedEpisodeRow>(
                    r#"
                    SELECT COALESCE(grandparent_title, title) as show_title, season_number, watched_at as last_watched, thumb_url
                    FROM scrobbles
                    WHERE user_id = ? AND media_type = 'episode'
                    ORDER BY watched_at DESC
                    "#
                )
                .bind(&auth.user_id)
                .fetch_all(&state.pool)
                .await
                .unwrap_or_default();

                if !watched_episodes.is_empty() {
                    let mut show_map: std::collections::HashMap<String, (String, Option<String>, i64, std::collections::HashSet<i32>)> = std::collections::HashMap::new();
                    for ep in watched_episodes {
                        let norm = normalize_for_match(&ep.show_title);
                        let entry = show_map.entry(norm).or_insert_with(|| {
                            (ep.last_watched.clone(), ep.thumb_url.clone(), 0, std::collections::HashSet::new())
                        });
                        entry.2 += 1;
                        if let Some(sn) = ep.season_number {
                            entry.3.insert(sn);
                        }
                    }

                    let req_url = format!("{}/api/v3/series", safe_url.as_str().trim_end_matches('/'));
                    let mut res = client.get(&req_url).header("X-Api-Key", s_key.as_str()).send().await;

                    if res.is_err() && safe_url.scheme() == "https" {
                        let mut fallback_url = safe_url.clone();
                        if fallback_url.set_scheme("http").is_ok() {
                            let fallback_req = format!("{}/api/v3/series", fallback_url.as_str().trim_end_matches('/'));
                            if let Ok(fallback_res) = client.get(&fallback_req).header("X-Api-Key", s_key.as_str()).send().await {
                                if fallback_res.status().is_success() {
                                    res = Ok(fallback_res);
                                }
                            }
                        }
                    }

                    match res {
                        Ok(resp) if resp.status().is_success() => {
                            if let Ok(sonarr_series) = resp.json::<Vec<serde_json::Value>>().await {
                                for ss in sonarr_series {
                                    let ss_id = ss.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
                                    if ss_id <= 0 { continue; }
                                    let ss_title = ss.get("title").and_then(|v| v.as_str()).unwrap_or("");
                                    let ss_year = ss.get("year").and_then(|v| v.as_i64());
                                    let monitored = ss.get("monitored").and_then(|v| v.as_bool()).unwrap_or(true);

                                    let stats = ss.get("statistics");
                                    let size_on_disk = stats.and_then(|st| st.get("sizeOnDisk")).and_then(|v| v.as_i64()).unwrap_or(0);
                                    let ep_file_count = stats.and_then(|st| st.get("episodeFileCount")).and_then(|v| v.as_i64()).unwrap_or(0);
                                    let total_ep_count = stats.and_then(|st| st.get("totalEpisodeCount")).and_then(|v| v.as_i64()).unwrap_or(0);

                                    let norm_ss = normalize_for_match(ss_title);
                                    if let Some((last_watched, thumb_url, watched_count, watched_seasons)) = show_map.get(&norm_ss) {
                                        let poster_url = ss.get("images")
                                            .and_then(|imgs| imgs.as_array())
                                            .and_then(|arr| arr.iter().find(|img| img.get("coverType").and_then(|c| c.as_str()) == Some("poster")))
                                            .and_then(|p| p.get("remoteUrl").or_else(|| p.get("url")))
                                            .and_then(|u| u.as_str())
                                            .map(|s| s.to_string())
                                            .or_else(|| thumb_url.clone());

                                        let mut seasons_info = Vec::new();
                                        if let Some(seasons_arr) = ss.get("seasons").and_then(|s| s.as_array()) {
                                            for s in seasons_arr {
                                                let season_num = s.get("seasonNumber").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                                                let s_monitored = s.get("monitored").and_then(|v| v.as_bool()).unwrap_or(true);
                                                let s_file_count = s.get("statistics")
                                                    .and_then(|st| st.get("episodeFileCount"))
                                                    .and_then(|v| v.as_i64())
                                                    .unwrap_or(0);

                                                seasons_info.push(CleanupSeasonInfo {
                                                    season_number: season_num,
                                                    monitored: s_monitored,
                                                    episode_file_count: s_file_count,
                                                    watched: watched_seasons.contains(&season_num),
                                                });
                                            }
                                        }

                                        items.push(CleanupItem {
                                            id: format!("sonarr:{}", ss_id),
                                            media_type: "series".to_string(),
                                            service: "sonarr".to_string(),
                                            service_id: ss_id,
                                            title: ss_title.to_string(),
                                            year: ss_year.map(|y| y as i32),
                                            size_on_disk,
                                            size_formatted: format_bytes(size_on_disk),
                                            has_file: ep_file_count > 0 || size_on_disk > 0,
                                            monitored,
                                            poster_url,
                                            last_watched_at: last_watched.clone(),
                                            watch_count: *watched_count,
                                            episode_file_count: Some(ep_file_count),
                                            total_episode_count: Some(total_ep_count),
                                            watched_episodes_count: Some(*watched_count),
                                            seasons: seasons_info,
                                        });
                                    }
                                }
                            }
                        }
                        Ok(resp) => {
                            sonarr_error = Some(format!("Sonarr returned HTTP status {}", resp.status()));
                        }
                        Err(e) => {
                            let hint = if safe_url.scheme() == "https" {
                                " (Tip: if Sonarr is running without SSL on this port, use http:// instead of https://)"
                            } else {
                                ""
                            };
                            sonarr_error = Some(format!("Failed to connect to Sonarr: {}{}", e, hint));
                        }
                    }
                }
            }
            Err(e) => {
                sonarr_error = Some(format!("Invalid Sonarr URL: {}", e));
            }
        }
    }

    // Sort items by last_watched_at DESC
    items.sort_by(|a, b| b.last_watched_at.cmp(&a.last_watched_at));

    let total_size_bytes: i64 = items.iter().map(|it| it.size_on_disk).sum();
    let total_items = items.len();

    Ok(Json(serde_json::to_value(CleanupResponse {
        radarr_configured,
        sonarr_configured,
        radarr_error,
        sonarr_error,
        total_items,
        total_size_bytes,
        total_size_formatted: format_bytes(total_size_bytes),
        items,
    }).unwrap_or_default()))
}
