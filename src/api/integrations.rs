use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use reqwest::Client;
use serde::Deserialize;

use crate::{api::AppState, auth::AuthUser};

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
    let client = Client::builder().timeout(std::time::Duration::from_secs(5)).build().unwrap_or_default();

    if query.media_type == "movie" {
        let user_radarr_url = crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "radarr_url").await;
        let user_radarr_key = crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "radarr_api_key").await;
        if let (Some(ref radarr_url), Some(ref api_key)) = (user_radarr_url, user_radarr_key) {
            if let Ok(safe_url) = crate::api::ssrf::validate_integration_url(radarr_url) {
                let lookup_url = format!("{}/api/v3/movie/lookup", safe_url.as_str().trim_end_matches('/'));
            let res = client
                .get(&lookup_url)
                .header("X-Api-Key", api_key)
                .query(&[("term", &query.title)])
                .send()
                .await;

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
                let res = client
                    .get(&lookup_url)
                    .header("X-Api-Key", api_key)
                    .query(&[("term", search_term)])
                    .send()
                    .await;

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

    let client = Client::builder().timeout(std::time::Duration::from_secs(10)).build().unwrap_or_default();
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
