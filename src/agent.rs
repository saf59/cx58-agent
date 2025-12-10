use ai_agent_shared::{
    AgentRequest, AppError, ErrorCode, NodeData, NodeType, Result, StreamEvent, TreeNode,
};
use axum::{
    Json,
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
};
use futures::stream::Stream;
use std::sync::Arc;

// Now all handlers use shared types
pub async fn chat_stream_handler(
    State(state): State<Arc<AppState>>,
    Json(request): Json<AgentRequest>, // Using shared AgentRequest
) -> axum::response::Result<Sse<impl Stream<Item = std::result::Result<Event, String>>>, AppError> {
    // Validation using shared types
    if request.message.trim().is_empty() {
        return Err(AppError::bad_request("Message cannot be empty"));
    }

    let agent = state.agent.read().await;
    let stream = agent.execute(request, state.clone()).await;

    let event_stream = stream.map(|result| {
        result.and_then(|event| {
            serde_json::to_string(&event)
                .map_err(|e| e.to_string())
                .map(|json| Event::default().data(json))
        })
    });

    Ok(Sse::new(event_stream).keep_alive(KeepAlive::default()))
}

// Error handling example
pub async fn get_tree_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Path((user_id, root_id)): axum::extract::Path<(uuid::Uuid, uuid::Uuid)>,
) -> Result<Json<TreeNode>> {
    // Using shared Result type
    let tree = load_full_tree(&state.db, &user_id, &root_id)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => AppError::not_found("Tree node"),
            _ => AppError::from(e),
        })?;

    Ok(Json(tree))
}
