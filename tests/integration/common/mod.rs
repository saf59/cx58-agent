// tests/integration/common/mod.rs
//
// Shared utilities for all integration tests.
//
// AppState is initialised once per test process via OnceLock — avoids
// redundant DB connections and tracing subscriber conflicts across tests.
//
// SSE wire format (Axum Event):
//
//   Named event:          TextChunk (no event: line):
//     event: comparison     data: some text\n
//     data: {...}\n         \n
//     \n
//
//   StreamEvent::Completed is NOT sent as SSE — server closes the connection.
//   Detected as: stream ends without "error" or "cancelled" events.
//
// Event names from server code:
//   "started", "progress", "object", "report_list", "description",
//   "comparison", "context_request", "error", "cancelled"
//   "" (empty) → TextChunk

use cx58_agent::agents::document_agent::get_documents;
use cx58_agent::db::{NodeType, get_tree};
use cx58_agent::init::app_init;
use cx58_agent::{AgentRequest, AppState, TaskParameters};
use futures_util::StreamExt;
use hmac::{Hmac, Mac};
use reqwest::Client;
use sha2::Sha256;
use std::sync::{Arc, OnceLock};
use uuid::Uuid;

// ── Constants ─────────────────────────────────────────────────────────────────

pub const TEST_OBJECT_NAME: &str = "Room 11";
pub const BASE_URL: &str = "http://127.0.0.1:3050";
pub const CHAT_ENDPOINT: &str = "/agent/chat";

// ── Shared state (initialised once per process) ───────────────────────────────

static STATE: OnceLock<Arc<AppState>> = OnceLock::new();
static OBJECT_ID: OnceLock<String> = OnceLock::new();
static REPORT_IDS: OnceLock<(String, String)> = OnceLock::new();

/// Return the shared AppState, initialising it on the first call.
/// Subsequent calls return the cached instance immediately.
pub async fn shared_state() -> Arc<AppState> {
    if let Some(state) = STATE.get() {
        return Arc::clone(state);
    }

    dotenv::dotenv().ok();
    let _ = tracing_subscriber::fmt()
        .with_env_filter("cx58_agent=info")
        .try_init();

    let (_config, state) = app_init()
        .await
        .expect("app_init failed — check .env and DB connectivity");

    // OnceLock::set fails silently if another thread already set it —
    // that's fine, we just use whichever instance won the race.
    let _ = STATE.set(Arc::clone(&state));
    state
}

/// Read TEST_USER_ID from environment (set by dotenv in shared_state()).
pub fn test_user_id() -> String {
    std::env::var("TEST_USER_ID").expect("TEST_USER_ID must be set in .env")
}

/// Generate a fresh chat_id (UUID v7) for each test.
pub fn new_chat_id() -> String {
    Uuid::now_v7().to_string()
}

// ── Fixture resolution ────────────────────────────────────────────────────────

async fn try_resolve_object_id(
    state: &Arc<AppState>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let user_id = test_user_id();
    let tree = get_tree(&state.db, &user_id, false).await?;
    let node = tree
        .iter()
        .find(|n| {
            n.own && n.node_type == NodeType::Branch && n.name.as_deref() == Some(TEST_OBJECT_NAME)
        })
        .or_else(|| {
            tree.iter().find(|n| {
                n.node_type == NodeType::Branch && n.name.as_deref() == Some(TEST_OBJECT_NAME)
            })
        })
        .ok_or_else(|| {
            let available: Vec<_> = tree
                .iter()
                .filter(|n| n.node_type == NodeType::Branch)
                .map(|n| (n.name.as_deref().unwrap_or("<no name>"), n.own))
                .collect();
            format!(
                "Object '{}' not found for user '{}'.
Available Branch nodes: {:?}",
                TEST_OBJECT_NAME, user_id, available
            )
        })?;
    Ok(node.id.to_string())
}

/// Resolve the UUID of TEST_OBJECT_NAME from the user's object tree.
/// Result is cached after the first DB call. Retries up to 3 times on failure.
pub async fn resolve_object_id(state: &Arc<AppState>) -> String {
    if let Some(id) = OBJECT_ID.get() {
        return id.clone();
    }
    for attempt in 1..=3 {
        match try_resolve_object_id(state).await {
            Ok(id) => {
                let _ = OBJECT_ID.set(id.clone());
                return id;
            }
            Err(e) if attempt < 3 => {
                tracing::warn!(
                    "resolve_object_id attempt {}: {}, retrying in 2s...",
                    attempt,
                    e
                );
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
            Err(e) => panic!("get_tree failed: {}", e),
        }
    }
    unreachable!()
}

async fn try_resolve_report_ids(
    state: &Arc<AppState>,
    object_id: &str,
) -> Result<(String, String), Box<dyn std::error::Error + Send + Sync>> {
    let node_id =
        Uuid::parse_str(object_id).map_err(|_| format!("Invalid object_id UUID: {}", object_id))?;
    let params = TaskParameters {
        last: false,
        all: true,
        period: None,
        amount: None,
        exact_datetime: None,
    };
    let mut images: Vec<_> = get_documents(&&params, &state.db, node_id)
        .await?
        .into_iter()
        .filter(|n| n.node_type == NodeType::ImageLeaf)
        .collect();
    if images.len() < 2 {
        return Err(format!(
            "Object '{}' has {} report(s), need at least 2",
            TEST_OBJECT_NAME,
            images.len()
        )
        .into());
    }
    images.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok((images[0].id.to_string(), images[1].id.to_string()))
}

/// Resolve the two most recent report IDs for the given object_id.
/// Returns (current/newest, previous/older). Panics if fewer than 2 reports exist.
/// Result is cached after the first DB call. Retries up to 3 times on failure.
pub async fn resolve_report_ids(state: &Arc<AppState>, object_id: &str) -> (String, String) {
    if let Some(ids) = REPORT_IDS.get() {
        return ids.clone();
    }
    for attempt in 1..=3 {
        match try_resolve_report_ids(state, object_id).await {
            Ok(ids) => {
                let _ = REPORT_IDS.set(ids.clone());
                return ids;
            }
            Err(e) if attempt < 3 => {
                tracing::warn!(
                    "resolve_report_ids attempt {}: {}, retrying in 2s...",
                    attempt,
                    e
                );
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
            Err(e) => panic!("get_documents failed: {}", e),
        }
    }
    unreachable!()
}

// ── HMAC signing ──────────────────────────────────────────────────────────────

type HmacSha256 = Hmac<Sha256>;

/// Sign a request body. Mirrors verify_signature() in hmac.rs:
///   HMAC-SHA256(secret, timestamp_bytes || body_bytes)
pub fn sign_request(body: &[u8], secret: &str) -> (String, String) {
    let timestamp = chrono::Utc::now().timestamp().to_string();
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(timestamp.as_bytes());
    mac.update(body);
    (timestamp, hex::encode(mac.finalize().into_bytes()))
}

// ── SSE parser ────────────────────────────────────────────────────────────────

/// A decoded SSE event. `name` is the value from the "event:" line,
/// or empty string for TextChunk (no event: line). `data` is raw string.
#[derive(Debug, Clone)]
pub struct SseEvent {
    pub name: String,
    pub data: String,
}

pub async fn send_and_collect(state: &Arc<AppState>, request: &AgentRequest) -> Vec<SseEvent> {
    let body_bytes = serde_json::to_vec(request).expect("serialization failed");
    let (timestamp, signature) = sign_request(&body_bytes, &state.ai_config.agent_secret);
    let url = format!("{}{}", BASE_URL, CHAT_ENDPOINT);

    let response = Client::new()
        .post(&url)
        .header("Content-Type", "application/json")
        .header("X-Timestamp", &timestamp)
        .header("X-Signature", &signature)
        .body(body_bytes)
        .send()
        .await
        .unwrap_or_else(|e| panic!("HTTP POST {} failed: {}", url, e));

    assert_eq!(
        response.status(),
        200,
        "Expected 200 OK, got {}",
        response.status()
    );

    collect_sse_events(response).await
}

async fn collect_sse_events(response: reqwest::Response) -> Vec<SseEvent> {
    let mut events = Vec::new();
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut current_name = String::new();
    let mut current_data: Option<String> = None;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.expect("SSE stream read error");
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim_end_matches('\r').to_string();
            buffer.drain(..=pos);

            if line.is_empty() {
                if let Some(data) = current_data.take() {
                    events.push(SseEvent {
                        name: current_name.clone(),
                        data,
                    });
                }
                current_name.clear();
            } else if let Some(name) = line.strip_prefix("event: ") {
                current_name = name.trim().to_string();
            } else if let Some(data) = line.strip_prefix("data: ") {
                current_data = Some(data.to_string());
            }
        }
    }

    if let Some(data) = current_data {
        events.push(SseEvent {
            name: current_name,
            data,
        });
    }

    events
}

// ── Assertion helpers ─────────────────────────────────────────────────────────

/// Extract the "type" field from a JSON data payload, falling back to the SSE
/// event name so the helper works regardless of how the server encodes events.
fn event_type(e: &SseEvent) -> String {
    serde_json::from_str::<serde_json::Value>(&e.data)
        .ok()
        .and_then(|v| v["type"].as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| e.name.clone())
}

/// Find the first event whose JSON "type" field (or SSE name) matches `name`.
/// Returns the inner "data" field as a JSON string (or the full payload when
/// there is no nested "data" key).
pub fn assert_event(events: &[SseEvent], name: &str) -> String {
    let event = events
        .iter()
        .find(|e| event_type(e) == name)
        .unwrap_or_else(|| {
            panic!(
                "Expected SSE event '{}' not found.\nAll events: {:?}",
                name,
                events.iter().map(|e| event_type(e)).collect::<Vec<_>>()
            )
        });
    if let Ok(outer) = serde_json::from_str::<serde_json::Value>(&event.data) {
        if let Some(inner) = outer.get("data") {
            return inner.to_string();
        }
    }
    event.data.clone()
}

/// Completed = stream contained a "completed" event and no "error"/"cancelled".
pub fn assert_completed(events: &[SseEvent]) {
    let bad: Vec<_> = events
        .iter()
        .filter(|e| matches!(event_type(e).as_str(), "error" | "cancelled"))
        .collect();
    assert!(
        bad.is_empty(),
        "Stream ended with error/cancelled: {:?}",
        bad.iter()
            .map(|e| format!("{}: {}", event_type(e), e.data))
            .collect::<Vec<_>>()
    );
    let has_completed = events.iter().any(|e| event_type(e) == "completed");
    assert!(
        has_completed,
        "Stream did not contain a 'completed' event.\nAll events: {:?}",
        events.iter().map(|e| event_type(e)).collect::<Vec<_>>()
    );
}

pub fn assert_no_error(events: &[SseEvent]) {
    let errs: Vec<_> = events.iter().filter(|e| event_type(e) == "error").collect();
    assert!(
        errs.is_empty(),
        "Unexpected error event(s): {:?}",
        errs.iter().map(|e| &e.data).collect::<Vec<_>>()
    );
}

// ── Request builders ──────────────────────────────────────────────────────────

pub fn build_request(message: impl Into<String>, language: impl Into<String>) -> AgentRequest {
    AgentRequest {
        message: message.into(),
        user_id: test_user_id(),
        chat_id: new_chat_id(),
        language: language.into(),
        object_id: None,
        prev_leaf: None,
        next_leaf: None,
        metadata: None,
    }
}

pub fn build_request_with_object(
    message: impl Into<String>,
    language: impl Into<String>,
    object_id: impl Into<String>,
) -> AgentRequest {
    AgentRequest {
        object_id: Some(object_id.into()),
        ..build_request(message, language)
    }
}

/// prev_leaf = current/newer; next_leaf = previous/older.
pub fn build_request_with_reports(
    message: impl Into<String>,
    language: impl Into<String>,
    object_id: impl Into<String>,
    current_report_id: impl Into<String>,
    previous_report_id: impl Into<String>,
) -> AgentRequest {
    AgentRequest {
        object_id: Some(object_id.into()),
        prev_leaf: Some(current_report_id.into()),
        next_leaf: Some(previous_report_id.into()),
        ..build_request(message, language)
    }
}
