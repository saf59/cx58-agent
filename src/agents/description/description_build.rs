use crate::agents::agent_error::AgentError;
use crate::agents::agents_helper::extract_text_from_choice;
use crate::agents::description::DescriptionContent;
use rig::completion::CompletionModel;
use rig::message::{DocumentSourceKind, Message, UserContent};
use rig::prelude::CompletionClient;
use rig::providers::ollama;
use rig::{OneOrMany, completion::message::Image, message::ImageMediaType};
use std::sync::Arc;

/// More robust extraction with fallback parsing
pub fn extract_description_content_robust(
    llm_response: &str,
) -> Result<DescriptionContent, Box<dyn std::error::Error + Send + Sync>> {
    // Step 1: Try direct parsing
    if let Ok(content) = serde_json::from_str::<DescriptionContent>(llm_response.trim())
        && !content.description.trim().is_empty()
    {
        return Ok(content);
    }

    // Step 2: RemoveMarkdown code blocks and try again
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
    // Step 2.5: LLM wrapped the JSON in quotes — unescape and try again
    if let Ok(inner) = serde_json::from_str::<String>(llm_response.trim()) {
        if let Ok(content) = serde_json::from_str::<DescriptionContent>(inner.trim())
            && !content.description.trim().is_empty()
        {
            return Ok(content);
        }
        // Also try find { } inside the unescaped string
        if let Some(start) = inner.find('{')
            && let Some(end) = inner.rfind('}')
        {
            let json_str = &inner[start..=end];
            if let Ok(content) = serde_json::from_str::<DescriptionContent>(json_str)
                && !content.description.trim().is_empty()
            {
                return Ok(content);
            }
        }
    }

    // Step 3: Try to find JSON object in the text
    if let Some(start) = llm_response.find('{')
        && let Some(end) = llm_response.rfind('}')
    {
        let json_str = &llm_response[start..=end];
        if let Ok(content) = serde_json::from_str::<DescriptionContent>(json_str)
            && !content.description.trim().is_empty()
        {
            return Ok(content);
        }
    }

    Err("Failed to parse DescriptionContent from model response".into())
}

/// Generate description content from image bytes using LLM
/// The image should already be resized to the target dimensions
pub async fn generate_description_from_image(
    client: &Arc<ollama::Client>,
    model: &str,
    image_bytes: &[u8],
    user_prompt: &str,
    system_prompt: &str,
) -> Result<(String, Option<u64>), AgentError> {
    // Convert image to base64
    let image_base64 = base64::Engine::encode(&base64::prelude::BASE64_STANDARD, image_bytes);

    // Create Image for prompt
    let image = Image {
        data: DocumentSourceKind::base64(&image_base64),
        media_type: Some(ImageMediaType::JPEG),
        ..Default::default()
    };

    let model = client.completion_model(model);
    let content = OneOrMany::many(vec![
        UserContent::Text(user_prompt.into()),
        UserContent::Image(image),
    ])
    .map_err(AgentError::internal)?;

    // Limit output tokens — a structured JSON description needs at most ~1024 tokens.
    // Without this limit qwen3-vl:8b generates 4000-6000 tokens of reasoning before
    // the actual JSON, causing 2-5 minute timeouts.
    let request = model
        .completion_request(Message::User { content })
        .preamble(system_prompt.to_string())
        .temperature(0.2)
        .max_tokens(1024)
        .build();
    let response = model.completion(request).await?;

    let choice = response.choice.clone();
    let text = extract_text_from_choice(choice);

    if text.is_empty() {
        let err_msg = format!(
            "Orchestrator LLM returned no text content for image-based description generation. Response: {:?}",
            response.choice
        );
        tracing::error!("{}", err_msg);
        let err = AgentError::internal(err_msg.clone());
        return Err(err);
    }

    let tokens = Some(
        response.raw_response.prompt_eval_count.unwrap_or(0)
            + response.raw_response.eval_count.unwrap_or(0),
    );

    Ok((text, tokens))
}

/// Resize image to maximum dimensions while maintaining aspect ratio
/// Returns resized image as bytes
pub fn resize_image_to_bytes(
    image_bytes: &[u8],
    output_width: u32,
    output_height: u32,
) -> Result<Vec<u8>, image::ImageError> {
    // Open the image from bytes
    let img = image::load_from_memory(image_bytes)?;

    // Normalize every input to JPEG so the declared model media type is truthful.
    let normalized_img = if img.height() <= output_height && img.width() <= output_width {
        img
    } else {
        img.resize(
            output_width,
            output_height,
            image::imageops::FilterType::Lanczos3,
        )
    };

    // Encode as JPEG
    let mut bytes: Vec<u8> = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    normalized_img.write_to(&mut cursor, image::ImageFormat::Jpeg)?;

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb};
    use std::io::Cursor;

    /// Encodes a small PNG fixture in memory.
    fn small_png() -> Vec<u8> {
        let image = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(2, 2, Rgb([1, 2, 3])));
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    /// Verifies that small non-JPEG inputs match the declared JPEG media type.
    #[test]
    fn resize_normalizes_small_png_to_jpeg() {
        let bytes = resize_image_to_bytes(&small_png(), 1200, 1200).unwrap();

        assert_eq!(image::guess_format(&bytes).unwrap(), ImageFormat::Jpeg);
    }

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
        assert_eq!(content.prompt_version, None);
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

    #[test]
    fn prompt_version_marks_current_cache_without_affecting_legacy_json() {
        let response = r#"{
            "description": "A modern room",
            "doors": "Installation complete"
        }"#;

        let mut content = extract_description_content_robust(response).unwrap();
        assert!(!content.uses_current_prompt());

        content.mark_current_prompt();
        assert!(content.uses_current_prompt());
        assert_eq!(
            serde_json::to_value(content).unwrap()["_prompt_version"],
            crate::agents::description::description_json::DESCRIPTION_PROMPT_VERSION
        );
    }

    #[test]
    fn structured_element_fields_are_normalized_to_text() {
        let response = r#"{
            "description": "Raum im Bauzustand.",
            "windows": [],
            "doors": [{
                "Anzahl": 1,
                "Typ": "Schwingtür",
                "Material": "Holz",
                "Griff": "silber",
                "Zustand": "installiert",
                "Einbauphase": "installiert"
            }],
            "radiators": [],
            "openings": []
        }"#;

        let content = extract_description_content_robust(response).unwrap();

        assert_eq!(content.windows, None);
        assert_eq!(
            content.doors.as_deref(),
            Some(
                "Anzahl: 1; Einbauphase: installiert; Griff: silber; Material: Holz; Typ: Schwingtür; Zustand: installiert"
            )
        );
        assert_eq!(content.radiators, None);
        assert_eq!(content.openings, None);
    }

    fn extract_description_content(
        llm_response: &str,
    ) -> Result<DescriptionContent, Box<dyn std::error::Error + Send + Sync>> {
        // Clean the response - remove potentialMarkdown code blocks
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
