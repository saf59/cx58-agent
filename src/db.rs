use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;
// ============================================================================
// Enums
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum NodeType {
    Root,
    Branch,
    ImageLeaf,
}

// Hand made Implementation for PostgreSQL enum
impl sqlx::Type<sqlx::Postgres> for NodeType {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("node_type_enum")
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for NodeType {
    fn decode(value: sqlx::postgres::PgValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let s = <&str as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        match s {
            "Root" => Ok(NodeType::Root),
            "Branch" => Ok(NodeType::Branch),
            "ImageLeaf" => Ok(NodeType::ImageLeaf),
            _ => Err(format!("Unknown node type: {}", s).into()),
        }
    }
}

impl sqlx::Encode<'_, sqlx::Postgres> for NodeType {
    fn encode_by_ref(
        &self,
        buf: &mut sqlx::postgres::PgArgumentBuffer,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        let s = match self {
            NodeType::Root => "Root",
            NodeType::Branch => "Branch",
            NodeType::ImageLeaf => "ImageLeaf",
        };
        <&str as sqlx::Encode<sqlx::Postgres>>::encode_by_ref(&s, buf)
    }
}

// ============================================================================
// Structures
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeNode {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub node_type: NodeType,
    pub name: Option<String>,
    pub data: sqlx::types::JsonValue,
    pub path: String,
    pub updated_at: NaiveDateTime,
    pub depth: i32,
    pub own: bool,
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for TreeNode {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;

        Ok(TreeNode {
            id: row.try_get("id")?,
            parent_id: row.try_get("parent_id")?,
            node_type: row.try_get("node_type")?,
            name: row.try_get("name")?,
            data: row.try_get("data")?,
            path: row.try_get("path")?,
            updated_at: row.try_get("updated_at")?,
            depth: row.try_get("depth")?,
            own: row.try_get("own")?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeWithLeaf {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub node_type: NodeType,
    pub name: Option<String>,
    pub data: sqlx::types::JsonValue,
    pub path: String,
    pub updated_at: NaiveDateTime,
    pub full_name: Option<String>,
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for NodeWithLeaf {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;

        Ok(NodeWithLeaf {
            id: row.try_get("id")?,
            parent_id: row.try_get("parent_id")?,
            node_type: row.try_get("node_type")?,
            name: row.try_get("name")?,
            data: row.try_get("data")?,
            path: row.try_get("path")?,
            updated_at: row.try_get("updated_at")?,
            full_name: row.try_get("full_name")?,
        })
    }
}

// ============================================================================
// Database Functions
// ============================================================================
pub async fn evict_expired_cache(pool: &PgPool) {
    // Descriptions older than N days
    match sqlx::query!(
        "DELETE FROM image_descriptions WHERE created_at <= (NOW() - INTERVAL '30 days')"
    )
    .execute(pool)
    .await
    {
        Ok(r) => tracing::info!(deleted = r.rows_affected(), "Cache eviction: descriptions"),
        Err(e) => tracing::error!("Cache eviction failed (descriptions): {e}"),
    }

    // Comparisons older than N days
    match sqlx::query!("DELETE FROM comparison WHERE created_at <= (NOW() - INTERVAL '30 days')")
        .execute(pool)
        .await
    {
        Ok(r) => tracing::info!(deleted = r.rows_affected(), "Cache eviction: comparisons"),
        Err(e) => tracing::error!("Cache eviction failed (comparisons): {e}"),
    }
}

/// Get users node tree
///
/// # Arguments
/// * `pool` - db pool
/// * `user_id` - user_id
/// * `with_leafs` - show ImageLeaf (default true)
///
/// # Returns
/// Vec<TreeNode> - nodes
pub async fn get_tree(
    pool: &PgPool,
    user_id: &str,
    with_leafs: bool,
) -> Result<Vec<TreeNode>, sqlx::Error> {
    sqlx::query_as::<_, TreeNode>(
        r#"
        SELECT * FROM get_tree($1, $2)
        "#,
    )
    .bind(user_id)
    .bind(with_leafs)
    .fetch_all(pool)
    .await
}

/// Receive node with its ImageLeafs
///
/// # Arguments
/// * `pool` - db pool
/// * `node_id` - node id
/// * `limit` - Amount of ImageLeaf for getting (default 1)
/// * `from_timestamp` - Start timestamp (optional)
/// * `to_timestamp` - End timestamp (optional)
///
/// # Returns
/// Vec<NodeWithLeaf> - Owner with_leafs
pub async fn get_node_with_leafs(
    pool: &PgPool,
    node_id: Uuid,
    limit: Option<i32>,
    from_timestamp: Option<NaiveDateTime>,
    to_timestamp: Option<NaiveDateTime>,
) -> Result<Vec<NodeWithLeaf>, sqlx::Error> {
    sqlx::query_as::<_, NodeWithLeaf>(
        r#"
        SELECT * FROM get_node_with_leafs($1, $2, $3, $4)
        "#,
    )
    .bind(node_id)
    .bind(limit.unwrap_or(1))
    .bind(from_timestamp)
    .bind(to_timestamp)
    .fetch_all(pool)
    .await
}

/// Insert new ImageLeaf node
///
/// # Arguments
/// * `pool` - db pool
/// * `parent_id` - node id
/// * `url` - image URL
/// * `berlin_datetime` - like "DD.MM.YYYY HH24:MI:SS"
///
/// # Returns
/// Uuid - ID of the created node
pub async fn insert_image_leaf(
    pool: &PgPool,
    parent_id: Uuid,
    url: &str,
    berlin_datetime: &str,
) -> Result<Uuid, sqlx::Error> {
    let result: (Uuid,) = sqlx::query_as(
        r#"
        SELECT insert_image_leaf($1, $2, $3)
        "#,
    )
    .bind(parent_id)
    .bind(url)
    .bind(berlin_datetime)
    .fetch_one(pool)
    .await?;

    Ok(result.0)
}
pub async fn update_leaf_datetime(
    pool: &PgPool,
    url: &str,
    berlin_datetime: &str,
) -> Result<Uuid, sqlx::Error> {
    let result: (Uuid,) = sqlx::query_as(
        r#"
        UPDATE tree_nodes
        SET updated_at = timezone('UTC', to_timestamp($2, 'DD.MM.YYYY HH24:MI:SS') AT TIME ZONE 'Europe/Berlin')
        WHERE data->>'src' = $1
        RETURNING id
        "#,
    )
        .bind(url)
        .bind(berlin_datetime)
        .fetch_one(pool)
        .await?;

    Ok(result.0)
}

/// Retrieve full node name
///
/// # Arguments
/// * `pool` - db pool
/// * `node_id` - node id
///
/// # Returns
/// String - Full node name (path through "/")
pub async fn get_full_node_name(
    pool: &PgPool,
    node_id: Uuid,
) -> Result<Option<String>, sqlx::Error> {
    let result: (Option<String>,) = sqlx::query_as(
        r#"
        SELECT get_full_node_name($1)
        "#,
    )
    .bind(node_id)
    .fetch_one(pool)
    .await?;

    Ok(result.0)
}

pub async fn get_id_by_name(pool: &PgPool, node_name: &str) -> Result<Option<Uuid>, sqlx::Error> {
    let result = sqlx::query_scalar::<_, Uuid>("SELECT id FROM tree_nodes WHERE name = $1 LIMIT 1")
        .bind(node_name)
        .fetch_optional(pool)
        .await?;

    Ok(result)
}

// ============================================================================
// Helper Functions
// ============================================================================

impl TreeNode {
    pub fn is_root(&self) -> bool {
        self.node_type == NodeType::Root
    }

    pub fn is_branch(&self) -> bool {
        self.node_type == NodeType::Branch
    }
}
impl Leaf for TreeNode {
    fn node_type(&self) -> &NodeType {
        &self.node_type
    }
    fn data(&self) -> &serde_json::Value {
        &self.data
    }
}

impl Leaf for NodeWithLeaf {
    fn node_type(&self) -> &NodeType {
        &self.node_type
    }
    fn data(&self) -> &serde_json::Value {
        &self.data
    }
}
pub trait Leaf {
    fn node_type(&self) -> &NodeType;
    fn data(&self) -> &serde_json::Value;

    fn is_leaf(&self) -> bool {
        *self.node_type() == NodeType::ImageLeaf
    }

    fn get_image_url(&self) -> Option<String> {
        if self.is_leaf() {
            self.data()
                .get("url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        } else {
            None
        }
    }
}

impl std::fmt::Display for NodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeType::Root => write!(f, "Root"),
            NodeType::Branch => write!(f, "Branch"),
            NodeType::ImageLeaf => write!(f, "ImageLeaf"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[tokio::test]
    async fn example_usage() {
        dotenv::from_path(".env.test").ok();

        // Now you can read the variables using std::env::var
        let db_url = env::var("DATABASE_URL")
            .expect("DATABASE_URL must be set in .env.test file or environment");
        let user_id = env::var("TEST_USER").expect("TEST_USER must be set");
        // Initialize pool (replace with your settings)
        let pool = PgPool::connect(&db_url)
            .await
            .expect("Failed to connect to database");

        // Example 1: Get tree for user
        let tree = get_tree(&pool, &user_id, true)
            .await
            .expect("Failed to get tree");

        tracing::info!("Tree nodes count: {}", tree.len());
        for node in &tree {
            tracing::info!(
                "Node: {} ({}), depth: {}, own: {}",
                node.name.as_ref().unwrap_or(&"<unnamed>".to_string()),
                node.node_type,
                node.depth,
                node.own
            );
        }

        // Example 2: Get node with last 5 leafs
        let node_id = tree.first().unwrap().id;
        let nodes_with_leafs = get_node_with_leafs(&pool, node_id, Some(5), None, None)
            .await
            .expect("Failed to get node with leafs");

        tracing::info!("\nNode with leafs:");
        for node in &nodes_with_leafs {
            tracing::info!(
                "Node: {} - {}",
                node.full_name.as_ref().unwrap_or(&"<unnamed>".to_string()),
                node.node_type
            );
            if let Some(url) = node.get_image_url() {
                tracing::info!("  Image URL: {}", url);
            }
        }

        // Example 3: Get nodes with time frames
        use chrono::NaiveDate;
        let from_date = NaiveDate::from_ymd_opt(2025, 12, 20)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();

        let nodes_filtered = get_node_with_leafs(&pool, node_id, Some(1), Some(from_date), None)
            .await
            .expect("Failed to get filtered nodes");

        tracing::info!("\nFiltered nodes:");
        for node in &nodes_filtered {
            tracing::info!(
                "Node: {} at {}",
                node.full_name.as_ref().unwrap_or(&"<unnamed>".to_string()),
                node.updated_at
            );
        }

        // Example 4: Insert new ImageLeaf
        let new_leaf_id = insert_image_leaf(&pool, node_id, "new_image.jpg", "28.12.2025 15:30:00")
            .await
            .expect("Failed to insert image leaf");

        tracing::info!("\nNew leaf created with ID: {}", new_leaf_id);

        // Example 5: Get full name of node
        let full_name = get_full_node_name(&pool, new_leaf_id)
            .await
            .expect("Failed to get full name");

        tracing::info!("Full name: {}", full_name.unwrap_or("<none>".to_string()));
    }
}
