use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

/// Represents a row in the image_descriptions table
#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct ImageDescription {
    pub id: Uuid,
    pub node_id: Uuid,
    pub model_name: String,
    pub description: String,
    pub confidence: Option<f32>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Input structure for creating a new image description
#[derive(Debug, Clone)]
pub struct CreateImageDescription {
    pub node_id: Uuid,
    pub model_name: String,
    pub description: String,
    pub confidence: Option<f32>,
}

/// Convert CreateImageDescription to ImageDescription (with dummy id and current timestamp)
impl From<CreateImageDescription> for ImageDescription {
    fn from(create: CreateImageDescription) -> Self {
        ImageDescription {
            id: Uuid::now_v7(),
            node_id: create.node_id,
            model_name: create.model_name,
            description: create.description,
            confidence: create.confidence,
            created_at: chrono::Utc::now(),
        }
    }
}

/// Get all image descriptions for a specific node
pub async fn get_descriptions_by_node(
    pool: &PgPool,
    node_id: &Uuid,
    model: &str,
    lang_id: &str,
) -> Result<Vec<ImageDescription>, sqlx::Error> {
    sqlx::query_as::<_, ImageDescription>(
        r#"
        SELECT id, node_id, model_name, description, confidence, created_at
        FROM image_descriptions
        WHERE node_id = $1 and model_name = $2 and lang = $3
        ORDER BY created_at DESC
        "#,
    )
    .bind(node_id)
    .bind(model)
    .bind(lang_id)
    .fetch_all(pool)
    .await
}

/// Get a specific image description by node_id, model_name, and prompt
pub async fn get_description(
    pool: &PgPool,
    node_id: &Uuid,
    model_name: &str,
    lang_code: &str,
) -> Result<Option<ImageDescription>, sqlx::Error> {
    sqlx::query_as::<_, ImageDescription>(
        r#"
        SELECT id, node_id, model_name,  description, confidence, created_at
        FROM image_descriptions
        WHERE node_id = $1 AND model_name = $2 AND lang = $3
        "#,
    )
    .bind(node_id)
    .bind(model_name)
    .bind(lang_code)
    .fetch_optional(pool)
    .await
}

/// Insert or update an image description (upsert based on UNIQUE constraint)
pub async fn upsert_description(
    pool: &PgPool,
    data: &CreateImageDescription,
    lang_code: &str,
) -> Result<ImageDescription, sqlx::Error> {
    sqlx::query_as::<_, ImageDescription>(
        r#"
        INSERT INTO image_descriptions (node_id, model_name, description, confidence, lang)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (node_id, model_name, lang)
        DO UPDATE SET
            description = EXCLUDED.description,
            confidence = EXCLUDED.confidence,
            model_name = EXCLUDED.model_name,
            created_at = NOW()
        RETURNING id, node_id, model_name, description, confidence, created_at
        "#,
    )
    .bind(&data.node_id)
    .bind(&data.model_name)
    .bind(&data.description)
    .bind(data.confidence)
    .bind(lang_code)
    .fetch_one(pool)
    .await
}

/// Insert a new image description (returns error if already exists)
pub async fn create_description(
    pool: &PgPool,
    data: &CreateImageDescription,
) -> Result<ImageDescription, sqlx::Error> {
    sqlx::query_as::<_, ImageDescription>(
        r#"
        INSERT INTO image_descriptions (node_id, model_name, description, confidence)
        VALUES ($1, $2, $3, $4)
        RETURNING id, node_id, model_name, description, confidence, created_at
        "#,
    )
    .bind(&data.node_id)
    .bind(&data.model_name)
    .bind(&data.description)
    .bind(data.confidence)
    .fetch_one(pool)
    .await
}

/// Delete all image descriptions for a specific node
pub async fn delete_descriptions_by_node(
    pool: &PgPool,
    node_id: &Uuid,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        DELETE FROM image_descriptions
        WHERE node_id = $1
        "#,
    )
    .bind(node_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

/// Delete a specific image description by node_id, model_name, and prompt
pub async fn delete_description(
    pool: &PgPool,
    node_id: &Uuid,
    model_name: &str,
    lang_code: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        DELETE FROM image_descriptions
        WHERE node_id = $1 AND model_name = $2 AND lang = $3
        "#,
    )
    .bind(node_id)
    .bind(model_name)
    .bind(lang_code)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Delete an image description by its ID
pub async fn delete_description_by_id(pool: &PgPool, id: &Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        DELETE FROM image_descriptions
        WHERE id = $1
        "#,
    )
    .bind(id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}
