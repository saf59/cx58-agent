use crate::agents::agent_error::AgentError;
use crate::agents::agents_helper::extract_text_from_choice;
use crate::agents::description::DescriptionContent;
use rig::completion::{CompletionError, CompletionModel};
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
) -> Result<(String, Option<u64>, u32), AgentError> {
    // Convert image to base64
    let image_base64 = base64::Engine::encode(&base64::prelude::BASE64_STANDARD, image_bytes);

    // Create Image for prompt
    let image = Image {
        data: DocumentSourceKind::base64(&image_base64),
        media_type: Some(ImageMediaType::JPEG),
        ..Default::default()
    };

    let model_name = model;
    let model = client.completion_model(model_name);
    let content = OneOrMany::many(vec![
        UserContent::Text(user_prompt.into()),
        UserContent::Image(image),
    ])
    .map_err(AgentError::internal)?;

    // Ollama reads the generation limit from options.num_predict.
    let mut request = model
        .completion_request(Message::User { content })
        .preamble(system_prompt.to_string())
        .temperature(0.2)
        .additional_params(serde_json::json!({"num_predict": 1024}))
        .build();
    let tokens = match description_completion(model.completion(request.clone()).await)? {
        (Some(text), used) => return Ok((text, Some(used), 1)),
        (None, used) => used,
    };

    tracing::warn!(model = %model_name, "Ollama returned an empty photo description; retrying once");
    request.additional_params = Some(serde_json::json!({"num_predict": 2048}));
    request.output_schema = Some(
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {"description": {"type": "string"}},
            "required": ["description"],
            "additionalProperties": true
        }))
        .map_err(AgentError::internal)?,
    );
    match description_completion(model.completion(request).await)? {
        (Some(text), used) => Ok((text, Some(tokens + used), 2)),
        (None, _) => Err(AgentError::EmptyPhotoDescription),
    }
}

fn description_completion(
    result: Result<
        rig::completion::CompletionResponse<ollama::CompletionResponse>,
        CompletionError,
    >,
) -> Result<(Option<String>, u64), AgentError> {
    match result {
        Ok(response) => {
            let text = extract_text_from_choice(response.choice);
            Ok((
                (!text.trim().is_empty()).then_some(text),
                response.usage.total_tokens,
            ))
        }
        // Rig discards the raw response and its token counts when content and tool calls are empty.
        Err(CompletionError::ResponseError(message)) if message == "No content provided" => {
            Ok((None, 0))
        }
        Err(error) => Err(error.into()),
    }
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

    async fn run_vision_fixture(
        responses: &[&'static str],
    ) -> Result<(String, Option<u64>, u32), AgentError> {
        use axum::{Json, Router, extract::State, routing::post};
        use serde_json::{Value, json};
        use std::sync::Mutex;
        type Responses = Arc<Mutex<std::iter::Enumerate<std::vec::IntoIter<&'static str>>>>;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = Arc::new(
            ollama::Client::builder()
                .api_key(rig::client::Nothing)
                .base_url(format!("http://{}", listener.local_addr().unwrap()))
                .build()
                .unwrap(),
        );
        let responses = Arc::new(Mutex::new(responses.to_vec().into_iter().enumerate()));
        let app = Router::new()
            .route(
                "/api/chat",
                post(
                    |State(responses): State<Responses>, Json(request): Json<Value>| async move {
                        let (attempt, content) =
                            responses.lock().unwrap().next().expect("unexpected retry");
                        assert_eq!(request["think"], false);
                        assert_eq!(request["messages"][0]["content"], "Use JSON");
                        assert_eq!(request["messages"][1]["content"], "Describe");
                        assert!(
                            !request["messages"][1]["images"][0]
                                .as_str()
                                .unwrap()
                                .is_empty()
                        );
                        assert_eq!(
                            request["options"]["num_predict"],
                            if attempt == 0 { 1024 } else { 2048 }
                        );
                        if attempt == 0 {
                            assert!(request.get("format").is_none());
                        } else {
                            assert_eq!(request["format"]["required"], json!(["description"]));
                        }
                        Json(json!({
                            "model": "qwen3-vl:8b",
                            "created_at": "2026-11-18T11:00:00Z",
                            "message": {"role": "assistant", "content": content},
                            "done": true,
                            "done_reason": if content.is_empty() { "length" } else { "stop" },
                            "prompt_eval_count": 2,
                            "eval_count": 1
                        }))
                    },
                ),
            )
            .with_state(responses.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let jpeg = resize_image_to_bytes(&small_png(), 1200, 1200).unwrap();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            generate_description_from_image(&client, "qwen3-vl:8b", &jpeg, "Describe", "Use JSON"),
        )
        .await;
        server.abort();
        assert_eq!(responses.lock().unwrap().len(), 0);
        result.unwrap()
    }

    #[tokio::test]
    async fn vision_request_sets_ollama_generation_limit() {
        let json = r#"{"description":"Room"}"#;
        assert_eq!(
            run_vision_fixture(&[json]).await.unwrap(),
            (json.to_string(), Some(3), 1)
        );
    }

    #[tokio::test]
    async fn retries_empty_ollama_response_with_more_tokens_and_json_format() {
        let json = r#"{"description":"Room","power_outlets":[]}"#;
        let (text, tokens, calls) = run_vision_fixture(&["", json]).await.unwrap();
        assert_eq!((tokens, calls), (Some(3), 2));
        assert_eq!(
            extract_description_content_robust(&text)
                .unwrap()
                .description,
            "Room"
        );
    }

    #[tokio::test]
    async fn stops_after_two_empty_ollama_responses() {
        assert!(matches!(
            run_vision_fixture(&[" \n", ""]).await,
            Err(AgentError::EmptyPhotoDescription)
        ));
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
