use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub app_url: String,
    pub jwt_secret: String,
    pub oidc_enabled: bool,
    pub oidc_issuer_url: Option<String>,
    pub oidc_client_id: Option<String>,
    pub oidc_client_secret: Option<String>,
    pub oidc_redirect_url: Option<String>,
    pub dev_login_enabled: bool,
    pub radarr_url: Option<String>,
    pub radarr_api_key: Option<String>,
    pub sonarr_url: Option<String>,
    pub sonarr_api_key: Option<String>,
    pub base_path: String,
}

impl Config {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();

        let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8080);

        let database_url = env::var("DATABASE_URL")
            .unwrap_or_else(|_| "sqlite://bingesync.db?mode=rwc".to_string());

        let app_url = env::var("APP_URL")
            .or_else(|_| env::var("BINGESYNC_BASE_URL"))
            .unwrap_or_else(|_| format!("http://localhost:{}", port));

        let raw_base = env::var("BASE_PATH")
            .or_else(|_| env::var("SUBPATH"))
            .ok()
            .or_else(|| {
                if let Ok(parsed) = url::Url::parse(&app_url) {
                    let p = parsed.path().trim_end_matches('/').to_string();
                    if !p.is_empty() && p != "/" {
                        Some(p)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .unwrap_or_default();

        let base_path = if raw_base.is_empty() || raw_base == "/" {
            String::new()
        } else if !raw_base.starts_with('/') {
            format!("/{}", raw_base.trim_end_matches('/'))
        } else {
            raw_base.trim_end_matches('/').to_string()
        };

        let jwt_secret = env::var("JWT_SECRET")
            .unwrap_or_else(|_| "bingesync-super-secret-key-change-in-production-123456789".to_string());

        let oidc_issuer_url = env::var("OIDC_ISSUER_URL").ok().filter(|s| !s.trim().is_empty());
        let oidc_client_id = env::var("OIDC_CLIENT_ID").ok().filter(|s| !s.trim().is_empty());
        let oidc_client_secret = env::var("OIDC_CLIENT_SECRET").ok().filter(|s| !s.trim().is_empty());
        let oidc_redirect_url = env::var("OIDC_REDIRECT_URL").ok().filter(|s| !s.trim().is_empty())
            .or_else(|| Some(format!("{}/api/v1/auth/callback", app_url)));

        let oidc_enabled = env::var("OIDC_ENABLED")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or_else(|_| oidc_issuer_url.is_some() && oidc_client_id.is_some());

        // Dev login is enabled if explicitly true, or automatically when OIDC is not enabled
        let dev_login_enabled = env::var("DEV_LOGIN_ENABLED")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(!oidc_enabled);

        let radarr_url = env::var("RADARR_URL").ok().filter(|s| !s.trim().is_empty());
        let radarr_api_key = env::var("RADARR_API_KEY").ok().filter(|s| !s.trim().is_empty());
        let sonarr_url = env::var("SONARR_URL").ok().filter(|s| !s.trim().is_empty());
        let sonarr_api_key = env::var("SONARR_API_KEY").ok().filter(|s| !s.trim().is_empty());

        Self {
            host,
            port,
            database_url,
            app_url,
            jwt_secret,
            oidc_enabled,
            oidc_issuer_url,
            oidc_client_id,
            oidc_client_secret,
            oidc_redirect_url,
            dev_login_enabled,
            radarr_url,
            radarr_api_key,
            sonarr_url,
            sonarr_api_key,
            base_path,
        }
    }
}
