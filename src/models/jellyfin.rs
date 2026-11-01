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

    pub fn resolved_client(&self) -> Option<String> {
        self.client.clone().or_else(|| {
            self.playback_info.as_ref().and_then(|pb| {
                pb.get("Client")
                    .or_else(|| pb.get("ClientName"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_string())
            })
        })
    }

    pub fn resolved_device_name(&self) -> Option<String> {
        self.device_name.clone().or_else(|| {
            self.playback_info.as_ref().and_then(|pb| {
                pb.get("DeviceName")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_string())
            })
        })
    }

    pub fn resolved_platform(&self) -> Option<String> {
        self.playback_info.as_ref().and_then(|pb| {
            pb.get("DeviceProfile")
                .or_else(|| pb.get("Platform"))
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
        }).or_else(|| {
            self.item.as_ref().and_then(|it| {
                it.get("Platform").and_then(|v| v.as_str()).map(|s| s.trim().to_string())
            })
        })
    }

    pub fn resolved_is_local(&self) -> Option<bool> {
        self.playback_info.as_ref().and_then(|pb| {
            pb.get("IsLocal")
                .or_else(|| pb.get("IsDirect"))
                .and_then(|v| v.as_bool())
        })
    }

    pub fn resolved_studio(&self) -> Option<String> {
        self.item.as_ref().and_then(|it| {
            if let Some(arr) = it.get("Studios").and_then(|v| v.as_array()) {
                arr.first().and_then(|s| {
                    s.get("Name").or(Some(s)).and_then(|v| v.as_str()).map(|n| n.trim().to_string())
                })
            } else if let Some(s) = it.get("Studio").and_then(|v| v.as_str()) {
                Some(s.trim().to_string())
            } else {
                None
            }
        })
    }

    pub fn resolved_original_language(&self) -> Option<String> {
        self.item.as_ref().and_then(|it| {
            it.get("OriginalLanguage")
                .or_else(|| it.get("Language"))
                .or_else(|| it.get("PreferredMetadataLanguage"))
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_lowercase())
        })
    }

    pub fn resolved_content_rating(&self) -> Option<String> {
        self.item.as_ref().and_then(|it| {
            it.get("OfficialRating")
                .or_else(|| it.get("CustomRating"))
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
        })
    }

    pub fn resolved_media_specs(&self) -> (Option<String>, Option<String>, Option<String>, Option<String>, Option<String>) {
        let streams = self.item.as_ref().and_then(|it| {
            it.get("MediaStreams")
                .or_else(|| it.get("MediaSources").and_then(|arr| arr.as_array()?.first()?.get("MediaStreams")))
                .and_then(|v| v.as_array())
        });

        let mut resolution = None;
        let mut hdr_format = None;
        let mut audio_codec = None;
        let mut audio_channels = None;
        let mut video_codec = None;

        if let Some(arr) = streams {
            for s in arr {
                let stype = s.get("Type").and_then(|v| v.as_str()).unwrap_or_default().to_lowercase();
                if stype == "video" && video_codec.is_none() {
                    let width = s.get("Width").and_then(|v| v.as_i64()).unwrap_or(0);
                    let height = s.get("Height").and_then(|v| v.as_i64()).unwrap_or(0);
                    resolution = if width >= 3800 || height >= 2000 {
                        Some("4K".to_string())
                    } else if width >= 1900 || height >= 1000 {
                        Some("1080p".to_string())
                    } else if width >= 1200 || height >= 700 {
                        Some("720p".to_string())
                    } else if width > 0 || height > 0 {
                        Some("480p".to_string())
                    } else {
                        None
                    };

                    let vcodec_raw = s.get("Codec").and_then(|v| v.as_str()).unwrap_or_default().to_lowercase();
                    video_codec = match vcodec_raw.as_str() {
                        "hevc" | "h265" => Some("HEVC".to_string()),
                        "h264" | "avc" => Some("H.264".to_string()),
                        "av1" => Some("AV1".to_string()),
                        "vc1" => Some("VC-1".to_string()),
                        "mpeg2video" | "mpeg2" => Some("MPEG-2".to_string()),
                        other if !other.is_empty() => Some(other.to_uppercase()),
                        _ => None,
                    };

                    let vrange = s.get("VideoRange").and_then(|v| v.as_str()).unwrap_or_default().to_lowercase();
                    hdr_format = if vrange.contains("dovi") || vrange.contains("dolby vision") {
                        Some("Dolby Vision".to_string())
                    } else if vrange.contains("hdr10") || vrange == "hdr" {
                        Some("HDR10".to_string())
                    } else if vrange.contains("hlg") {
                        Some("HLG".to_string())
                    } else {
                        None
                    };
                } else if stype == "audio" && audio_codec.is_none() {
                    let acodec_raw = s.get("Codec").and_then(|v| v.as_str()).unwrap_or_default().to_lowercase();
                    audio_codec = match acodec_raw.as_str() {
                        "truehd" => Some("TrueHD".to_string()),
                        "dts-hd" | "dtshd" | "dca-ma" => Some("DTS-HD MA".to_string()),
                        "dts" | "dca" => Some("DTS".to_string()),
                        "eac3" => Some("EAC3".to_string()),
                        "ac3" => Some("AC3".to_string()),
                        "aac" => Some("AAC".to_string()),
                        "flac" => Some("FLAC".to_string()),
                        "opus" => Some("Opus".to_string()),
                        other if !other.is_empty() => Some(other.to_uppercase()),
                        _ => None,
                    };

                    let channels = s.get("Channels").and_then(|v| v.as_i64()).unwrap_or(0);
                    audio_channels = match channels {
                        8 => Some("7.1".to_string()),
                        6 => Some("5.1".to_string()),
                        2 => Some("2.0".to_string()),
                        1 => Some("1.0".to_string()),
                        other if other > 0 => Some(format!("{}ch", other)),
                        _ => None,
                    };
                }
            }
        }

        (resolution, hdr_format, audio_codec, audio_channels, video_codec)
    }
}
