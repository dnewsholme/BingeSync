use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JellyfinWebhookPayload {
    #[serde(alias = "NotificationType", alias = "notificationType", alias = "Event", alias = "event", alias = "event_type")]
    pub notification_type: Option<String>,

    #[serde(alias = "ItemType", alias = "itemType", alias = "Type", alias = "type")]
    pub item_type: Option<String>,

    #[serde(alias = "Name", alias = "name", alias = "Title", alias = "title")]
    pub name: Option<String>,

    #[serde(alias = "SeriesName", alias = "seriesName", alias = "GrandparentTitle", alias = "grandparentTitle")]
    pub series_name: Option<String>,

    #[serde(alias = "SeasonNumber", alias = "seasonNumber", alias = "ParentIndexNumber", alias = "parentIndexNumber")]
    pub season_number: Option<i32>,

    #[serde(alias = "EpisodeNumber", alias = "episodeNumber", alias = "IndexNumber", alias = "indexNumber")]
    pub episode_number: Option<i32>,

    #[serde(alias = "Year", alias = "year", alias = "ProductionYear", alias = "productionYear")]
    pub year: Option<i32>,

    #[serde(alias = "Overview", alias = "overview", alias = "Description", alias = "description", alias = "Summary", alias = "summary")]
    pub overview: Option<String>,

    #[serde(alias = "RunTimeTicks", alias = "runTimeTicks", alias = "DurationTicks", alias = "durationTicks")]
    pub runtime_ticks: Option<i64>,

    #[serde(alias = "PlaybackPositionTicks", alias = "playbackPositionTicks", alias = "PositionTicks", alias = "positionTicks")]
    pub playback_position_ticks: Option<i64>,

    #[serde(alias = "PlayedToCompletion", alias = "playedToCompletion")]
    pub played_to_completion: Option<bool>,

    #[serde(alias = "Played", alias = "played")]
    pub played: Option<bool>,

    #[serde(alias = "Genres", alias = "genres", default)]
    pub genres: Option<serde_json::Value>,

    #[serde(alias = "UserId", alias = "userId")]
    pub user_id: Option<String>,

    #[serde(alias = "NotificationUsername", alias = "notificationUsername", alias = "Username", alias = "username", alias = "User", alias = "user")]
    pub notification_username: Option<String>,

    #[serde(alias = "Provider_imdb", alias = "Provider_Imdb", alias = "Provider_IMDB", alias = "imdb_id", alias = "ImdbId")]
    pub provider_imdb: Option<String>,

    #[serde(alias = "Provider_tvdb", alias = "Provider_Tvdb", alias = "Provider_TVDB", alias = "tvdb_id", alias = "TvdbId")]
    pub provider_tvdb: Option<String>,

    #[serde(alias = "Provider_tmdb", alias = "Provider_Tmdb", alias = "Provider_TMDB", alias = "tmdb_id", alias = "TmdbId")]
    pub provider_tmdb: Option<String>,

    #[serde(alias = "ProviderIds", alias = "providerIds")]
    pub provider_ids: Option<serde_json::Value>,

    #[serde(alias = "ItemId", alias = "itemId", alias = "Id", alias = "id")]
    pub item_id: Option<String>,

    #[serde(alias = "ImageUrl", alias = "imageUrl", alias = "PosterUrl", alias = "posterUrl")]
    pub image_url: Option<String>,

    #[serde(alias = "ServerUrl", alias = "serverUrl")]
    pub server_url: Option<String>,

    #[serde(alias = "DeviceName", alias = "deviceName")]
    pub device_name: Option<String>,

    #[serde(alias = "Client", alias = "client", alias = "ClientName", alias = "clientName")]
    pub client: Option<String>,
}

impl JellyfinWebhookPayload {
    pub fn resolved_genres(&self) -> Option<String> {
        match &self.genres {
            Some(serde_json::Value::Array(arr)) => {
                let tags: Vec<String> = arr
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                    .filter(|s| !s.is_empty())
                    .collect();
                if tags.is_empty() {
                    None
                } else {
                    Some(tags.join(", "))
                }
            }
            Some(serde_json::Value::String(s)) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }
            _ => None,
        }
    }

    pub fn resolved_imdb_id(&self) -> Option<String> {
        if let Some(ref id) = self.provider_imdb {
            if !id.trim().is_empty() {
                return Some(id.trim().to_string());
            }
        }
        if let Some(serde_json::Value::Object(map)) = &self.provider_ids {
            for key in ["Imdb", "imdb", "IMDB"] {
                if let Some(serde_json::Value::String(s)) = map.get(key) {
                    if !s.trim().is_empty() {
                        return Some(s.trim().to_string());
                    }
                }
            }
        }
        None
    }

    pub fn resolved_tvdb_id(&self) -> Option<String> {
        if let Some(ref id) = self.provider_tvdb {
            if !id.trim().is_empty() {
                return Some(id.trim().to_string());
            }
        }
        if let Some(serde_json::Value::Object(map)) = &self.provider_ids {
            for key in ["Tvdb", "tvdb", "TVDB"] {
                if let Some(serde_json::Value::String(s)) = map.get(key) {
                    if !s.trim().is_empty() {
                        return Some(s.trim().to_string());
                    }
                }
                if let Some(serde_json::Value::Number(n)) = map.get(key) {
                    return Some(n.to_string());
                }
            }
        }
        None
    }

    pub fn resolved_tmdb_id(&self) -> Option<String> {
        if let Some(ref id) = self.provider_tmdb {
            if !id.trim().is_empty() {
                return Some(id.trim().to_string());
            }
        }
        if let Some(serde_json::Value::Object(map)) = &self.provider_ids {
            for key in ["Tmdb", "tmdb", "TMDB"] {
                if let Some(serde_json::Value::String(s)) = map.get(key) {
                    if !s.trim().is_empty() {
                        return Some(s.trim().to_string());
                    }
                }
                if let Some(serde_json::Value::Number(n)) = map.get(key) {
                    return Some(n.to_string());
                }
            }
        }
        None
    }

    pub fn resolved_thumb_url(&self) -> Option<String> {
        if let Some(ref url) = self.image_url {
            if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("/api/v1/images/") {
                return Some(url.clone());
            }
        }
        if let (Some(server), Some(item_id)) = (&self.server_url, &self.item_id) {
            if !server.trim().is_empty() && !item_id.trim().is_empty() {
                let base = server.trim_end_matches('/');
                return Some(format!("{}/Items/{}/Images/Primary", base, item_id));
            }
        }
        None
    }
}
