use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub oidc_sub: Option<String>,
    pub webhook_token: String,
    pub avatar_url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserResponse {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub webhook_token: String,
    pub avatar_url: Option<String>,
    pub webhook_url: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AdminUserItem {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub webhook_token: String,
    pub avatar_url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub scrobble_count: i64,
}

impl User {
    pub fn to_response(&self, app_url: &str) -> UserResponse {
        let trimmed_app = app_url.trim_end_matches('/');
        UserResponse {
            id: self.id.clone(),
            email: self.email.clone(),
            display_name: self.display_name.clone(),
            role: self.role.clone(),
            webhook_token: self.webhook_token.clone(),
            avatar_url: self.avatar_url.clone(),
            webhook_url: format!("{}/api/v1/webhooks/plex/{}", trimmed_app, self.webhook_token),
            created_at: self.created_at.clone(),
        }
    }
}

