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

    #[serde(alias = "NotificationUsername", alias = "notificationUsername", alias = "Username", alias = "username")]
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

    #[serde(alias = "Item", alias = "item", default)]
    pub item: Option<serde_json::Value>,

    #[serde(alias = "User", alias = "user", default)]
    pub user: Option<serde_json::Value>,

    #[serde(alias = "PlaybackInfo", alias = "playbackInfo", default)]
    pub playback_info: Option<serde_json::Value>,
}

impl JellyfinWebhookPayload {
    pub fn resolved_name(&self) -> Option<String> {
        self.name.clone().or_else(|| {
            self.item.as_ref().and_then(|it| {
                it.get("Name")
                    .or_else(|| it.get("Title"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
        })
    }

    pub fn resolved_item_type(&self) -> Option<String> {
        self.item_type.clone().or_else(|| {
            self.item.as_ref().and_then(|it| {
                it.get("Type")
                    .or_else(|| it.get("ItemType"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
        })
    }

    pub fn resolved_series_name(&self) -> Option<String> {
        self.series_name.clone().or_else(|| {
            self.item.as_ref().and_then(|it| {
                it.get("SeriesName")
                    .or_else(|| it.get("GrandparentTitle"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
        })
    }

    pub fn resolved_season_number(&self) -> Option<i32> {
        self.season_number.or_else(|| {
            self.item.as_ref().and_then(|it| {
                it.get("SeasonNumber")
                    .or_else(|| it.get("ParentIndexNumber"))
                    .and_then(|v| v.as_i64().map(|n| n as i32))
            })
        })
    }

    pub fn resolved_episode_number(&self) -> Option<i32> {
        self.episode_number.or_else(|| {
            self.item.as_ref().and_then(|it| {
                it.get("EpisodeNumber")
                    .or_else(|| it.get("IndexNumber"))
                    .and_then(|v| v.as_i64().map(|n| n as i32))
            })
        })
    }

    pub fn resolved_year(&self) -> Option<i32> {
        self.year.or_else(|| {
            self.item.as_ref().and_then(|it| {
                it.get("ProductionYear")
                    .or_else(|| it.get("Year"))
                    .and_then(|v| v.as_i64().map(|n| n as i32))
            })
        })
    }

    pub fn resolved_overview(&self) -> Option<String> {
        self.overview.clone().or_else(|| {
            self.item.as_ref().and_then(|it| {
                it.get("Overview")
                    .or_else(|| it.get("Summary"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
        })
    }

    pub fn resolved_runtime_ticks(&self) -> Option<i64> {
        self.runtime_ticks.or_else(|| {
            self.item.as_ref().and_then(|it| {
                it.get("RunTimeTicks")
                    .or_else(|| it.get("DurationTicks"))
                    .and_then(|v| v.as_i64())
            })
        })
    }

    pub fn resolved_playback_position_ticks(&self) -> Option<i64> {
        self.playback_position_ticks.or_else(|| {
            self.playback_info.as_ref().and_then(|pb| {
                pb.get("PositionTicks")
                    .or_else(|| pb.get("PlaybackPositionTicks"))
                    .and_then(|v| v.as_i64())
            })
        })
    }

    pub fn resolved_notification_username(&self) -> Option<String> {
        self.notification_username.clone().or_else(|| {
            self.user.as_ref().and_then(|u| {
                u.get("Name")
                    .or_else(|| u.get("Username"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
        })
    }

    pub fn resolved_user_id(&self) -> Option<String> {
        self.user_id.clone().or_else(|| {
            self.user.as_ref().and_then(|u| {
                u.get("Id")
                    .or_else(|| u.get("UserId"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
        })
    }

    pub fn resolved_genres(&self) -> Option<String> {
        let g = self.genres.as_ref().or_else(|| self.item.as_ref().and_then(|it| it.get("Genres")));
        match g {
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
        let p_ids = self.provider_ids.as_ref().or_else(|| self.item.as_ref().and_then(|it| it.get("ProviderIds")));
        if let Some(serde_json::Value::Object(map)) = p_ids {
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
        let p_ids = self.provider_ids.as_ref().or_else(|| self.item.as_ref().and_then(|it| it.get("ProviderIds")));
        if let Some(serde_json::Value::Object(map)) = p_ids {
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
        let p_ids = self.provider_ids.as_ref().or_else(|| self.item.as_ref().and_then(|it| it.get("ProviderIds")));
        if let Some(serde_json::Value::Object(map)) = p_ids {
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
        let item_id = self.item_id.as_deref().or_else(|| {
            self.item.as_ref().and_then(|it| it.get("Id").and_then(|v| v.as_str()))
        });
        if let (Some(server), Some(id)) = (&self.server_url, item_id) {
            if !server.trim().is_empty() && !id.trim().is_empty() {
                let base = server.trim_end_matches('/');
                return Some(format!("{}/Items/{}/Images/Primary", base, id));
            }
        }
        None
    }
}
