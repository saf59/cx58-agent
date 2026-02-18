use sqlx::PgPool;
use uuid::Uuid;

/// Get node data (JSON) from database by node_id
pub async fn resolve_node_data(
    pool: &PgPool,
    node_id: &Uuid,
) -> Result<sqlx::types::JsonValue, Box<dyn std::error::Error + Send + Sync>> {
    let row: (sqlx::types::JsonValue,) = sqlx::query_as(
        r#"SELECT data FROM tree_nodes WHERE id = $1 AND node_type = 'ImageLeaf'"#,
    )
        .bind(node_id)
        .fetch_one(pool)
        .await?;

    Ok(row.0)
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
        .ok_or_else(|| "'storage_path' field not found in node data")?;

    Ok(storage_path.to_string())
}
// Get full node name (path) from database by node_id
pub async fn resolve_node_full_name(
    pool: &PgPool,
    node_id: &Uuid,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let result: (Option<String>,) = sqlx::query_as(
        r#"SELECT get_full_node_name($1)"#,
    )
        .bind(node_id)
        .fetch_one(pool)
        .await?;

    result.0.ok_or_else(|| format!("Node with id '{}' not found", node_id).into())
}

// Get image filename/src from node data in database by node_id
/*
pub async fn resolve_node_filename(
    pool: &PgPool,
    node_id: &Uuid,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let data = resolve_node_data(pool, node_id).await?;

    let filename = data
        .get("src")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "src field not found in node data")?;

    Ok(filename.to_string())
}
// Get the latest description for a node (without filtering by model)
pub async fn get_latest_description(
    pool: &PgPool,
    node_id: &Uuid,
) -> Result<Option<ImageDescription>, sqlx::Error> {
    let descriptions = get_descriptions_by_node(pool, node_id).await?;

    Ok(descriptions.first().cloned())
}

// Get the latest description for a node matching a specific model
pub async fn get_description_for_model(
    pool: &PgPool,
    node_id: &Uuid,
    model_name: &str,
) -> Result<Option<ImageDescription>, sqlx::Error> {
    let descriptions = get_descriptions_by_node(pool, node_id).await?;

    Ok(descriptions.iter().find(|d| d.model_name == model_name).cloned())
}

// Check if a description exists for the given node and model
pub async fn description_exists(
    pool: &PgPool,
    node_id: &Uuid,
    model_name: &str,
) -> Result<bool, sqlx::Error> {
    let result: (i32,) = sqlx::query_as(
        r#"SELECT COUNT(*) FROM image_descriptions WHERE node_id = $1 AND model_name = $2"#,
    )
    .bind(node_id)
    .bind(model_name)
    .fetch_one(pool)
    .await?;

    Ok(result.0 > 0)
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_resolve_node_data() {
        // This is a placeholder test - actual database connection needed
        // #[ignore]
        // fn requires_db() {}
    }
}
*/