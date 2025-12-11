use crate::models::{AppError, ImageMetadata, Result, StorageResult, UploadResponse};

pub async fn upload_image_handler(
    State(state): State<Arc<AppState>>,
    mut multipart: axum::extract::Multipart,
) -> Result<Json<UploadResponse>> {
    let user_id = extract_user_context()?;

    // File validation using shared FileSize type
    let max_size = ai_agent_shared::FileSize::megabytes(10);

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad_request(format!("Multipart error: {}", e)))?
    {
        if field.name() == Some("image") {
            let filename = field
                .file_name()
                .ok_or_else(|| AppError::bad_request("Missing filename"))?
                .to_string();

            let data = field
                .bytes()
                .await
                .map_err(|e| AppError::bad_request(format!("Read error: {}", e)))?;

            // Size validation
            if data.len() as u64 > max_size.as_bytes() {
                return Err(AppError::bad_request(format!(
                    "File too large. Max size: {}",
                    max_size
                )));
            }

            // MIME type validation using shared type
            let mime = ai_agent_shared::MimeType::from(
                mime_guess::from_path(&filename)
                    .first_or_octet_stream()
                    .to_string(),
            );

            if !mime.is_image() {
                return Err(AppError::validation("Only image files are allowed"));
            }

            let node_id = uuid::Uuid::new_v4();
            let result = state
                .storage
                .upload_image(&user_id, &node_id, data, &filename)
                .await?;

            return Ok(Json(UploadResponse {
                node_id,
                url: result.public_url,
                storage_path: result.storage_path,
                size: result.size,
            }));
        }
    }

    Err(AppError::bad_request("No image field found"))
}

// Helper to extract user from context
fn extract_user_context() -> Result<uuid::Uuid> {
    // Implementation depends on your auth middleware
    Ok(uuid::Uuid::new_v4())
}

// ============================================================================
// Example: Database queries with shared types
// ============================================================================

use sqlx::PgPool;

pub async fn create_tree_node(
    db: &PgPool,
    user_id: &uuid::Uuid,
    parent_id: Option<uuid::Uuid>,
    node_type: NodeType,
    data: NodeData,
) -> Result<TreeNode> {
    let id = uuid::Uuid::new_v4();

    sqlx::query!(
        r#"
        INSERT INTO tree_nodes (id, user_id, parent_id, node_type, data)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING created_at
        "#,
        id,
        user_id,
        parent_id,
        node_type as NodeType, // sqlx handles the enum conversion
        serde_json::to_value(&data)
            .map_err(|e| AppError::internal(format!("Serialization error: {}", e)))?
    )
    .fetch_one(db)
    .await?;

    Ok(TreeNode {
        id,
        parent_id,
        node_type,
        data,
        children: vec![],
        created_at: ai_agent_shared::Timestamp::now().to_string(),
    })
}

pub async fn get_user_stats(
    db: &PgPool,
    user_id: &uuid::Uuid,
) -> Result<ai_agent_shared::UserStats> {
    let stats = sqlx::query!(
        r#"
        SELECT
            COUNT(*) FILTER (WHERE node_type = 'ImageLeaf') as "total_images!",
            COUNT(*) as "total_nodes!",
            COALESCE(SUM((data->>'size')::bigint), 0) as "storage_bytes!"
        FROM tree_nodes
        WHERE user_id = $1
        "#,
        user_id
    )
    .fetch_one(db)
    .await?;

    let message_count = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM chat_messages WHERE user_id = $1",
        user_id
    )
    .fetch_one(db)
    .await?
    .unwrap_or(0);

    Ok(ai_agent_shared::UserStats {
        user_id: *user_id,
        total_nodes: stats.total_nodes as u64,
        total_images: stats.total_images as u64,
        total_messages: message_count as u64,
        storage_used_bytes: stats.storage_bytes as u64,
        last_activity: None,
    })
}

// ============================================================================
// Example: Validation using shared types
// ============================================================================

use ai_agent_shared::{ValidationError, ValidationErrors};

pub fn validate_agent_request(request: &AgentRequest) -> Result<()> {
    let mut errors = ValidationErrors::new();

    if request.message.trim().is_empty() {
        errors.add(ValidationError::new("message", "Message cannot be empty"));
    }

    if request.message.len() > 10000 {
        errors.add(
            ValidationError::new("message", "Message too long (max 10000 chars)")
                .with_code("TOO_LONG"),
        );
    }

    if !ai_agent_shared::validate_uuid(&request.user_id.to_string()) {
        errors.add(ValidationError::new("user_id", "Invalid user ID"));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.into_app_error())
    }
}

// ============================================================================
// Example: Health check using shared types
// ============================================================================

use ai_agent_shared::{HealthStatus, ServiceHealth};

pub async fn health_check_handler(State(state): State<Arc<AppState>>) -> Json<HealthStatus> {
    let mut health = HealthStatus::healthy();

    // Check database
    health.services.database = sqlx::query("SELECT 1").fetch_one(&state.db).await.is_ok();

    // Check Redis
    health.services.redis = redis::cmd("PING")
        .query_async::<_, String>(&mut state.redis.clone())
        .await
        .is_ok();

    // Check S3
    health.services.s3 = state
        .storage
        .operator
        .is_exist("health-check")
        .await
        .is_ok();

    // Check Qdrant
    health.services.qdrant = state.qdrant.health_check().await.is_ok();

    // Check Ollama
    health.services.ollama = reqwest::get(format!("{}/api/tags", state.ollama_url))
        .await
        .is_ok();

    // Update status
    if !health.is_healthy() {
        health.status = "degraded".to_string();
    }

    Json(health)
}

// ============================================================================
// Example: Middleware using shared error types
// ============================================================================

use axum::{
    body::Body,
    http::{Request, Response},
    middleware::Next,
};

pub async fn error_handler_middleware(req: Request<Body>, next: Next) -> Response<Body> {
    let response = next.run(req).await;

    // Log errors based on status
    if response.status().is_server_error() {
        log::error!("Server error: {}", response.status());
    } else if response.status().is_client_error() {
        log::warn!("Client error: {}", response.status());
    }

    response
}

// Rate limiting example
pub async fn rate_limit_middleware(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
    next: Next,
) -> std::result::Result<Response<Body>, AppError> {
    let user_id = req
        .extensions()
        .get::<uuid::Uuid>()
        .ok_or_else(|| AppError::unauthorized("User not authenticated"))?;

    // Check rate limit in Redis
    let key = format!("rate_limit:{}", user_id);
    let count: i32 = redis::cmd("INCR")
        .arg(&key)
        .query_async(&mut state.redis.clone())
        .await?;

    if count == 1 {
        redis::cmd("EXPIRE")
            .arg(&key)
            .arg(60) // 1 minute window
            .query_async::<_, ()>(&mut state.redis.clone())
            .await?;
    }

    if count > 100 {
        return Err(AppError::rate_limit());
    }

    Ok(next.run(req).await)
}
