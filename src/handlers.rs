// ============================================================================
// Middleware
// ============================================================================
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use crate::models::*;
use crate::error::*;
use axum::{
    extract::{Path, Request, State},
    Json,
};
use std::sync::Arc;
use uuid::Uuid;

pub use crate::storage::{StorageService, ImageProcessor, ImageUrlResolver};
use crate::storage::AppState;

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
