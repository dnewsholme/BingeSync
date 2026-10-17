use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcDiscovery {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub userinfo_endpoint: Option<String>,
    pub jwks_uri: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcTokenResponse {
    pub access_token: String,
    pub token_type: Option<String>,
    pub id_token: Option<String>,
    pub expires_in: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OidcUserInfo {
    pub sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub preferred_username: Option<String>,
    pub picture: Option<String>,
    #[serde(default)]
    pub groups: serde_json::Value,
    #[serde(default)]
    pub roles: serde_json::Value,
}

impl OidcUserInfo {
    pub fn is_admin(&self) -> bool {
        let mut extracted: Vec<String> = Vec::new();

        let mut extract = |val: &serde_json::Value| {
            match val {
                serde_json::Value::Array(arr) => {
                    for item in arr {
                        match item {
                            serde_json::Value::String(s) => extracted.push(s.clone()),
                            serde_json::Value::Object(map) => {
                                if let Some(serde_json::Value::String(name)) = map.get("name") {
                                    extracted.push(name.clone());
                                } else if let Some(serde_json::Value::String(slug)) = map.get("slug") {
                                    extracted.push(slug.clone());
                                }
                            }
                            _ => {}
                        }
                    }
                }
                serde_json::Value::String(s) => {
                    for part in s.split([',', ' ', ';']) {
                        let trimmed = part.trim();
                        if !trimmed.is_empty() {
                            extracted.push(trimmed.to_string());
                        }
                    }
                }
                _ => {}
            }
        };

        extract(&self.groups);
        extract(&self.roles);

        let check = |s: &str| {
            let l = s.trim().to_lowercase();
            l == "admin" || l == "admins" || l == "administrator" || l == "administrators"
        };

        extracted.iter().any(|g| check(g))
    }
}

pub struct OidcClient {
    http: Client,
    issuer_url: String,
    client_id: String,
    client_secret: Option<String>,
    redirect_uri: String,
    discovery: tokio::sync::RwLock<Option<OidcDiscovery>>,
}

impl OidcClient {
    pub fn new(
        issuer_url: String,
        client_id: String,
        client_secret: Option<String>,
        redirect_uri: String,
    ) -> Self {
        Self {
            http: Client::builder().build().unwrap_or_default(),
            issuer_url,
            client_id,
            client_secret,
            redirect_uri,
            discovery: tokio::sync::RwLock::new(None),
        }
    }

    pub async fn get_discovery(&self) -> Result<OidcDiscovery, String> {
        {
            let read_lock = self.discovery.read().await;
            if let Some(disc) = &*read_lock {
                return Ok(disc.clone());
            }
        }

        let well_known = format!(
            "{}/.well-known/openid-configuration",
            self.issuer_url.trim_end_matches('/')
        );

        let res = self
            .http
            .get(&well_known)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch OIDC discovery from {}: {}", well_known, e))?;

        if !res.status().is_success() {
            return Err(format!(
                "OIDC discovery returned HTTP {}: {}",
                res.status(),
                well_known
            ));
        }

        let disc: OidcDiscovery = res
            .json()
            .await
            .map_err(|e| format!("Failed to parse OIDC discovery JSON: {}", e))?;

        let mut write_lock = self.discovery.write().await;
        *write_lock = Some(disc.clone());
        Ok(disc)
    }

    pub async fn generate_auth_url(&self, pool: &SqlitePool) -> Result<String, String> {
        let disc = self.get_discovery().await?;

        // Generate random bytes for state, nonce, and code_verifier
        let mut rand_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut rand_bytes);
        let state = URL_SAFE_NO_PAD.encode(&rand_bytes);

        rand::thread_rng().fill_bytes(&mut rand_bytes);
        let nonce = URL_SAFE_NO_PAD.encode(&rand_bytes);

        rand::thread_rng().fill_bytes(&mut rand_bytes);
        let code_verifier = URL_SAFE_NO_PAD.encode(&rand_bytes);

        // PKCE: code_challenge = base64url(sha256(code_verifier))
        let mut hasher = Sha256::new();
        hasher.update(code_verifier.as_bytes());
        let challenge_hash = hasher.finalize();
        let code_challenge = URL_SAFE_NO_PAD.encode(&challenge_hash);

        // Store state, nonce, and code_verifier in DB
        sqlx::query(
            "INSERT INTO oidc_states (state, nonce, code_verifier, redirect_uri) VALUES (?, ?, ?, ?)"
        )
        .bind(&state)
        .bind(&nonce)
        .bind(&code_verifier)
        .bind(&self.redirect_uri)
        .execute(pool)
        .await
        .map_err(|e| format!("Database error saving OIDC state: {}", e))?;

        let mut auth_url = url::Url::parse(&disc.authorization_endpoint)
            .map_err(|e| format!("Invalid authorization_endpoint URL: {}", e))?;

        auth_url.query_pairs_mut()
            .append_pair("client_id", &self.client_id)
            .append_pair("response_type", "code")
            .append_pair("scope", "openid email profile groups")
            .append_pair("redirect_uri", &self.redirect_uri)
            .append_pair("state", &state)
            .append_pair("nonce", &nonce)
            .append_pair("code_challenge", &code_challenge)
            .append_pair("code_challenge_method", "S256");

        Ok(auth_url.to_string())
    }

    pub async fn handle_callback(
        &self,
        pool: &SqlitePool,
        code: &str,
        state: &str,
    ) -> Result<OidcUserInfo, String> {
        let disc = self.get_discovery().await?;

        // Retrieve stored state
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT code_verifier, redirect_uri FROM oidc_states WHERE state = ?"
        )
        .bind(state)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("Database query error on oidc_states: {}", e))?;

        let (code_verifier, redirect_uri) = match row {
            Some(data) => data,
            None => return Err("Invalid or expired state parameter".to_string()),
        };

        // Clean up consumed state
        let _ = sqlx::query("DELETE FROM oidc_states WHERE state = ?")
            .bind(state)
            .execute(pool)
            .await;

        // Exchange authorization code for token
        let mut form_params = vec![
            ("grant_type", "authorization_code".to_string()),
            ("code", code.to_string()),
            ("redirect_uri", redirect_uri),
            ("client_id", self.client_id.clone()),
            ("code_verifier", code_verifier),
        ];

        if let Some(ref secret) = self.client_secret {
            form_params.push(("client_secret", secret.clone()));
        }

        let token_res = self
            .http
            .post(&disc.token_endpoint)
            .form(&form_params)
            .send()
            .await
            .map_err(|e| format!("Failed to request token from OIDC provider: {}", e))?;

        if !token_res.status().is_success() {
            let status = token_res.status();
            let body = token_res.text().await.unwrap_or_default();
            return Err(format!(
                "Token endpoint returned HTTP {}: {}",
                status, body
            ));
        }

        let token_payload: OidcTokenResponse = token_res
            .json()
            .await
            .map_err(|e| format!("Failed to parse token response JSON: {}", e))?;

        let mut userinfo_opt: Option<OidcUserInfo> = None;

        // If userinfo_endpoint is available, fetch userinfo with access_token
        if let Some(userinfo_url) = disc.userinfo_endpoint {
            let userinfo_res = self
                .http
                .get(&userinfo_url)
                .bearer_auth(&token_payload.access_token)
                .send()
                .await;

            if let Ok(resp) = userinfo_res {
                if resp.status().is_success() {
                    if let Ok(info) = resp.json::<OidcUserInfo>().await {
                        userinfo_opt = Some(info);
                    }
                }
            }
        }

        // Also parse claims from id_token if present (e.g. PocketID groups)
        let mut id_token_info: Option<OidcUserInfo> = None;
        if let Some(id_token) = token_payload.id_token {
            let parts: Vec<&str> = id_token.split('.').collect();
            if parts.len() >= 2 {
                if let Ok(decoded_bytes) = URL_SAFE_NO_PAD.decode(parts[1]) {
                    if let Ok(info) = serde_json::from_slice::<OidcUserInfo>(&decoded_bytes) {
                        id_token_info = Some(info);
                    }
                }
            }
        }

        match (userinfo_opt, id_token_info) {
            (Some(mut u), Some(id)) => {
                if (u.groups.is_null() || u.groups == serde_json::Value::Array(vec![])) && !id.groups.is_null() {
                    u.groups = id.groups;
                }
                if (u.roles.is_null() || u.roles == serde_json::Value::Array(vec![])) && !id.roles.is_null() {
                    u.roles = id.roles;
                }
                Ok(u)
            }
            (Some(u), None) => Ok(u),
            (None, Some(id)) => Ok(id),
            (None, None) => Err("Could not retrieve user info from OIDC provider".to_string()),
        }
    }
}
