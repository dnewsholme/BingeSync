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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlexMetadata {
    #[serde(rename = "librarySectionType")]
    pub library_section_type: Option<String>,
    #[serde(rename = "type")]
    pub media_type: Option<String>,
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
