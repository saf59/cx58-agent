use crate::error::*;
use crate::models::*;
use axum::extract::Query;
// ============================================================================
// Middleware
// ============================================================================
use crate::AgentRequest;
use crate::AppState;
use crate::agents::StreamEvent;
use crate::db::{NodeType, NodeWithLeaf, TreeNode, get_node_with_leafs, get_tree};
use crate::model_settings::{
    ModelCapability, ModelChangeResult, UpdateModelsRequest, UpdateModelsResponse,
    UserModelSettings, UserModelsResponse, inspect_ollama_model, list_ollama_models,
    load_user_model_settings, required_capability_label, save_user_model_settings, supports_role,
};
use crate::report_datetime::parse_berlin_datetime_as_utc_naive;
pub use crate::storage::StorageService;
use crate::storage::{apply_storage_url_update, resolve_storage_url_updates};
use axum::http::StatusCode;
use axum::response::Sse;
use axum::response::sse::{Event, KeepAlive};
use axum::{
    Json,
    extract::{Path, State},
};
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct TreeQuery {
    #[serde(default)]
    pub with_leafs: bool,
}

#[derive(Deserialize)]
pub struct ModelsQuery {
    pub capability: Option<String>,
}

pub async fn get_tree_handler(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<String>,
    Query(query): Query<TreeQuery>,
) -> Result<Json<Vec<TreeNode>>> {
    let mut tree = get_tree(&state.db, &user_id, query.with_leafs).await?;

    attach_tree_storage_urls(state, &mut tree).await;

    Ok(Json(tree))
}
pub async fn reports_handler(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
) -> Result<Json<Vec<NodeWithLeaf>>> {
    let node_id =
        Uuid::parse_str(&node_id).map_err(|_e| AppError::bad_request("Invalid node_id format"))?;

    let mut data = get_node_with_leafs(&state.db, node_id, Some(1000), None, None)
        .await
        .map_err(|_e| AppError::internal("Failed to fetch reports"))?;

    attach_report_storage_urls(state, &mut data).await;
    Ok(Json(data))
}

async fn attach_tree_storage_urls(state: Arc<AppState>, nodes: &mut [TreeNode]) {
    let requests = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            if !matches!(node.node_type, NodeType::ImageLeaf) {
                return None;
            }
            let storage_path = node
                .data
                .as_object()
                .and_then(|obj| obj.get("storage_path"))
                .and_then(|v| v.as_str())?
                .to_string();
            Some((index, node.id, storage_path))
        })
        .collect();

    for (index, update) in resolve_storage_url_updates(state, requests).await {
        if let Some(obj) = nodes[index].data.as_object_mut() {
            apply_storage_url_update(obj, update);
        }
    }
}

async fn attach_report_storage_urls(state: Arc<AppState>, nodes: &mut [NodeWithLeaf]) {
    let requests = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            if !matches!(node.node_type, NodeType::ImageLeaf) {
                return None;
            }
            let storage_path = node
                .data
                .as_object()
                .and_then(|obj| obj.get("storage_path"))
                .and_then(|v| v.as_str())?
                .to_string();
            Some((index, node.id, storage_path))
        })
        .collect();

    for (index, update) in resolve_storage_url_updates(state, requests).await {
        if let Some(obj) = nodes[index].data.as_object_mut() {
            apply_storage_url_update(obj, update);
        }
    }
}

#[derive(Deserialize)]
pub struct UpdateReportDatetimeRequest {
    pub berlin_datetime: String,
}

pub async fn update_report_datetime_handler(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
    Json(request): Json<UpdateReportDatetimeRequest>,
) -> Result<StatusCode> {
    let node_id =
        Uuid::parse_str(&node_id).map_err(|_e| AppError::bad_request("Invalid node_id format"))?;
    let updated_at_utc = parse_berlin_datetime_as_utc_naive(&request.berlin_datetime)
        .map_err(|e| AppError::bad_request(e.to_string()))?;

    let result = sqlx::query(
        r#"
        UPDATE tree_nodes
        SET updated_at = $2
        WHERE id = $1 AND node_type = 'ImageLeaf'::node_type_enum
        "#,
    )
    .bind(node_id)
    .bind(updated_at_utc)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::not_found("Report"));
    }

    Ok(StatusCode::NO_CONTENT)
}
pub async fn get_user_models_handler(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<String>,
    Query(query): Query<ModelsQuery>,
) -> Result<Json<UserModelsResponse>> {
    let capability = match query.capability.as_deref() {
        Some(raw) => Some(
            ModelCapability::from_query(raw)
                .ok_or_else(|| AppError::bad_request("Unknown model capability"))?,
        ),
        None => None,
    };
    let current = load_user_model_settings(&state.db, &user_id, &state.ai_config).await;
    let defaults = UserModelSettings::from_defaults(&user_id, &state.ai_config);
    let models = list_ollama_models(&state.ai_config.url, capability).await?;

    Ok(Json(UserModelsResponse {
        user_id,
        current,
        defaults,
        models,
        capability,
    }))
}

pub async fn update_user_models_handler(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<String>,
    Json(request): Json<UpdateModelsRequest>,
) -> Result<Json<UpdateModelsResponse>> {
    let mut current = load_user_model_settings(&state.db, &user_id, &state.ai_config).await;
    let mut changes = Vec::new();

    if let Some(model) = request.vision_model.as_deref() {
        let info = inspect_ollama_model(&state.ai_config.url, model).await?;
        if apply_model_change(&mut current, "vision_model", &info, &mut changes) && request.same {
            apply_model_change(&mut current, "text_model", &info, &mut changes);
            apply_model_change(&mut current, "chat_model", &info, &mut changes);
        }
    }

    if let Some(model) = request.text_model.as_deref() {
        let info = inspect_ollama_model(&state.ai_config.url, model).await?;
        apply_model_change(&mut current, "text_model", &info, &mut changes);
    }

    if let Some(model) = request.chat_model.as_deref() {
        let info = inspect_ollama_model(&state.ai_config.url, model).await?;
        apply_model_change(&mut current, "chat_model", &info, &mut changes);
    }

    save_user_model_settings(&state.db, &current).await?;

    Ok(Json(UpdateModelsResponse {
        user_id,
        current,
        changes,
    }))
}

fn apply_model_change(
    settings: &mut UserModelSettings,
    role: &str,
    model: &crate::model_settings::OllamaModelInfo,
    changes: &mut Vec<ModelChangeResult>,
) -> bool {
    if !supports_role(model, role) {
        changes.push(ModelChangeResult {
            role: role.to_string(),
            model: model.name.clone(),
            applied: false,
            reason: format!(
                "Model does not support required capability: {}",
                required_capability_label(role)
            ),
        });
        return false;
    }

    match role {
        "vision_model" => settings.vision_model = model.name.clone(),
        "text_model" => settings.text_model = model.name.clone(),
        "chat_model" => settings.chat_model = model.name.clone(),
        _ => return false,
    }

    changes.push(ModelChangeResult {
        role: role.to_string(),
        model: model.name.clone(),
        applied: true,
        reason: "Applied".to_string(),
    });
    true
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
/// POST /agent/chat/stream
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

                    // Keep reading after Error so the top-level Completed event
                    // remains observable by clients.
                    match event {
                        StreamEvent::Completed { .. }
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
/// DELETE /agent/chat/cancel/:request_id
///
/// Returns: JSON with cancellation status
pub async fn chat_stream_cancel(
    State(state): State<Arc<AppState>>,
    Path(request_id): Path<String>,
) -> std::result::Result<Json<CancelResponse>, (StatusCode, Json<CancelErrorResponse>)> {
    // Attempt to cancel the request
    tracing::info!("Attempt to stop request: {}", request_id);
    let cancelled = state.master_agent.cancel_request(&request_id).await;

    if cancelled {
        tracing::debug!("Request cancelled successfully");
        Ok(Json(CancelResponse {
            success: true,
            request_id: request_id.clone(),
            message: format!("Request {} cancelled successfully", request_id),
        }))
    } else {
        tracing::info!("Cancel request not found or already completed!");
        Err((
            StatusCode::FAILED_DEPENDENCY,
            Json(CancelErrorResponse {
                error: "NOT_FOUND".to_string(),
                message: format!("Request {} not found or already completed", request_id),
            }),
        ))
    }
}
pub async fn health_check(State(state): State<Arc<AppState>>) -> Json<HealthStatus> {
    let mut health = HealthStatus::healthy();

    health.services.database = sqlx::query("SELECT 1").fetch_one(&state.db).await.is_ok();

    /*    health.services.redis = redis::cmd("PING")
            .query_async::<_, String>(&mut state.redis.clone())
            .await
            .is_ok();
    */
    health.services.s3 = match state.storage.list_user_images("health-check").await {
        Ok(_) => true,
        Err(e) => {
            tracing::warn!("S3 health check failed: {}", e);
            false
        }
    };

    health.services.ollama = match reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
    {
        Ok(client) => client
            .get(format!("{}/api/tags", state.ai_config.url))
            .send()
            .await
            .is_ok(),
        Err(e) => {
            tracing::warn!("Failed to build Ollama health client: {}", e);
            false
        }
    };

    if !health.is_healthy() {
        health.status = "degraded".to_string();
    }

    Json(health)
}
