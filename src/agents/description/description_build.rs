use crate::agents::description::DescriptionContent;
use rig::completion::Prompt;
use rig::message::{DocumentSourceKind, Message, UserContent};
use rig::prelude::*;
use rig::providers::ollama;
use rig::{completion::message::Image, message::ImageMediaType, OneOrMany};
use std::sync::Arc;
/*
// Generate the system prompt for LLM to produce DescriptionContent format

// Extract and parse DescriptionContent from LLM response
pub fn extract_description_content(
    llm_response: &str,
) -> Result<DescriptionContent, Box<dyn std::error::Error + Send + Sync>> {
    // Clean the response - remove potential markdown code blocks
    let cleaned = llm_response
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    // Parse JSON
    let content: DescriptionContent = serde_json::from_str(cleaned)?;

    // Validate that description is not empty
    if content.description.trim().is_empty() {
        return Err("Description field is empty".into());
    }

    Ok(content)
}
*/
/// More robust extraction with fallback parsing
pub fn extract_description_content_robust(
    llm_response: &str,
) -> Result<DescriptionContent, Box<dyn std::error::Error + Send + Sync>> {
    // Step 1: Try direct parsing
    if let Ok(content) = serde_json::from_str::<DescriptionContent>(llm_response.trim()) {
        if !content.description.trim().is_empty() {
            return Ok(content);
        }
    }

    // Step 2: Remove markdown code blocks and try again
    let cleaned = llm_response
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    if let Ok(content) = serde_json::from_str::<DescriptionContent>(cleaned)
        && !content.description.trim().is_empty()
    {
        return Ok(content);
    }

    // Step 3: Try to find JSON object in the text
    if let Some(start) = llm_response.find('{') {
        if let Some(end) = llm_response.rfind('}') {
            let json_str = &llm_response[start..=end];
            if let Ok(content) = serde_json::from_str::<DescriptionContent>(json_str)
                && !content.description.trim().is_empty()
            {
                return Ok(content);
            }
        }
    }

    Err(format!(
        "Failed to parse DescriptionContent from response: {}",
        llm_response
    )
    .into())
}
/*
// Validate DescriptionContent structure
pub fn validate_description_content(
    content: &DescriptionContent,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if content.description.trim().is_empty() {
        return Err("Description cannot be empty".into());
    }

    // Optional: Add more validation rules
    if content.description.len() < 10 {
        return Err("Description is too short".into());
    }

    Ok(())
}
*/

/// Generate description content from image bytes using LLM
/// The image should already be resized to the target dimensions
pub async fn generate_description_from_image(
    client: &Arc<ollama::Client>,
    model: &str,
    image_bytes: &[u8],
    user_prompt: &str,
    system_prompt: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    // Convert image to base64
    let image_base64 = base64::Engine::encode(&base64::prelude::BASE64_STANDARD, image_bytes);

    // Create Image for prompt
    let image = Image {
        data: DocumentSourceKind::base64(&image_base64),
        media_type: Some(ImageMediaType::JPEG),
        ..Default::default()
    };

    // Create agent with system prompt
    let json = serde_json::json!({
        "format": "json"
    });

    let agent = client
        .agent(model)
        .additional_params(json)
        .preamble(system_prompt)
        .temperature(0.1)
        .build();

    // Send user prompt with image
    let response = agent
        .prompt(Message::User {
            content: OneOrMany::many(vec![
                UserContent::Text(user_prompt.into()),
                UserContent::Image(image),
            ])?,
        })
        .await?;

    Ok(response)
}

/*
// Generate description content from image file path
// This function handles loading, resizing, and LLM call
pub async fn generate_description(
    client: &Arc<ollama::Client>,
    model: &str,
    image_path: &str,
    user_prompt: &str,
    system_prompt: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    // Load image bytes
    let image_bytes = tokio::fs::read(image_path).await
        .map_err(|e| format!("Failed to read image: {}", e))?;

    // Resize to 1200x1200
    let resized_bytes = resize_image_to_bytes(&image_bytes, 1200, 1200)?;

    // Generate description
    generate_description_from_image(client, model, &resized_bytes, user_prompt, system_prompt).await
}
*/

/// Resize image to maximum dimensions while maintaining aspect ratio
/// Returns resized image as bytes
pub fn resize_image_to_bytes(
    image_bytes: &[u8],
    output_width: u32,
    output_height: u32,
) -> Result<Vec<u8>, image::ImageError> {
    // Open the image from bytes
    let img = image::load_from_memory(image_bytes)?;

    // If image is smaller than target, return as is
    if img.height() <= output_height && img.width() <= output_width {
        return Ok(image_bytes.to_vec());
    }

    // Resize using Lanczos3 filter for high quality
    let resized_img = img.resize(
        output_width,
        output_height,
        image::imageops::FilterType::Lanczos3,
    );

    // Encode as JPEG
    let mut bytes: Vec<u8> = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    resized_img.write_to(&mut cursor, image::ImageFormat::Jpeg)?;

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_clean_json() {
        let response = r#"{
            "description": "A modern room",
            "windows": "Two large windows",
            "doors": null,
            "radiators": null,
            "openings": null
        }"#;

        let result = extract_description_content(response);
        assert!(result.is_ok());
        let content = result.unwrap();
        assert_eq!(content.description, "A modern room");
        assert_eq!(content.windows, Some("Two large windows".to_string()));
    }

    #[test]
    fn test_extract_with_markdown() {
        let response = r#"```json
{
    "description": "A modern room",
    "windows": "Two large windows",
    "doors": null,
    "radiators": null,
    "openings": null
}
```"#;

        let result = extract_description_content(response);
        assert!(result.is_ok());
    }

    #[test]
    fn test_extract_with_text_around() {
        let response = r#"Here is the analysis:
{
    "description": "A modern room",
    "windows": null,
    "doors": null,
    "radiators": null,
    "openings": null
}
Hope this helps!"#;

        let result = extract_description_content_robust(response);
        assert!(result.is_ok());
    }
    fn extract_description_content(
        llm_response: &str,
    ) -> Result<DescriptionContent, Box<dyn std::error::Error + Send + Sync>> {
        // Clean the response - remove potential markdown code blocks
        let cleaned = llm_response
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        // Parse JSON
        let content: DescriptionContent = serde_json::from_str(cleaned)?;

        // Validate that description is not empty
        if content.description.trim().is_empty() {
            return Err("Description field is empty".into());
        }

        Ok(content)
    }
}
