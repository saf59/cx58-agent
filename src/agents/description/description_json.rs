use crate::db_description::ImageDescription;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

/// Parsed structure of the description JSON content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescriptionContent {
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub windows: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doors: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radiators: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openings: Option<String>,
}

/// Complete description data ready to send via SSE
#[derive(Debug, Clone, Serialize)]
pub struct DescriptionData {
    pub object: String,
    pub object_id: Uuid,
    pub date: String,
    pub date_id: Uuid,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub windows: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doors: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radiators: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openings: Option<String>,
    pub model_name: String,
    pub confidence: Option<f32>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Build complete description data from ImageDescription and caller context
pub fn build_description_data(
    image_desc: &ImageDescription,
    object: &str,
    object_id: &Uuid,
    date: &str,
) -> Result<DescriptionData, Box<dyn std::error::Error + Send + Sync>> {
    // Parse the JSON description field
    let content: DescriptionContent = serde_json::from_str(&image_desc.description)?;

    Ok(DescriptionData {
        object: object.to_string(),
        object_id: *object_id,
        date: date.to_string(),
        date_id: image_desc.node_id, // node_id is date_id
        description: content.description,
        windows: content.windows,
        doors: content.doors,
        radiators: content.radiators,
        openings: content.openings,
        model_name: image_desc.model_name.clone(),
        confidence: image_desc.confidence,
        created_at: image_desc.created_at,
    })
}

/// Alternative: build as generic JSON value for maximum flexibility
pub fn build_description_json(
    image_desc: &ImageDescription,
    object: &str,
    object_id: &Uuid,
    date: &str,
) -> Result<JsonValue, Box<dyn std::error::Error + Send + Sync>> {
    // Parse the JSON description field
    let mut content: JsonValue = serde_json::from_str(&image_desc.description)?;

    // Add caller context fields
    if let Some(obj) = content.as_object_mut() {
        obj.insert("object".to_string(), serde_json::json!(object));
        obj.insert("object_id".to_string(), serde_json::json!(object_id));
        obj.insert("date".to_string(), serde_json::json!(date));
        obj.insert("date_id".to_string(), serde_json::json!(image_desc.node_id));
        obj.insert("model_name".to_string(), serde_json::json!(image_desc.model_name));
        obj.insert("confidence".to_string(), serde_json::json!(image_desc.confidence));
        obj.insert("created_at".to_string(), serde_json::json!(image_desc.created_at));
    }

    Ok(content)
}

/*
// Example usage in SSE streaming context
pub async fn send_description_via_sse(
    image_desc: ImageDescription,
    object: &str,
    object_id: &Uuid,
    date: &str,
    _request_id: String,
    // sender: your SSE sender type
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    // Build the complete description data
    let description_data = build_description_data(
        &image_desc,
        object,
        object_id,
        date,
    )?;

    // Convert to JSON
    let json_data = serde_json::to_value(description_data)?;

    // Send via SSE (example - adapt to your actual SSE implementation)
    // self.send_event(StreamEvent::DescriptionChunk {
    //     request_id,
    //     data: json_data,
    // }).await;

    Ok(json_data)
}
 */