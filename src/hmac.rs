use crate::AppState;
use axum::extract::State;
use axum::{
    body::Body,
    extract::{ConnectInfo, Request},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;

type HmacSha256 = Hmac<Sha256>;
const MAX_HMAC_BODY_BYTES: usize = 30 * 1024 * 1024;

// Rate limiter
#[derive(Clone)]
pub struct RateLimiter {
    requests: Arc<RwLock<HashMap<String, Vec<Instant>>>>,
    max_requests: usize,
    window: Duration,
}

impl RateLimiter {
    pub fn new(max_requests: usize, window: Duration) -> Self {
        Self {
            requests: Arc::new(RwLock::new(HashMap::new())),
            max_requests,
            window,
        }
    }

    async fn check(&self, key: &str) -> bool {
        let mut requests = self.requests.write().await;
        let now = Instant::now();

        let entry = requests.entry(key.to_string()).or_insert_with(Vec::new);
        entry.retain(|&time| now.duration_since(time) < self.window);

        if entry.len() < self.max_requests {
            entry.push(now);
            true
        } else {
            false
        }
    }
}

pub async fn rate_limit_middleware(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(limiter): State<RateLimiter>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let ip = addr.ip().to_string();

    if !limiter.check(&ip).await {
        tracing::warn!("Rate limit exceeded for IP: {}", ip);
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    Ok(next.run(req).await)
}
pub async fn verify_signature(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let (parts, body) = req.into_parts();
    let bytes = axum::body::to_bytes(body, MAX_HMAC_BODY_BYTES)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    verify_signature_bytes(&state.ai_config.agent_secret, &parts.headers, &bytes)?;

    let req = Request::from_parts(parts, Body::from(bytes));
    Ok(next.run(req).await)
}

pub async fn verify_signature_when_present(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let (parts, body) = req.into_parts();
    let has_timestamp = parts.headers.contains_key("X-Timestamp");
    let has_signature = parts.headers.contains_key("X-Signature");
    let bytes = axum::body::to_bytes(body, MAX_HMAC_BODY_BYTES)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if has_timestamp || has_signature {
        verify_signature_bytes(&state.ai_config.agent_secret, &parts.headers, &bytes)?;
    } else {
        tracing::warn!(
            method = %parts.method,
            path = %parts.uri.path(),
            "Unsigned legacy agent request accepted temporarily; add HMAC in admin boundary pass"
        );
    }

    let req = Request::from_parts(parts, Body::from(bytes));
    Ok(next.run(req).await)
}

fn verify_signature_bytes(
    agent_secret: &str,
    headers: &HeaderMap,
    bytes: &[u8],
) -> Result<(), StatusCode> {
    let timestamp = headers
        .get("X-Timestamp")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<i64>().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let provided_signature = headers
        .get("X-Signature")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Check timestamp freshness (within 1 minute)
    let now = chrono::Utc::now().timestamp();
    if (now - timestamp).abs() > 60 {
        tracing::warn!("Timestamp out of range");
        return Err(StatusCode::UNAUTHORIZED);
    }

    let provided_signature_bytes = hex::decode(provided_signature).map_err(|e| {
        tracing::warn!("Invalid signature format: {}", e);
        StatusCode::UNAUTHORIZED
    })?;

    // Create HMAC
    let mut mac = HmacSha256::new_from_slice(agent_secret.as_bytes())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    mac.update(timestamp.to_string().as_bytes());
    mac.update(&bytes);

    // Using of embed constant-time verification to prevent timing attacks
    mac.verify_slice(&provided_signature_bytes).map_err(|_| {
        tracing::warn!("HMAC verification failed");
        StatusCode::UNAUTHORIZED
    })?;

    tracing::debug!("HMAC signature verified successfully");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn signed_headers(secret: &str, timestamp: i64, body: &[u8]) -> HeaderMap {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(timestamp.to_string().as_bytes());
        mac.update(body);

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Timestamp",
            HeaderValue::from_str(&timestamp.to_string()).unwrap(),
        );
        headers.insert(
            "X-Signature",
            HeaderValue::from_str(&hex::encode(mac.finalize().into_bytes())).unwrap(),
        );
        headers
    }

    #[test]
    fn verify_signature_bytes_accepts_valid_signature() {
        let secret = "demo-secret";
        let body = br#"{"message":"hello"}"#;
        let headers = signed_headers(secret, chrono::Utc::now().timestamp(), body);

        assert_eq!(verify_signature_bytes(secret, &headers, body), Ok(()));
    }

    #[test]
    fn verify_signature_bytes_rejects_tampered_body() {
        let secret = "demo-secret";
        let headers = signed_headers(secret, chrono::Utc::now().timestamp(), b"original");

        assert_eq!(
            verify_signature_bytes(secret, &headers, b"tampered"),
            Err(StatusCode::UNAUTHORIZED)
        );
    }

    #[test]
    fn verify_signature_bytes_rejects_stale_timestamp() {
        let secret = "demo-secret";
        let body = b"body";
        let headers = signed_headers(secret, chrono::Utc::now().timestamp() - 120, body);

        assert_eq!(
            verify_signature_bytes(secret, &headers, body),
            Err(StatusCode::UNAUTHORIZED)
        );
    }

    #[test]
    fn verify_signature_bytes_rejects_missing_headers() {
        assert_eq!(
            verify_signature_bytes("demo-secret", &HeaderMap::new(), b""),
            Err(StatusCode::UNAUTHORIZED)
        );
    }
}
