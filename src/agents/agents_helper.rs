use std::sync::Arc;
use rig::completion::AssistantContent;
use rig::OneOrMany;
use crate::templating::TemplateManager;

/// Formats optional context values for display in prompts
///
/// Converts `Option<String>` to a display string, showing either the value
/// or a localized "Not set" message.
///
/// ## Arguments
///
/// * `opt` - Optional value to format
/// * `lang` - Language code for localization of "Not set" message
///
/// ## Returns
///
/// Either the contained value or localized "Not set" message
///
/// ## Example
///
/// ```text
/// Some("building-123") → "building-123"
/// None                 → "Not set" (or "Nicht gesetzt" in German)
/// ```
pub fn format_optional(_template_manager:Arc<TemplateManager>, opt: &Option<String>, _lang: &str) -> String {
    match opt {
        Some(val) => val.to_string(),  // just result
        None => String::new(),
    }
}

//noinspection ALL
/// Cleans LLM response to extract pure JSON
///
/// LLMs often wrap JSON in markdown code fences or add explanatory text.
/// This function strips all formatting to get the raw JSON object.
///
/// ## Cleaning Steps
///
/// 1. Remove leading/trailing whitespace
/// 2. Strip markdown code fences (```json or ```)
/// 3. Find first `{` character (start of JSON object)
/// 4. Find last `}` character (end of JSON object)
/// 5. Extract only the JSON portion
///
/// ## Arguments
///
/// * `response` - Raw LLM response potentially containing markdown or extra text
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

    // Remove opening markdown code fence
    if cleaned.starts_with("```json") {
        cleaned = cleaned.trim_start_matches("```json").trim_start().to_string();
    } else if cleaned.starts_with("```") {
        cleaned = cleaned.trim_start_matches("```").trim_start().to_string();
    }

    // Remove closing markdown code fence
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