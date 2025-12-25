use std::convert::Infallible;
// ============================================================================
// Middleware
// ============================================================================
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{Response, Sse};
use crate::models::*;
use crate::error::*;
use axum::{
    extract::{Path, Request, State},
    Json,
};
use std::sync::Arc;
use axum::response::sse::{Event, KeepAlive};
use futures::Stream;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use crate::storage::{StorageService, ImageProcessor, ImageUrlResolver};
use crate::AppState;
use crate::AgentRequest;
use crate::agents::StreamEvent;

pub async fn auth_middleware(mut request: Request, next: Next) -> std::result::Result<Response, StatusCode> {
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

pub async fn get_tree_handler(
    State(state): State<Arc<AppState>>,
    Path((user_id, root_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<TreeNode>> {
    let tree = load_full_tree(&state.db, &user_id, &root_id).await?;
    Ok(Json(tree))
}

async fn load_full_tree(
    db: &sqlx::PgPool,
    user_id: &Uuid,
    root_id: &Uuid,
) -> Result<TreeNode> {
    let node = sqlx::query!(
        r#"
        WITH RECURSIVE tree AS (
            SELECT * FROM tree_nodes WHERE id = $1 AND user_id = $2
            UNION ALL
            SELECT tn.* FROM tree_nodes tn
            INNER JOIN tree t ON tn.parent_id = t.id
            WHERE tn.user_id = $2
        )
        SELECT id, parent_id, node_type as "node_type!: NodeType",
               data, created_at
        FROM tree
        WHERE id = $1
        "#,
        root_id,
        user_id
    )
        .fetch_one(db)
        .await?;

    Ok(TreeNode {
        id: node.id.unwrap(),
        parent_id: node.parent_id,
        node_type: node.node_type,
        data: serde_json::from_value(node.data.unwrap())?,
        children: vec![],
        created_at: node.created_at.unwrap().to_rfc3339(),
    })
}
// ============================================================================
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
