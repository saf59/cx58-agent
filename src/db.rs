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

// Ручная реализация для работы с PostgreSQL enum
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

/// Получить дерево узлов для пользователя
///
/// # Arguments
/// * `pool` - Пул подключений к базе данных
/// * `user_id` - ID пользователя
/// * `with_leafs` - Включать ли ImageLeaf узлы (по умолчанию true)
///
/// # Returns
/// Vec<TreeNode> - Вектор узлов дерева
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

/// Получить узел с его дочерними ImageLeaf
///
/// # Arguments
/// * `pool` - Пул подключений к базе данных
/// * `node_id` - ID узла
/// * `limit` - Количество ImageLeaf для получения (по умолчанию 1)
/// * `from_timestamp` - Начальная временная метка (опционально)
/// * `to_timestamp` - Конечная временная метка (опционально)
///
/// # Returns
/// Vec<NodeWithLeaf> - Вектор узлов с полными именами
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

/// Вставить новый ImageLeaf узел
///
/// # Arguments
/// * `pool` - Пул подключений к базе данных
/// * `parent_id` - ID родительского узла
/// * `url` - URL изображения
/// * `berlin_datetime` - Дата и время в формате "DD.MM.YYYY HH24:MI:SS" (берлинское время)
///
/// # Returns
/// Uuid - ID созданного узла
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

/// Получить полное имя узла
///
/// # Arguments
/// * `pool` - Пул подключений к базе данных
/// * `node_id` - ID узла
///
/// # Returns
/// String - Полное имя узла (путь через "/")
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

// ============================================================================
// Helper Functions
// ============================================================================

impl TreeNode {
    /// Проверить, является ли узел корневым
    pub fn is_root(&self) -> bool {
        self.node_type == NodeType::Root
    }

    /// Проверить, является ли узел веткой
    pub fn is_branch(&self) -> bool {
        self.node_type == NodeType::Branch
    }

    /// Проверить, является ли узел листом
    pub fn is_leaf(&self) -> bool {
        self.node_type == NodeType::ImageLeaf
    }

    /// Получить URL изображения для ImageLeaf узла
    pub fn get_image_url(&self) -> Option<String> {
        if self.is_leaf() {
            self.data
                .get("url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        } else {
            None
        }
    }
}

impl NodeWithLeaf {
    /// Проверить, является ли узел листом
    pub fn is_leaf(&self) -> bool {
        self.node_type == NodeType::ImageLeaf
    }

    /// Получить URL изображения для ImageLeaf узла
    pub fn get_image_url(&self) -> Option<String> {
        if self.is_leaf() {
            self.data
                .get("url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        } else {
            None
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
        // Инициализация пула (замените на ваши настройки)
        let pool = PgPool::connect(&db_url)
            .await
            .expect("Failed to connect to database");

        // Пример 1: Получить дерево для пользователя
        let tree = get_tree(&pool, &user_id, true)
            .await
            .expect("Failed to get tree");

        println!("Tree nodes count: {}", tree.len());
        for node in &tree {
            println!(
                "Node: {} ({}), depth: {}, own: {}",
                node.name.as_ref().unwrap_or(&"<unnamed>".to_string()),
                node.node_type,
                node.depth,
                node.own
            );
        }

        // Пример 2: Получить узел с последними 5 листьями
        let node_id = tree.first().unwrap().id;
        let nodes_with_leafs = get_node_with_leafs(&pool, node_id, Some(5), None, None)
            .await
            .expect("Failed to get node with leafs");

        println!("\nNode with leafs:");
        for node in &nodes_with_leafs {
            println!(
                "Node: {} - {}",
                node.full_name.as_ref().unwrap_or(&"<unnamed>".to_string()),
                node.node_type
            );
            if let Some(url) = node.get_image_url() {
                println!("  Image URL: {}", url);
            }
        }

        // Пример 3: Получить узлы с временными рамками
        use chrono::NaiveDate;
        let from_date = NaiveDate::from_ymd_opt(2025, 12, 20)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();

        let nodes_filtered = get_node_with_leafs(&pool, node_id, Some(1), Some(from_date), None)
            .await
            .expect("Failed to get filtered nodes");

        println!("\nFiltered nodes:");
        for node in &nodes_filtered {
            println!(
                "Node: {} at {}",
                node.full_name.as_ref().unwrap_or(&"<unnamed>".to_string()),
                node.updated_at
            );
        }

        // Пример 4: Вставить новый ImageLeaf
        let new_leaf_id = insert_image_leaf(&pool, node_id, "new_image.jpg", "28.12.2025 15:30:00")
            .await
            .expect("Failed to insert image leaf");

        println!("\nNew leaf created with ID: {}", new_leaf_id);

        // Пример 5: Получить полное имя узла
        let full_name = get_full_node_name(&pool, new_leaf_id)
            .await
            .expect("Failed to get full name");

        println!("Full name: {}", full_name.unwrap_or("<none>".to_string()));
    }
}

// ============================================================================
// Display Implementations
// ============================================================================

impl std::fmt::Display for NodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeType::Root => write!(f, "Root"),
            NodeType::Branch => write!(f, "Branch"),
            NodeType::ImageLeaf => write!(f, "ImageLeaf"),
        }
    }
}
