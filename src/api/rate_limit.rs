use axum::{
    extract::Request,
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct RateLimiter {
    // Map of client identifier -> list of request timestamps
    requests: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
    max_requests: usize,
    window: Duration,
    enabled: bool,
}

impl RateLimiter {
    pub fn new(max_requests: usize, window: Duration) -> Self {
        let enabled = std::env::var("RATE_LIMIT_ENABLED")
            .map(|v| !v.eq_ignore_ascii_case("false") && v != "0")
            .unwrap_or(true);

        Self {
            requests: Arc::new(Mutex::new(HashMap::new())),
            max_requests,
            window,
            enabled,
        }
    }

    /// Checks if a client IP is allowed. Returns Ok(()) if permitted, or Err(retry_after_secs) if rate limited.
    pub fn check(&self, client_id: &str) -> Result<(), u64> {
        if !self.enabled {
            return Ok(());
        }

        let now = Instant::now();
        let mut map = self.requests.lock().unwrap();

        // Periodic pruning if map grows large (> 1000 items)
        if map.len() > 1000 {
            let cutoff = now.checked_sub(self.window).unwrap_or(now);
            map.retain(|_, timestamps| {
                timestamps.retain(|&ts| ts > cutoff);
                !timestamps.is_empty()
            });
        }

        let timestamps = map.entry(client_id.to_string()).or_default();
        let cutoff = now.checked_sub(self.window).unwrap_or(now);
        timestamps.retain(|&ts| ts > cutoff);

        if timestamps.len() >= self.max_requests {
            let oldest = timestamps.first().copied().unwrap_or(now);
            let elapsed = now.duration_since(oldest);
            let retry_after = self.window.saturating_sub(elapsed).as_secs().max(1);
            return Err(retry_after);
        }

        timestamps.push(now);
        Ok(())
    }
}

pub fn extract_client_ip(headers: &HeaderMap) -> String {
    // 1. Check X-Forwarded-For
    if let Some(forwarded) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if let Some(client_ip) = forwarded.split(',').next() {
            let trimmed = client_ip.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }

    // 2. Check X-Real-IP
    if let Some(real_ip) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        let trimmed = real_ip.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    // 3. Fallback
    "127.0.0.1".to_string()
}

pub async fn auth_rate_limit_middleware(req: Request, next: Next) -> Response {
    static LIMITER: OnceLock<RateLimiter> = OnceLock::new();
    let limiter = LIMITER.get_or_init(|| RateLimiter::new(240, Duration::from_secs(60)));
    apply_limiter(limiter, req, next).await
}

pub async fn webhook_rate_limit_middleware(req: Request, next: Next) -> Response {
    static LIMITER: OnceLock<RateLimiter> = OnceLock::new();
    let limiter = LIMITER.get_or_init(|| RateLimiter::new(300, Duration::from_secs(60)));
    apply_limiter(limiter, req, next).await
}

pub async fn import_rate_limit_middleware(req: Request, next: Next) -> Response {
    static LIMITER: OnceLock<RateLimiter> = OnceLock::new();
    let limiter = LIMITER.get_or_init(|| RateLimiter::new(60, Duration::from_secs(60)));
    apply_limiter(limiter, req, next).await
}

async fn apply_limiter(limiter: &RateLimiter, req: Request, next: Next) -> Response {
    let client_ip = extract_client_ip(req.headers());

    match limiter.check(&client_ip) {
        Ok(()) => next.run(req).await,
        Err(retry_after) => (
            StatusCode::TOO_MANY_REQUESTS,
            [
                ("Retry-After", retry_after.to_string()),
                ("Content-Type", "application/json".to_string()),
            ],
            Json(serde_json::json!({
                "error": "Too many requests. Please slow down.",
                "retry_after_seconds": retry_after
            })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiter_allows_up_to_max() {
        let limiter = RateLimiter::new(3, Duration::from_secs(60));
        assert!(limiter.check("1.2.3.4").is_ok());
        assert!(limiter.check("1.2.3.4").is_ok());
        assert!(limiter.check("1.2.3.4").is_ok());
        assert!(limiter.check("1.2.3.4").is_err());
        // Different IP is not affected
        assert!(limiter.check("5.6.7.8").is_ok());
    }
}
