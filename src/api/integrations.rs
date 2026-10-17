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
}

pub async fn get_integrations_status(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> impl IntoResponse {
    let radarr_configured = state.config.radarr_url.is_some() && state.config.radarr_api_key.is_some();
    let sonarr_configured = state.config.sonarr_url.is_some() && state.config.sonarr_api_key.is_some();

    Json(serde_json::json!({
        "radarr_configured": radarr_configured,
        "sonarr_configured": sonarr_configured,
    }))
}

pub async fn check_media_in_library(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(query): Query<IntegrationCheckQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let client = Client::builder().timeout(std::time::Duration::from_secs(5)).build().unwrap_or_default();

    if query.media_type == "movie" {
        if let (Some(ref radarr_url), Some(ref api_key)) = (&state.config.radarr_url, &state.config.radarr_api_key) {
            let lookup_url = format!("{}/api/v3/movie/lookup", radarr_url.trim_end_matches('/'));
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

        return Ok(Json(serde_json::json!({ "found": false, "service": "radarr" })));
    } else {
        // Episode / TV Series -> check Sonarr
        if let (Some(ref sonarr_url), Some(ref api_key)) = (&state.config.sonarr_url, &state.config.sonarr_api_key) {
            let search_term = query.grandparent_title.as_deref().unwrap_or(&query.title);
            let lookup_url = format!("{}/api/v3/series/lookup", sonarr_url.trim_end_matches('/'));
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

        return Ok(Json(serde_json::json!({ "found": false, "service": "sonarr" })));
    }
}

pub async fn delete_from_radarr(
    State(state): State<AppState>,
    _auth: AuthUser,
    Json(payload): Json<RadarrDeleteRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let (radarr_url, api_key) = match (&state.config.radarr_url, &state.config.radarr_api_key) {
        (Some(u), Some(k)) => (u, k),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Radarr is not configured on this instance" })),
            ));
        }
    };

    let delete_files = payload.delete_files.unwrap_or(true);
    let target_url = format!(
        "{}/api/v3/movie/{}?deleteFiles={}&addImportExclusion=false",
        radarr_url.trim_end_matches('/'),
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
    _auth: AuthUser,
    Json(payload): Json<SonarrDeleteRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let (sonarr_url, api_key) = match (&state.config.sonarr_url, &state.config.sonarr_api_key) {
        (Some(u), Some(k)) => (u, k),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Sonarr is not configured on this instance" })),
            ));
        }
    };

    let delete_files = payload.delete_files.unwrap_or(true);
    let target_url = format!(
        "{}/api/v3/series/{}?deleteFiles={}&addImportListExclusion=false",
        sonarr_url.trim_end_matches('/'),
        payload.series_id,
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
        "message": "Series successfully removed from Sonarr"
    })))
}
