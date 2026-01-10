use crate::error::*;
use crate::models::*;
use axum::extract::Query;
// ============================================================================
// Middleware
// ============================================================================
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::sse::{Event, KeepAlive};
use axum::response::{Response, Sse};
use axum::{
    Json,
    extract::{Path, Request, State},
};
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use uuid::Uuid;

use crate::AgentRequest;
use crate::AppState;
use crate::agents::StreamEvent;
use crate::db::{NodeType, TreeNode, get_tree};
pub use crate::storage::{ImageProcessor, ImageUrlResolver, StorageService};

pub async fn auth_middleware(
    mut request: Request,
    next: Next,
) -> std::result::Result<Response, StatusCode> {
    let user_id = request
        .headers()
        .get("X-User-ID")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or(StatusCode::UNAUTHORIZED);

    let session_id = request
        .headers()
        .get("X-Session-ID")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .ok_or(StatusCode::UNAUTHORIZED);

    let language = request
        .headers()
        .get("X-Language")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "en".to_string());

    let chat_id = request
        .headers()
        .get("X-Chat-ID")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| Uuid::parse_str(s).ok())
        .unwrap_or_else(Uuid::now_v7);

    request.extensions_mut().insert(user_id);
    request.extensions_mut().insert(session_id);
    request.extensions_mut().insert(language);
    request.extensions_mut().insert(chat_id);

    Ok(next.run(request).await)
}

#[derive(Deserialize)]
pub struct TreeQuery {
    #[serde(default)]
    pub with_leafs: bool,
}

pub async fn get_tree_handler(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<String>,
    Query(query): Query<TreeQuery>,
) -> Result<Json<Vec<TreeNode>>> {
    let mut tree = get_tree(&state.db, &user_id, query.with_leafs).await?;

    // Process ImageLeaf nodes
    for node in &mut tree {
        if matches!(node.node_type, NodeType::ImageLeaf)
            && let Some(obj) = node.data.as_object_mut()
        {
            // Copy storage_path to String to avoid borrowing conflicts
            let storage_path = obj
                .get("storage_path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            if let Some(storage_path) = storage_path {
                // Generate signed URL for original image
                match state
                    .storage
                    .generate_presigned_url(&storage_path, 86400)
                    .await
                {
                    Ok(url) => {
                        obj.insert("url".to_string(), serde_json::json!(url));
                    }
                    Err(e) => {
                        eprintln!("Failed to generate URL for {}: {}", storage_path, e);
                    }
                }

                // Get or create thumbnail (already with public URL)
                match state
                    .storage
                    .get_or_create_thumbnail(&node.id, &storage_path, 300, 300)
                    .await
                {
                    Ok(thumbnail) => {
                        // thumbnail.url is already public - just insert it
                        obj.insert(
                            "thumbnail_url".to_string(),
                            serde_json::json!(thumbnail.public_url),
                        );
                    }
                    Err(e) => {
                        eprintln!("Failed to create thumbnail for {}: {}", storage_path, e);
                    }
                }
            }
        }
    }

    Ok(Json(tree))
}

/// ============================================================================
// RESPONSE TYPES
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct CancelResponse {
    pub success: bool,
    pub request_id: String,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CancelErrorResponse {
    pub error: String,
    pub message: String,
}

// ============================================================================
// SSE STREAM HANDLER
// ============================================================================

/// Handler for streaming chat responses via SSE
///
/// POST /api/chat/stream
/// Body: AgentRequest JSON
///
/// Returns: Server-Sent Events stream with StreamEvent data
pub async fn chat_stream_handler(
    State(state): State<Arc<AppState>>,
    Json(request): Json<AgentRequest>,
) -> Sse<impl Stream<Item = std::result::Result<Event, Infallible>>> {
    let agent = state.master_agent.clone();
    let mut rx = agent.handle_request_stream(state.clone(), request).await;

    // Get event receiver from agent
    //let mut rx = state.agent.handle_request_stream(request).await;

    // Create async stream
    let stream = async_stream::stream! {
        while let Some(event) = rx.recv().await {
            // Serialize event to JSON
            match serde_json::to_string(&event) {
                Ok(json_data) => {
                    // Create SSE event with JSON data
                    let sse_event = Event::default()
                        .event("message")
                        .data(json_data);

                    yield Ok(sse_event);

                    // Check if this is a terminal event
                    match event {
                        StreamEvent::Completed { .. }
                        | StreamEvent::Error { .. }
                        | StreamEvent::Cancelled { .. } => {
                            break;
                        }
                        _ => {}
                    }
                }
                Err(e) => {
                    // If serialization fails, send error event
                    let error_event = StreamEvent::Error {
                        request_id: "unknown".to_string(),
                        error: format!("Serialization error: {}", e),
                        recoverable: false,
                    };

                    if let Ok(json_data) = serde_json::to_string(&error_event) {
                        let sse_event = Event::default()
                            .event("error")
                            .data(json_data);
                        yield Ok(sse_event);
                    }
                    break;
                }
            }
        }

        // Send final event to indicate stream end
        let done_event = Event::default()
            .event("done")
            .data("Stream closed");
        yield Ok(done_event);
    };

    // Return SSE with keep-alive
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("keep-alive"),
    )
}

// ============================================================================
// CANCEL HANDLER
// ============================================================================

/// Handler for cancelling an active request
///
/// DELETE /api/chat/cancel/:request_id
///
/// Returns: JSON with cancellation status
pub async fn chat_stream_cancel(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
) -> std::result::Result<Json<CancelResponse>, (StatusCode, Json<CancelErrorResponse>)> {
    // Attempt to cancel the request
    let cancelled = state.master_agent.cancel_request(&request_id).await;

    if cancelled {
        Ok(Json(CancelResponse {
            success: true,
            request_id: request_id.clone(),
            message: format!("Request {} cancelled successfully", request_id),
        }))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(CancelErrorResponse {
                error: "NOT_FOUND".to_string(),
                message: format!("Request {} not found or already completed", request_id),
            }),
        ))
    }
}
pub async fn health_check(State(state): State<Arc<AppState>>) -> axum::Json<HealthStatus> {
    let mut health = HealthStatus::healthy();

    health.services.database = sqlx::query("SELECT 1").fetch_one(&state.db).await.is_ok();

    /*    health.services.redis = redis::cmd("PING")
            .query_async::<_, String>(&mut state.redis.clone())
            .await
            .is_ok();
    */
    health.services.s3 = state.storage.exists("health-check").await.unwrap_or(true);

    health.services.ollama = reqwest::get(format!("{}/api/tags", state.ai_config.url))
        .await
        .is_ok();

    if !health.is_healthy() {
        health.status = "degraded".to_string();
    }

    axum::Json(health)
}
