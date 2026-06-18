use crate::agents::LocalizationManager;
use rig::OneOrMany;
use rig::completion::AssistantContent;
use std::sync::Arc;

/// Formats optional context values for display in prompts
///
/// Converts `Option<String>` to a prompt-safe string, showing either the value
/// or the stable sentinel `not_set`.
///
/// ## Arguments
///
/// * `opt` - Optional value to format
/// * `lang` - Language code, kept for call-site compatibility
///
/// ## Returns
///
/// Either the contained value or `not_set`
///
/// ## Example
///
/// ```text
/// Some("building-123") → "building-123"
/// None                 → "not_set"
/// ```
pub fn format_optional(
    _lang_manager: &Arc<LocalizationManager>,
    value: &Option<String>,
    _lang: &str,
) -> String {
    match value {
        Some(v) => v.clone(),
        None => "not_set".to_string(),
    }
}

//noinspection ALL
/// Cleans LLM response to extract pure JSON
///
/// LLMs often wrap JSON inMarkdown code fences or add explanatory text.
/// This function strips all formatting to get the raw JSON object.
///
/// ## Cleaning Steps
///
/// 1. Remove leading/trailing whitespace
/// 2. StripMarkdown code fences (```json or ```)
/// 3. Find first `{` character (start of JSON object)
/// 4. Find last `}` character (end of JSON object)
/// 5. Extract only the JSON portion
///
/// ## Arguments
///
/// * `response` - Raw LLM response potentially containingMarkdown or extra text
///
/// ## Returns
///
/// Clean JSON string ready for parsing
///
/// ## Examples
///
/// ```text
/// Input:  "```json\n{\"decision\": \"ExecuteWorker\"}\n```"
/// Output: "{\"decision\": \"ExecuteWorker\"}"
///
/// Input:  "Here's the decision: {\"decision\": \"Reject\"} - hope this helps!"
/// Output: "{\"decision\": \"Reject\"}"
/// ```
pub fn clean_json_response(response: &str) -> String {
    let mut cleaned = response.trim().to_string();

    // Remove openingMarkdown code fence
    if cleaned.starts_with("```json") {
        cleaned = cleaned
            .trim_start_matches("```json")
            .trim_start()
            .to_string();
    } else if cleaned.starts_with("```") {
        cleaned = cleaned.trim_start_matches("```").trim_start().to_string();
    }

    // Remove closingMarkdown code fence
    if cleaned.ends_with("```") {
        cleaned = cleaned.trim_end_matches("```").trim_end().to_string();
    }

    // Find first opening brace (start of JSON)
    if let Some(start_pos) = cleaned.find('{') {
        cleaned = cleaned[start_pos..].to_string();
    }

    // Find last closing brace (end of JSON)
    if let Some(end_pos) = cleaned.rfind('}') {
        cleaned = cleaned[..=end_pos].to_string();
    }

    cleaned.trim().to_string()
}

pub fn extract_text_from_choice(choice: OneOrMany<AssistantContent>) -> String {
    choice
        .iter()
        .filter_map(|c| match c {
            AssistantContent::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}
