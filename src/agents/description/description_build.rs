use serde::{Deserialize, Serialize};

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

/// Generate the system/user prompt for LLM to produce DescriptionContent format
pub fn create_description_prompt(additional_context: Option<&str>) -> String {
    let context = additional_context.unwrap_or("");

    format!(
        r#"You are an expert at analyzing architectural and construction images.
Analyze the provided image and generate a detailed description.

{context}

You MUST respond with ONLY a valid JSON object in this exact format:
{{
  "description": "General and complete description of the object in the image",
  "windows": "Detailed information about windows visible in the image (or null if none)",
  "doors": "Detailed information about doors visible in the image (or null if none)",
  "radiators": "Detailed information about radiators/heating elements visible in the image (or null if none)",
  "openings": "Detailed information about openings (arches, passages, etc.) visible in the image (or null if none)"
}}

IMPORTANT RULES:
1. Return ONLY the JSON object, no additional text before or after
2. All field values must be strings or null
3. The "description" field is REQUIRED and must contain a comprehensive description
4. Other fields (windows, doors, radiators, openings) should be null if the respective elements are not visible in the image
5. Be specific and detailed in your descriptions
6. Use proper JSON formatting with double quotes
7. Do not include markdown code blocks or any other formatting"#,
        context = context
    )
}

/// Alternative: Create prompt with schema example
pub fn create_description_prompt_with_schema() -> String {
    r#"You are an expert at analyzing architectural and construction images.
Analyze the provided image and generate a detailed description in JSON format.

REQUIRED OUTPUT FORMAT:
Return ONLY a valid JSON object matching this schema:

{
  "description": "string (REQUIRED) - General and complete description of the object",
  "windows": "string or null - Detailed information about windows only",
  "doors": "string or null - Detailed information about doors only",
  "radiators": "string or null - Detailed information about radiators only",
  "openings": "string or null - Detailed information about openings only"
}

EXAMPLE OUTPUT:
{
  "description": "A modern residential room with white walls and wooden flooring",
  "windows": "Two large double-glazed windows on the eastern wall with white frames",
  "doors": "Single wooden door with silver handle on the northern wall",
  "radiators": "White panel radiator mounted beneath the window",
  "openings": null
}

CRITICAL INSTRUCTIONS:
- Return ONLY the JSON object, no additional text
- Do not wrap the JSON in markdown code blocks
- Use null for fields where elements are not visible
- Ensure all strings are properly escaped
- The "description" field must always be present and non-empty"#
        .to_string()
}

/// Extract and parse DescriptionContent from LLM response
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

    if let Ok(content) = serde_json::from_str::<DescriptionContent>(cleaned) {
        if !content.description.trim().is_empty() {
            return Ok(content);
        }
    }

    // Step 3: Try to find JSON object in the text
    if let Some(start) = llm_response.find('{') {
        if let Some(end) = llm_response.rfind('}') {
            let json_str = &llm_response[start..=end];
            if let Ok(content) = serde_json::from_str::<DescriptionContent>(json_str) {
                if !content.description.trim().is_empty() {
                    return Ok(content);
                }
            }
        }
    }

    Err(format!("Failed to parse DescriptionContent from response: {}", llm_response).into())
}

/// Validate DescriptionContent structure
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
}
/*
``` rust
/// Example: Complete flow with LLM call
pub async fn describe_image_with_llm(
    node_id: &Uuid,
    image_data: &[u8], // or image path
    llm_client: &YourLLMClient, // Replace with your actual LLM client type
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    // Create the prompt
    let prompt = create_description_prompt(Some(
        "Focus on architectural elements and construction details."
    ));

    // Call LLM (pseudocode - adapt to your LLM client)
    let response = llm_client
        .generate_with_image(prompt, image_data)
        .await?;

    // Extract and validate the content
    let content = extract_description_content_robust(&response)?;

    // Serialize back to JSON string for storage
    let json_string = serde_json::to_string(&content)?;

    Ok(json_string)
}
```
Ключевые моменты:

1. **Промпт для LLM**:
- Чёткие инструкции возвращать ТОЛЬКО JSON
- Указание формата с примером
- Правила для обязательных и опциональных полей
- Запрет на markdown и дополнительный текст

2. **Extraction**:
- `extract_description_content()` - базовая версия с очисткой от markdown
- `extract_description_content_robust()` - более надёжная версия с несколькими попытками парсинга
- Поиск JSON объекта внутри текста если LLM добавил комментарии

3. **Validation**:
- Проверка что обязательное поле `description` не пустое
- Дополнительные проверки по необходимости

4. **Тесты** для проверки различных форматов ответов от LLM
*/