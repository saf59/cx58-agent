use crate::AppState;
use axum::extract::State;
use axum::{
    body::Body,
    extract::{ConnectInfo, Request},
    http::StatusCode,
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
    axum::extract::State(limiter): axum::extract::State<RateLimiter>,
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

    let timestamp = parts
        .headers
        .get("X-Timestamp")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<i64>().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let provided_signature = parts
        .headers
        .get("X-Signature")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Check timestamp freshness (within 1 minute)
    let now = chrono::Utc::now().timestamp();
    if (now - timestamp).abs() > 60 {
        tracing::warn!("Timestamp out of range");
        return Err(StatusCode::UNAUTHORIZED);
    }

    let bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let provided_signature_bytes = hex::decode(provided_signature)
        .map_err(|e| {
            tracing::warn!("Invalid signature format: {}", e);
            StatusCode::UNAUTHORIZED
        })?;

    // Create HMAC
    let mut mac = HmacSha256::new_from_slice(state.ai_config.agent_secret.as_bytes())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    mac.update(timestamp.to_string().as_bytes());
    mac.update(&bytes);

    // Using of embed constant-time verification to prevent timing attacks
    mac.verify_slice(&provided_signature_bytes)
        .map_err(|_| {
            tracing::warn!("HMAC verification failed");
            StatusCode::UNAUTHORIZED
        })?;

    tracing::debug!("HMAC signature verified successfully");

    let req = Request::from_parts(parts, Body::from(bytes));
    Ok(next.run(req).await)
}
