use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlexWebhookPayload {
    pub event: String,
    pub user: Option<bool>,
    pub owner: Option<bool>,
    pub user_id: Option<serde_json::Value>,
    #[serde(rename = "Account")]
    pub account: Option<PlexAccount>,
    #[serde(rename = "Server")]
    pub server: Option<PlexServer>,
    #[serde(rename = "Player")]
    pub player: Option<PlexPlayer>,
    #[serde(rename = "Metadata")]
    pub metadata: Option<PlexMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlexAccount {
    pub id: Option<serde_json::Value>,
    pub thumb: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlexServer {
    pub title: Option<String>,
    pub uuid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlexPlayer {
    pub local: Option<bool>,
    #[serde(rename = "publicAddress")]
    pub public_address: Option<String>,
    pub title: Option<String>,
    pub uuid: Option<String>,
    pub product: Option<String>,
    pub platform: Option<String>,
    pub device: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlexMediaStream {
    #[serde(rename = "videoResolution")]
    pub video_resolution: Option<String>,
    #[serde(rename = "videoCodec")]
    pub video_codec: Option<String>,
    #[serde(rename = "audioCodec")]
    pub audio_codec: Option<String>,
    #[serde(rename = "audioChannels")]
    pub audio_channels: Option<serde_json::Value>,
    #[serde(rename = "videoProfile")]
    pub video_profile: Option<String>,
    #[serde(rename = "colorTransfer")]
    pub color_transfer: Option<String>,
    #[serde(rename = "doviProfile")]
    pub dovi_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlexGuidTag {
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlexMetadata {
    #[serde(rename = "librarySectionType")]
    pub library_section_type: Option<String>,
    #[serde(rename = "type")]
    pub media_type: Option<String>,
    pub subtype: Option<String>,
    #[serde(rename = "extraType")]
    pub extra_type: Option<serde_json::Value>,
    pub title: Option<String>,
    #[serde(rename = "grandparentTitle")]
    pub grandparent_title: Option<String>,
    #[serde(rename = "parentTitle")]
    pub parent_title: Option<String>,
    #[serde(rename = "grandparentThumb")]
    pub grandparent_thumb: Option<String>,
    #[serde(rename = "parentThumb")]
    pub parent_thumb: Option<String>,
    pub thumb: Option<String>,
    pub art: Option<String>,
    pub index: Option<i32>,
    #[serde(rename = "parentIndex")]
    pub parent_index: Option<i32>,
    pub year: Option<i32>,
    pub summary: Option<String>,
    #[serde(rename = "ratingKey")]
    pub rating_key: Option<String>,
    pub guid: Option<String>,
    pub duration: Option<i64>,
    #[serde(rename = "viewOffset")]
    pub view_offset: Option<i64>,
    pub studio: Option<String>,
    #[serde(rename = "contentRating")]
    pub content_rating: Option<String>,
    #[serde(rename = "Media")]
    pub media: Option<Vec<PlexMediaStream>>,
    #[serde(rename = "Guid")]
    pub guids: Option<Vec<PlexGuidTag>>,
    #[serde(rename = "Genre")]
    pub genre: Option<Vec<PlexTag>>,
    #[serde(rename = "Director")]
    pub director: Option<Vec<PlexTag>>,
    #[serde(rename = "Role")]
    pub role: Option<Vec<PlexTag>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlexTag {
    pub tag: Option<String>,
}

impl PlexMetadata {
    /// Determines whether this Plex metadata represents an auxiliary trailer, clip, extra, or cinema pre-roll.
    pub fn is_trailer_or_extra(&self) -> bool {
        // 1. Explicit subtype check (e.g. "trailer", "behindTheScenes", "deletedScene", "clip", "preroll")
        if let Some(ref st) = self.subtype {
            let st_low = st.trim().to_lowercase();
            if !st_low.is_empty() {
                return true;
            }
        }

        // 2. Explicit extraType check
        if self.extra_type.is_some() {
            return true;
        }

        // 3. Media type check: only "movie" and "episode" are valid main feature watches
        if let Some(ref mt) = self.media_type {
            let mt_low = mt.trim().to_lowercase();
            if mt_low == "clip" || mt_low == "trailer" || mt_low == "extra" || mt_low == "video" {
                return true;
            }
            if mt_low != "movie" && mt_low != "episode" {
                return true;
            }
        }

        // 4. Library section type check
        if let Some(ref lst) = self.library_section_type {
            let lst_low = lst.trim().to_lowercase();
            if lst_low == "clip" || lst_low == "trailer" {
                return true;
            }
        }

        // 5. Pre-roll title or GUID check
        if let Some(ref title) = self.title {
            let t_low = title.trim().to_lowercase();
            if t_low.contains("pre-roll") || t_low.contains("preroll") || t_low.starts_with("cinema intro") {
                return true;
            }
        }
        if let Some(ref guid) = self.guid {
            let g_low = guid.trim().to_lowercase();
            if g_low.starts_with("preroll") {
                return true;
            }
        }

        false
    }

    /// Resolves canonical IMDb ID (e.g. "tt1375666") from Plex GUIDs or main guid.
    pub fn resolved_imdb_id(&self) -> Option<String> {
        if let Some(ref list) = self.guids {
            for item in list {
                if let Some(ref gid) = item.id {
                    let g = gid.trim();
                    if g.starts_with("imdb://") {
                        let id = g.trim_start_matches("imdb://");
                        if id.starts_with("tt") {
                            return Some(id.to_string());
                        }
                    }
                }
            }
        }
        if let Some(ref g) = self.guid {
            if let Some(idx) = g.find("tt") {
                let sub = &g[idx..];
                let num_len = sub.chars().skip(2).take_while(|c| c.is_ascii_digit()).count();
                if num_len >= 5 {
                    return Some(sub[..2 + num_len].to_string());
                }
            }
        }
        None
    }

    /// Resolves technical stream specs: (resolution, hdr_format, audio_codec, audio_channels, video_codec)
    pub fn resolved_media_specs(&self) -> (Option<String>, Option<String>, Option<String>, Option<String>, Option<String>) {
        let stream = match self.media.as_ref().and_then(|m| m.first()) {
            Some(s) => s,
            None => return (None, None, None, None, None),
        };

        let resolution = stream.video_resolution.as_deref().and_then(|r| {
            let r_low = r.trim().to_lowercase();
            match r_low.as_str() {
                "4k" | "2160" | "uhd" => Some("4K".to_string()),
                "1080" => Some("1080p".to_string()),
                "720" => Some("720p".to_string()),
                "576" => Some("576p".to_string()),
                "480" | "sd" => Some("480p".to_string()),
                s if !s.is_empty() => Some(format!("{}p", s.trim_end_matches('p'))),
                _ => None,
            }
        });

        let hdr_format = {
            let has_dovi = stream.dovi_profile.is_some()
                || stream.video_profile.as_deref().map(|p| p.to_lowercase().contains("dovi") || p.to_lowercase().contains("dv")).unwrap_or(false);
            if has_dovi {
                Some("Dolby Vision".to_string())
            } else if let Some(ref ct) = stream.color_transfer {
                let ct_low = ct.to_lowercase();
                if ct_low.contains("smpte2084") || ct_low.contains("hdr") {
                    Some("HDR10".to_string())
                } else if ct_low.contains("arib-std-b67") || ct_low.contains("hlg") {
                    Some("HLG".to_string())
                } else {
                    None
                }
            } else {
                None
            }
        };

        let audio_codec = stream.audio_codec.as_deref().and_then(|ac| {
            let ac_low = ac.trim().to_lowercase();
            match ac_low.as_str() {
                "truehd" => Some("TrueHD".to_string()),
                "dca-ma" | "dts-hd ma" | "dtshd" => Some("DTS-HD MA".to_string()),
                "dca" | "dts" => Some("DTS".to_string()),
                "eac3" => Some("EAC3".to_string()),
                "ac3" => Some("AC3".to_string()),
                "aac" => Some("AAC".to_string()),
                "flac" => Some("FLAC".to_string()),
                "opus" => Some("Opus".to_string()),
                s if !s.is_empty() => Some(s.to_uppercase()),
                _ => None,
            }
        });

        let audio_channels = match &stream.audio_channels {
            Some(serde_json::Value::Number(n)) => match n.as_i64() {
                Some(8) => Some("7.1".to_string()),
                Some(6) => Some("5.1".to_string()),
                Some(2) => Some("2.0".to_string()),
                Some(1) => Some("1.0".to_string()),
                Some(other) => Some(format!("{}ch", other)),
                _ => None,
            },
            Some(serde_json::Value::String(s)) => {
                let s_trim = s.trim();
                if !s_trim.is_empty() { Some(s_trim.to_string()) } else { None }
            }
            _ => None,
        };

        let video_codec = stream.video_codec.as_deref().and_then(|vc| {
            let vc_low = vc.trim().to_lowercase();
            match vc_low.as_str() {
                "hevc" | "h265" => Some("HEVC".to_string()),
                "h264" | "avc" => Some("H.264".to_string()),
                "av1" => Some("AV1".to_string()),
                "vc1" => Some("VC-1".to_string()),
                "mpeg2video" | "mpeg2" => Some("MPEG-2".to_string()),
                s if !s.is_empty() => Some(s.to_uppercase()),
                _ => None,
            }
        });

        (resolution, hdr_format, audio_codec, audio_channels, video_codec)
    }
}
