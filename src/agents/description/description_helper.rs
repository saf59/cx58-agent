use chrono::NaiveDateTime;
use sqlx::PgPool;
use uuid::Uuid;

/// Database fields required to render an image report consistently.
pub struct ResolvedImageNode {
    pub data: sqlx::types::JsonValue,
    pub updated_at: NaiveDateTime,
}

/// Gets image node data together with the report timestamp.
pub async fn resolve_image_node(
    pool: &PgPool,
    node_id: &Uuid,
) -> Result<ResolvedImageNode, Box<dyn std::error::Error + Send + Sync>> {
    let row: (sqlx::types::JsonValue, NaiveDateTime) = sqlx::query_as(
        r#"SELECT data, updated_at FROM tree_nodes WHERE id = $1 AND node_type = 'ImageLeaf'"#,
    )
    .bind(node_id)
    .fetch_one(pool)
    .await?;

    Ok(ResolvedImageNode {
        data: row.0,
        updated_at: row.1,
    })
}

/// Get node data (JSON) from database by node_id
pub async fn resolve_node_data(
    pool: &PgPool,
    node_id: &Uuid,
) -> Result<sqlx::types::JsonValue, Box<dyn std::error::Error + Send + Sync>> {
    Ok(resolve_image_node(pool, node_id).await?.data)
}

/// Get image URL from node data in database by node_id
pub async fn resolve_node_storage_path(
    pool: &PgPool,
    node_id: &Uuid,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let data = resolve_node_data(pool, node_id).await?;

    let storage_path = data
        .get("storage_path")
        .and_then(|v| v.as_str())
        .ok_or("'storage_path' field not found in node data")?;

    Ok(storage_path.to_string())
}
// Get full node name (path) from database by node_id
pub async fn resolve_node_full_name(
    pool: &PgPool,
    node_id: &Uuid,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let result: (Option<String>,) = sqlx::query_as(r#"SELECT get_full_node_name($1)"#)
        .bind(node_id)
        .fetch_one(pool)
        .await?;

    result
        .0
        .ok_or_else(|| format!("Node with id '{}' not found", node_id).into())
}
