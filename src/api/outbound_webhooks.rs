use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::{api::AppState, auth::AuthUser};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrobbleNotificationData {
    pub title: String,
    pub grandparent_title: Option<String>,
    pub media_type: String,
    pub season_number: Option<i32>,
    pub episode_number: Option<i32>,
    pub year: Option<i32>,
    pub progress_percent: i64,
    pub thumb_url: Option<String>,
    pub user_rating: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct TestWebhookRequest {
    pub webhook_url: Option<String>,
    pub webhook_type: Option<String>,
}

/// Dispatches an outbound webhook notification asynchronously when a scrobble finishes
pub fn dispatch_scrobble_notification(
    pool: SqlitePool,
    user_id: String,
    data: ScrobbleNotificationData,
) {
    tokio::spawn(async move {
        let enabled = crate::api::user_settings::get_user_setting(&pool, &user_id, "outbound_webhook_enabled")
            .await
            .map(|s| s == "true" || s == "1")
            .unwrap_or(false);

        if !enabled {
            return;
        }

        let webhook_url = match crate::api::user_settings::get_user_setting(&pool, &user_id, "outbound_webhook_url").await {
            Some(u) if !u.trim().is_empty() => u.trim().to_string(),
            _ => return,
        };

        let webhook_type = crate::api::user_settings::get_user_setting(&pool, &user_id, "outbound_webhook_type")
            .await
            .unwrap_or_else(|| "auto".to_string());

        if let Err(e) = send_notification(&webhook_url, &webhook_type, &data).await {
            tracing::warn!(error = %e, url = %webhook_url, "Failed to send outbound webhook notification");
        }
    });
}

/// Sends formatted notification payload to the configured outbound webhook service
pub async fn send_notification(
    url: &str,
    wh_type: &str,
    data: &ScrobbleNotificationData,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let is_episode = data.media_type.eq_ignore_ascii_case("episode");
    let display_title = if is_episode {
        let show = data.grandparent_title.as_deref().unwrap_or("TV Show");
        let s = data.season_number.unwrap_or(1);
        let ep = data.episode_number.unwrap_or(1);
        format!("{} S{:02}E{:02} — {}", show, s, ep, data.title)
    } else if let Some(y) = data.year {
        format!("{} ({})", data.title, y)
    } else {
        data.title.clone()
    };

    let mut details = format!("Status: Completed ({}%)", data.progress_percent);
    if let Some(r) = data.user_rating {
        details.push_str(&format!("\n⭐ User Rating: {}/10", r));
    }

    let detected_type = if wh_type == "auto" || wh_type.is_empty() {
        if url.contains("discord.com") {
            "discord"
        } else if url.contains("telegram") {
            "telegram"
        } else if url.contains("ntfy") {
            "ntfy"
        } else if url.contains("gotify") {
            "gotify"
        } else {
            "generic"
        }
    } else {
        wh_type
    };

    match detected_type.to_lowercase().as_str() {
        "discord" => {
            let mut embed = serde_json::json!({
                "title": format!("🍿 Watched: {}", display_title),
                "description": details,
                "color": 0xe50914, // Trakt / BingeSync red
                "footer": { "text": "BingeSync Notification" },
                "timestamp": chrono::Utc::now().to_rfc3339(),
            });

            if let Some(ref thumb) = data.thumb_url {
                if thumb.starts_with("http://") || thumb.starts_with("https://") {
                    embed["thumbnail"] = serde_json::json!({ "url": thumb });
                }
            }

            let body = serde_json::json!({
                "embeds": [embed]
            });

            let resp = client.post(url).json(&body).send().await.map_err(|e| e.to_string())?;
            let status = resp.status();
            if !status.is_success() {
                let err_text = resp.text().await.unwrap_or_default();
                return Err(format!("Discord webhook failed (HTTP {}): {}", status, err_text));
            }
        }
        "telegram" => {
            let text = format!("🍿 *Finished Watching*\n*{display_title}*\n\n{details}");
            let body = serde_json::json!({
                "text": text,
                "parse_mode": "Markdown"
            });
            let resp = client.post(url).json(&body).send().await.map_err(|e| e.to_string())?;
            let status = resp.status();
            if !status.is_success() {
                let err_text = resp.text().await.unwrap_or_default();
                return Err(format!("Telegram webhook failed (HTTP {}): {}", status, err_text));
            }
        }
        "ntfy" => {
            let mut req = client.post(url)
                .header("Title", format!("🍿 Watched: {}", display_title))
                .header("Tags", "popcorn,movie_camera")
                .body(details);

            if let Some(ref thumb) = data.thumb_url {
                if thumb.starts_with("http://") || thumb.starts_with("https://") {
                    req = req.header("Attach", thumb.clone());
                }
            }

            let resp = req.send().await.map_err(|e| e.to_string())?;
            let status = resp.status();
            if !status.is_success() {
                let err_text = resp.text().await.unwrap_or_default();
                return Err(format!("ntfy notification failed (HTTP {}): {}", status, err_text));
            }
        }
        "gotify" => {
            let body = serde_json::json!({
                "title": format!("🍿 Watched: {}", display_title),
                "message": details,
                "priority": 5
            });
            let resp = client.post(url).json(&body).send().await.map_err(|e| e.to_string())?;
            let status = resp.status();
            if !status.is_success() {
                let err_text = resp.text().await.unwrap_or_default();
                return Err(format!("Gotify notification failed (HTTP {}): {}", status, err_text));
            }
        }
        _ => {
            // Generic JSON Webhook
            let body = serde_json::json!({
                "event": "scrobble.completed",
                "media_type": data.media_type,
                "title": data.title,
                "grandparent_title": data.grandparent_title,
                "season_number": data.season_number,
                "episode_number": data.episode_number,
                "year": data.year,
                "display_title": display_title,
                "progress_percent": data.progress_percent,
                "user_rating": data.user_rating,
                "thumbnail_url": data.thumb_url,
                "timestamp": chrono::Utc::now().to_rfc3339()
            });
            let resp = client.post(url).json(&body).send().await.map_err(|e| e.to_string())?;
            let status = resp.status();
            if !status.is_success() {
                let err_text = resp.text().await.unwrap_or_default();
                return Err(format!("Generic webhook failed (HTTP {}): {}", status, err_text));
            }
        }
    }

    Ok(())
}

/// Handler for testing user's outbound webhook: POST /api/v1/user/settings/test-outbound-webhook
pub async fn test_outbound_webhook_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(payload): Json<TestWebhookRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let url = match payload.webhook_url {
        Some(u) if !u.trim().is_empty() => u.trim().to_string(),
        _ => {
            match crate::api::user_settings::get_user_setting(&state.pool, &auth.user_id, "outbound_webhook_url").await {
                Some(u) if !u.trim().is_empty() => u.trim().to_string(),
                _ => return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "No outbound webhook URL provided or configured" })),
                )),
            }
        }
    };

    crate::api::ssrf::validate_integration_url(&url).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("Invalid webhook URL: {}", e) })),
        )
    })?;

    let wh_type = payload.webhook_type
        .unwrap_or_else(|| "auto".to_string());

    let test_data = ScrobbleNotificationData {
        title: "Inception".to_string(),
        grandparent_title: None,
        media_type: "movie".to_string(),
        season_number: None,
        episode_number: None,
        year: Some(2010),
        progress_percent: 100,
        thumb_url: Some("https://image.tmdb.org/t/p/w500/oYuLEt3zVCKq57qu2F8dT7NIa6f.jpg".to_string()),
        user_rating: Some(10),
    };

    match send_notification(&url, &wh_type, &test_data).await {
        Ok(_) => Ok(Json(serde_json::json!({
            "status": "success",
            "message": "Test notification sent successfully!"
        }))),
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e })),
        )),
    }
}
