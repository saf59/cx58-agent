use sqlx::PgPool;
use uuid::Uuid;
use crate::db_description::{create_description, get_description, upsert_description, CreateImageDescription, ImageDescription};

/// Get or create an image description for a node
///
/// First attempts to retrieve an existing description from the database.
/// If not found, generates a new description using describe_image(),
/// saves it to the database, and returns it.
pub async fn get_or_create_description(
    pool: &PgPool,
    node_id: &Uuid,
    model_name: &str,
    prompt: &str,
) -> Result<ImageDescription, Box<dyn std::error::Error + Send + Sync>> {
    // Try to get existing description from database
    match get_description(pool, node_id, model_name, prompt).await? {
        Some(description) => Ok(description),
        None => {
            // Description not found, generate a new one
            let new_description = describe_image(node_id).await?;

            // Prepare data for insertion
            let create_data = CreateImageDescription {
                node_id: *node_id,
                model_name: model_name.to_string(),
                prompt: prompt.to_string(),
                description: new_description.description.clone(),
                confidence: new_description.confidence,
            };

            // Save to database and return
            let saved = create_description(pool, &create_data).await?;
            Ok(saved)
        }
    }
}

/// Alternative version if describe_image returns just a string description
pub async fn get_or_create_description_simple(
    pool: &PgPool,
    node_id: &Uuid,
    model_name: &str,
    prompt: &str,
) -> Result<ImageDescription, Box<dyn std::error::Error + Send + Sync>> {
    // Try to get existing description from database
    match get_description(pool, node_id, model_name, prompt).await? {
        Some(description) => Ok(description),
        None => {
            // Description not found, generate a new one
            let generated_text = describe_image_simple(node_id).await?;

            // Prepare data for insertion
            let create_data = CreateImageDescription {
                node_id: *node_id,
                model_name: model_name.to_string(),
                prompt: prompt.to_string(),
                description: generated_text,
                confidence: None,
            };

            // Save to database and return
            let saved = create_description(pool, &create_data).await?;
            Ok(saved)
        }
    }
}

/// Version using upsert instead of create (safer if concurrent requests possible)
pub async fn get_or_create_description_upsert(
    pool: &PgPool,
    node_id: &Uuid,
    model_name: &str,
    prompt: &str,
) -> Result<ImageDescription, Box<dyn std::error::Error + Send + Sync>> {
    // Try to get existing description from database
    if let Some(description) = get_description(pool, node_id, model_name, prompt).await? {
        return Ok(description);
    }

    // Description not found, generate a new one
    let new_description = describe_image(node_id).await?;

    // Prepare data for insertion
    let create_data = CreateImageDescription {
        node_id: *node_id,
        model_name: model_name.to_string(),
        prompt: prompt.to_string(),
        description: new_description.description.clone(),
        confidence: new_description.confidence,
    };

    // Use upsert to handle potential race conditions
    let saved = upsert_description(pool, &create_data).await?;
    Ok(saved)
}

// Placeholder for the actual describe_image function
async fn describe_image(
    node_id: &Uuid,
) -> Result<ImageDescription, Box<dyn std::error::Error + Send + Sync>> {
    // TODO: Implement actual image description logic
    unimplemented!("describe_image should be implemented with actual AI/ML model")
}

// Alternative placeholder if describe_image returns just a string
async fn describe_image_simple(
    node_id: &Uuid,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    // TODO: Implement actual image description logic
    unimplemented!("describe_image_simple should be implemented with actual AI/ML model")
}