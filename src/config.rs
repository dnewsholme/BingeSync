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
    pub base_path: String,
    pub image_cache_dir: String,
    pub tvdb_api_key: Option<String>,
    pub tvdb_pin: Option<String>,
    pub tvdb_base_url: String,
    pub scrobble_threshold_percent: i64,
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

        let tvdb_api_key = env::var("TVDB_API_KEY").ok().filter(|s| !s.trim().is_empty());
        let tvdb_pin = env::var("TVDB_PIN").ok().filter(|s| !s.trim().is_empty());
        let tvdb_base_url = env::var("TVDB_BASE_URL").unwrap_or_else(|_| "https://api4.thetvdb.com/v4".to_string());

        let image_cache_dir = env::var("IMAGE_CACHE_DIR").unwrap_or_else(|_| {
            if let Some(path_str) = database_url
                .strip_prefix("sqlite://")
                .or_else(|| database_url.strip_prefix("sqlite:"))
            {
                let clean_path = path_str.split('?').next().unwrap_or(path_str);
                if let Some(parent) = std::path::Path::new(clean_path).parent() {
                    if !parent.as_os_str().is_empty() {
                        return parent.join("images").to_string_lossy().to_string();
                    }
                }
            }
            "./data/images".to_string()
        });

        let scrobble_threshold_percent = env::var("SCROBBLE_THRESHOLD_PERCENT")
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(80);

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
            base_path,
            image_cache_dir,
            tvdb_api_key,
            tvdb_pin,
            tvdb_base_url,
            scrobble_threshold_percent,
        }
    }
}
