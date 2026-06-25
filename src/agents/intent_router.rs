// src/agents/intent_router.rs
//
// ARCHITECTURAL ROLE:
// This is the Intent Router (Classification Agent) component as defined in plan.md.
// Responsible for:
// - Analyzing user queries to determine intent category
// - Validating required context (user_id, chat_id, language)
// - Routing to appropriate specialized worker
// - Handling ambiguity by requesting clarification
//
// POSITION IN ARCHITECTURE:
// Input Flow: Text Input → Intent Router → Route Classification → Specialized Workers
// This component sits at the entry point of the agent system.

use super::types::*;
use crate::agents::agent_error::AgentError;
use crate::agents::agents_helper::{
    clean_json_response, extract_text_from_choice, format_optional,
};
use crate::localization::LocalizationManager;
use crate::templating::TemplateManager;
use chrono::NaiveDate;
// Ollama uses OpenAI-compatible API
use rig::client::CompletionClient;
use rig::completion::CompletionModel;
use rig::providers::ollama;
use std::sync::Arc;
use tera::Context;
// "Intent Classification Strategy", the router should
// implement multi-level routing with scope checking first, then intent classification.
// Consider adding a separate scope validation method before classification.

// "Classification Criteria", the router should recognize:
// - Scope Keywords: "object", "building", "construction", "site", "report", "photo", "project"
// - Action Keywords for different intents:
//   * Tree: "show", "list", "hierarchy", "structure", "objects"
//   * Reports: "reports", "photos", "images", "dates"
//   * Description: "describe", "what", "show me", "analyze"
//   * Comparison: "compare", "difference", "changes", "vs", "between"
//   * Chat: "why", "how", "explain", "what is", "purpose"
// Current implementation relies on LLM classification. Consider adding keyword-based
// pre-filtering or confidence scoring.

pub struct IntentRouter {
    client: Arc<ollama::Client>,
    model: String,
    lang_manager: Arc<LocalizationManager>,
    template_manager: Arc<TemplateManager>,
}

// src/agents/intent_router.rs

impl IntentRouter {
    /// Creates a new IntentRouter instance.
    ///
    /// # Arguments
    /// * `model` - The Ollama model name to use for classification
    /// * `lang_manager` - Shared localization manager for multi-language support
    /// * `template_manager` - Shared template manager for prompt generation
    ///
    /// # Architecture Notes
    /// , the Intent Router should validate context and route
    /// to appropriate workers. This constructor should potentially initialize
    /// additional components for context validation and conversation memory.
    pub fn new(
        client: Arc<ollama::Client>,
        model: String,
        lang_manager: Arc<LocalizationManager>,
        template_manager: Arc<TemplateManager>,
    ) -> Self {
        Self {
            client,
            model,
            lang_manager,
            template_manager,
        }
    }

    /// Classifies user intent from a message and conversation context.
    ///
    /// # Arguments
    /// * `message` - The user's input message to classify
    /// * `context` - User context containing user_id, chat_id, language, and optional fields
    /// * `conversation_history` - Previous messages in the conversation
    ///
    /// # Returns
    /// A `ClassificationResult` containing the determined intent and extracted parameters

    pub async fn classify(
        &self,
        message: &str,
        context: &UserContext,
        conversation_history: &[String],
    ) -> Result<(ClassificationResult, Option<u64>), AgentError> {
        self.classify_with_model(message, context, conversation_history, &self.model)
            .await
    }

    pub async fn classify_with_model(
        &self,
        message: &str,
        context: &UserContext,
        conversation_history: &[String],
        model_name: &str,
    ) -> Result<(ClassificationResult, Option<u64>), AgentError> {
        // Step 1: Scope Check
        //   - Determine if query is in-scope (construction/monitoring related)
        //   - If out-of-scope, route to Rejection Handler
        //   - Only proceed with intent classification if in-scope

        let lang = context.language.to_code();

        // Get system prompt from prompts directory (not FTL)
        let system_prompt = self
            .lang_manager
            .get_prompt(lang, "intent-router-system-prompt")?;

        // Build user prompt using Tera template
        let user_prompt =
            self.build_classification_prompt(message, context, conversation_history, lang)?;

        let model = self.client.completion_model(model_name);
        let request = model
            .completion_request(&user_prompt)
            .preamble(system_prompt)
            .temperature(0.1)
            .build();

        // some queries require multiple workers. Consider adding logic to detect
        // multi-step scenarios and flag them in the classification result.
        // Example: "Compare the last two reports for Building A" requires:
        // 1. Object identification (from "Building A")
        // 2. Report listing (last 2)
        // 3. Vision analysis (both reports)
        // 4. Comparison worker

        //let response = agent.completion(&user_prompt).await?;
        let response = model.completion(request).await?;

        let choice = response.choice.clone();
        let text = extract_text_from_choice(choice);

        if text.is_empty() {
            let msg = "IntentRouter: LLM returned empty response";
            tracing::error!("{}", msg);
            let err = AgentError::internal(msg);
            return Err(err);
        }

        let tokens = Some(
            response.raw_response.prompt_eval_count.unwrap_or(0)
                + response.raw_response.eval_count.unwrap_or(0),
        );

        // Parse JSON response
        let cleaned = clean_json_response(&text);

        let mut result: ClassificationResult = serde_json::from_str(&cleaned).map_err(|e| {
            // Use FTL for error messages (they're short)
            let mut ctx = Context::new();
            ctx.insert("error", &e.to_string());
            let error_msg = self.lang_manager.get_msg_with_arg(
                lang,
                "error-classification",
                "error",
                &e.to_string(),
            );
            tracing::error!(
                "IntentRouter: failed to parse ClassificationResult: {}\nRaw response: {}",
                e,
                text
            );
            AgentError::internal(format!("{}\nResponse was: {}", error_msg, text))
        })?;
        normalize_classification_result(&mut result, context, message);

        // determines that optional context is needed but missing:
        // - Set a flag in the result indicating context request needed
        // - Return information about what context to request from user
        // - The Orchestrator should handle the actual request to the user

        Ok((result, tokens))
    }

    /// Builds the classification prompt using Tera templates.
    ///
    /// # Arguments
    /// * `message` - The user's input message
    /// * `context` - User context with IDs and language
    /// * `history` - Conversation history for context-aware classification
    /// * `lang` - Language code for localization
    ///
    /// # Returns
    /// A formatted prompt string ready for LLM classification
    ///
    /// # Architecture Notes
    /// This method assembles context information for the LLM to make informed
    /// routing decisions. , it should provide enough context
    /// for the LLM to determine both intent and whether clarification is needed.
    fn build_classification_prompt(
        &self,
        message: &str,
        context: &UserContext,
        history: &[String],
        lang: &str,
    ) -> Result<String, AgentError> {
        let mut ctx = Context::new();

        ctx.insert("user_id", &context.user_id);
        ctx.insert("chat_id", &context.chat_id);
        ctx.insert("language", context.language.as_str());
        ctx.insert(
            "object_id",
            &format_optional(&self.lang_manager.clone(), &context.object_id, lang),
        );
        ctx.insert(
            "current_report_id",
            &format_optional(&self.lang_manager.clone(), &context.current_report_id, lang),
        );
        ctx.insert(
            "previous_report_id",
            &format_optional(
                &self.lang_manager.clone(),
                &context.previous_report_id,
                lang,
            ),
        );

        let history_text = if history.is_empty() {
            self.lang_manager.get_msg(lang, "no-conversation-history")
        } else {
            history.join("\n")
        };
        ctx.insert("conversation_history", &history_text);
        ctx.insert("user_message", message);

        // Use Tera template
        self.template_manager
            .render(lang, "intent-router-user-prompt", ctx)
    }
}

fn normalize_classification_result(
    result: &mut ClassificationResult,
    context: &UserContext,
    message: &str,
) {
    if result.extracted_parameters.object_identifier.is_none()
        && let Some(object_identifier) = extract_quoted_object_name(message)
    {
        result.extracted_parameters.object_identifier = Some(object_identifier);
    }

    if let Some(object_identifier) = result.extracted_parameters.object_identifier.as_deref() {
        let normalized = object_identifier.trim();
        if normalized.is_empty()
            || normalized.eq_ignore_ascii_case("null")
            || normalized.eq_ignore_ascii_case("not_set")
            || normalized.eq_ignore_ascii_case("none")
        {
            result.extracted_parameters.object_identifier = None;
        } else if uuid::Uuid::parse_str(normalized).is_ok() && !message.contains(normalized) {
            tracing::warn!(
                object_identifier = normalized,
                object_id = ?context.object_id,
                current_report_id = ?context.current_report_id,
                previous_report_id = ?context.previous_report_id,
                "IntentRouter: dropping UUID copied from context as object_identifier"
            );
            result.extracted_parameters.object_identifier = None;
        }
    }

    if is_selected_pair_comparison_request(message, context) {
        result.intent = Intent::CompareReports;
        result.extracted_parameters.task_params = None;
    } else if is_endpoint_comparison_request(message) {
        result.intent = Intent::CompareReports;
        let params = ensure_task_params(&mut result.extracted_parameters);
        params.last = false;
        params.all = true;
        params.period = None;
        params.amount = None;
        params.exact_datetime = None;
    } else if let Some(exact_datetime) = extract_exact_datetime(message) {
        result.intent = if is_explicit_report_description_request(message) {
            Intent::DescribeReport
        } else {
            Intent::GetReportList
        };
        let params = ensure_task_params(&mut result.extracted_parameters);
        params.last = false;
        params.all = true;
        params.period = None;
        params.amount = None;
        params.exact_datetime = Some(exact_datetime);
    }

    let can_resolve_reports = result.extracted_parameters.task_params.is_some();
    result.missing_context.retain(|field| match field {
        ContextField::ObjectId => {
            needs_object_id(&result.intent)
                && context.object_id.is_none()
                && result.extracted_parameters.object_identifier.is_none()
        }
        ContextField::CurrentReportId => {
            needs_current_report_id(&result.intent)
                && context.current_report_id.is_none()
                && !can_resolve_reports
        }
        ContextField::PreviousReportId => {
            matches!(result.intent, Intent::CompareReports)
                && context.previous_report_id.is_none()
                && !can_resolve_reports
        }
    });
}

fn needs_object_id(intent: &Intent) -> bool {
    matches!(
        intent,
        Intent::GetReportList | Intent::DescribeReport | Intent::CompareReports
    )
}

fn needs_current_report_id(intent: &Intent) -> bool {
    matches!(intent, Intent::DescribeReport | Intent::CompareReports)
}

fn ensure_task_params(extracted: &mut ExtractedParameters) -> &mut TaskParameters {
    extracted.task_params.get_or_insert_with(|| TaskParameters {
        last: false,
        all: false,
        period: None,
        amount: None,
        exact_datetime: None,
    })
}

fn extract_quoted_object_name(message: &str) -> Option<String> {
    let quoted = quoted_segments(message);
    quoted
        .into_iter()
        .find(|segment| {
            let lower = segment.to_lowercase();
            !lower.contains("report")
                && !lower.contains("bericht")
                && segment.chars().any(|ch| ch == '-' || ch == '–')
        })
        .map(|segment| segment.trim().to_string())
}

fn quoted_segments(message: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;

    for ch in message.chars() {
        match (quote, ch) {
            (None, '"' | '\'' | '“' | '„') => {
                quote = Some(ch);
                current.clear();
            }
            (Some(start), '"' | '\'' | '”' | '“' | '„') if quote_closes(start, ch) => {
                let segment = current.trim();
                if !segment.is_empty() {
                    segments.push(segment.to_string());
                }
                current.clear();
                quote = None;
            }
            (Some(_), _) => current.push(ch),
            (None, _) => {}
        }
    }

    segments
}

fn quote_closes(start: char, ch: char) -> bool {
    matches!(
        (start, ch),
        ('"', '"') | ('\'', '\'') | ('“', '”') | ('„', '“') | ('„', '”')
    )
}

fn is_endpoint_comparison_request(message: &str) -> bool {
    let lower = message.to_lowercase();
    let asks_for_change =
        lower.contains("change") || lower.contains("changes") || lower.contains("änderung");
    let has_report = lower.contains("report") || lower.contains("bericht");
    let has_endpoint_pair = (lower.contains("oldest") && lower.contains("newest"))
        || (lower.contains("first") && lower.contains("last"))
        || (lower.contains("ersten") && lower.contains("letzten"))
        || (lower.contains("ältesten") && lower.contains("neuesten"));

    has_report && asks_for_change && has_endpoint_pair
}

fn is_selected_pair_comparison_request(message: &str, context: &UserContext) -> bool {
    let lower = message.to_lowercase();
    let asks_for_comparison = lower.contains("change")
        || lower.contains("changes")
        || lower.contains("compare")
        || lower.contains("comparison")
        || lower.contains("difference")
        || lower.contains("diff")
        || lower.contains("änderung")
        || lower.contains("änderungen")
        || lower.contains("unterschied");

    asks_for_comparison
        && context.current_report_id.is_some()
        && context.previous_report_id.is_some()
}

fn is_explicit_report_description_request(message: &str) -> bool {
    let lower = message.to_lowercase();
    let has_report = lower.contains("report") || lower.contains("bericht");
    let asks_for_description = lower.contains("describe")
        || lower.contains("description")
        || lower.contains("beschreib")
        || lower.contains("analyze")
        || lower.contains("analyse")
        || lower.contains("analysiere");

    has_report && asks_for_description
}

fn extract_exact_datetime(message: &str) -> Option<chrono::NaiveDateTime> {
    let words = tokenize_datetime_words(message);

    for (idx, word) in words.iter().enumerate() {
        let Some(month) = month_number(word) else {
            continue;
        };
        let day = idx
            .checked_sub(1)
            .and_then(|day_idx| leading_number(&words[day_idx]))?;
        let year = words
            .iter()
            .skip(idx + 1)
            .find_map(|word| parse_year(word))?;
        let (hour, minute) = parse_time_after_month(&words[idx + 1..]).unwrap_or((0, 0));

        return NaiveDate::from_ymd_opt(year, month, day)
            .and_then(|date| date.and_hms_opt(hour, minute, 0));
    }

    None
}

fn tokenize_datetime_words(message: &str) -> Vec<String> {
    message
        .split_whitespace()
        .map(|word| {
            word.trim_matches(|ch: char| {
                ch == ','
                    || ch == '"'
                    || ch == '\''
                    || ch == '“'
                    || ch == '”'
                    || ch == '„'
                    || ch == '?'
                    || ch == '!'
            })
            .trim_end_matches('.')
            .to_lowercase()
        })
        .filter(|word| !word.is_empty())
        .collect()
}

fn month_number(word: &str) -> Option<u32> {
    match word {
        "january" | "jan" | "januar" => Some(1),
        "february" | "feb" | "februar" => Some(2),
        "march" | "mar" | "märz" | "maerz" => Some(3),
        "april" | "apr" => Some(4),
        "may" | "mai" => Some(5),
        "june" | "jun" | "juni" => Some(6),
        "july" | "jul" | "juli" => Some(7),
        "august" | "aug" => Some(8),
        "september" | "sep" | "sept" => Some(9),
        "october" | "oct" | "oktober" | "okt" => Some(10),
        "november" | "nov" => Some(11),
        "december" | "dec" | "dezember" | "dez" => Some(12),
        _ => None,
    }
}

fn leading_number(word: &str) -> Option<u32> {
    let digits: String = word.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn parse_year(word: &str) -> Option<i32> {
    let year = leading_number(word)?;
    if (1900..=2200).contains(&year) {
        Some(year as i32)
    } else {
        None
    }
}

fn parse_time_after_month(words: &[String]) -> Option<(u32, u32)> {
    let year_idx = words.iter().position(|word| parse_year(word).is_some())?;
    let tail = &words[year_idx + 1..];

    for (idx, word) in tail.iter().enumerate() {
        if let Some((hour, minute)) = parse_clock_word(word) {
            let period = tail.get(idx + 1).map(String::as_str);
            return Some(apply_period(hour, minute, period));
        }
    }

    None
}

fn parse_clock_word(word: &str) -> Option<(u32, u32)> {
    let clean = word.trim_end_matches("uhr");
    if let Some((hour, minute)) = clean.split_once(':') {
        return Some((hour.parse().ok()?, minute.parse().ok()?));
    }
    leading_number(clean).map(|hour| (hour, 0))
}

fn apply_period(hour: u32, minute: u32, period: Option<&str>) -> (u32, u32) {
    match period {
        Some("pm") if hour < 12 => (hour + 12, minute),
        Some("am") if hour == 12 => (0, minute),
        _ => (hour, minute),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context_without_selection() -> UserContext {
        UserContext {
            user_id: "user".to_string(),
            chat_id: "chat".to_string(),
            language: Language::English,
            object_id: None,
            current_report_id: None,
            previous_report_id: None,
        }
    }

    fn context_with_selected_pair() -> UserContext {
        UserContext {
            user_id: "user".to_string(),
            chat_id: "chat".to_string(),
            language: Language::English,
            object_id: Some("object-id".to_string()),
            current_report_id: Some("newer-report-id".to_string()),
            previous_report_id: Some("older-report-id".to_string()),
        }
    }

    fn empty_classification(intent: Intent) -> ClassificationResult {
        ClassificationResult {
            intent,
            confidence: 0.5,
            extracted_parameters: ExtractedParameters {
                task_params: None,
                object_identifier: None,
                time_reference: None,
                report_references: Vec::new(),
            },
            missing_context: vec![
                ContextField::ObjectId,
                ContextField::CurrentReportId,
                ContextField::PreviousReportId,
            ],
            reasoning: "test".to_string(),
        }
    }

    #[test]
    fn normalizes_oldest_newest_changes_to_compare_all_reports() {
        let mut result = empty_classification(Intent::DescribeReport);
        normalize_classification_result(
            &mut result,
            &context_without_selection(),
            "Please show me the changes from \"EG - Bad\" from the oldest report to the newest report. Thanks!",
        );

        assert!(matches!(result.intent, Intent::CompareReports));
        assert_eq!(
            result.extracted_parameters.object_identifier.as_deref(),
            Some("EG - Bad")
        );
        let params = result.extracted_parameters.task_params.unwrap();
        assert!(params.all);
        assert!(!params.last);
        assert!(params.exact_datetime.is_none());
        assert!(!result.missing_context.contains(&ContextField::ObjectId));
    }

    #[test]
    fn normalizes_german_first_last_changes_to_compare_all_reports() {
        let mut result = empty_classification(Intent::DescribeReport);
        normalize_classification_result(
            &mut result,
            &context_without_selection(),
            "Bitte zeige mir die Änderungen von \"EG - Bad\" vom letzten Bericht zum ersten Bericht. Danke!",
        );

        assert!(matches!(result.intent, Intent::CompareReports));
        assert_eq!(
            result.extracted_parameters.object_identifier.as_deref(),
            Some("EG - Bad")
        );
        assert!(result.extracted_parameters.task_params.unwrap().all);
    }

    #[test]
    fn normalizes_short_changes_with_selected_pair_to_compare_reports() {
        let mut result = empty_classification(Intent::Ambiguous);
        normalize_classification_result(&mut result, &context_with_selected_pair(), "Show changes");

        assert!(matches!(result.intent, Intent::CompareReports));
        assert!(result.extracted_parameters.task_params.is_none());
        assert!(result.missing_context.is_empty());
    }

    #[test]
    fn normalizes_exact_datetime_report_description() {
        let mut result = empty_classification(Intent::GetReportList);
        normalize_classification_result(
            &mut result,
            &context_without_selection(),
            "Please, show me the report \"EG - Bad\" from 22nd May, 2026, 8 pm. And please describe the report. Thanks",
        );

        assert!(matches!(result.intent, Intent::DescribeReport));
        assert_eq!(
            result.extracted_parameters.object_identifier.as_deref(),
            Some("EG - Bad")
        );
        let exact = result
            .extracted_parameters
            .task_params
            .unwrap()
            .exact_datetime
            .unwrap();
        assert_eq!(
            exact.format("%d.%m.%Y %H:%M:%S").to_string(),
            "22.05.2026 20:00:00"
        );
        assert!(!result.missing_context.contains(&ContextField::ObjectId));
        assert!(
            !result
                .missing_context
                .contains(&ContextField::CurrentReportId)
        );
    }

    #[test]
    fn normalizes_exact_datetime_report_show_to_report_list() {
        let mut result = empty_classification(Intent::DescribeReport);
        normalize_classification_result(
            &mut result,
            &context_without_selection(),
            "show me the report \"EG - Bad\" from 22nd May, 2026, 8 pm.",
        );

        assert!(matches!(result.intent, Intent::GetReportList));
        assert_eq!(
            result.extracted_parameters.object_identifier.as_deref(),
            Some("EG - Bad")
        );
        let exact = result
            .extracted_parameters
            .task_params
            .unwrap()
            .exact_datetime
            .unwrap();
        assert_eq!(
            exact.format("%d.%m.%Y %H:%M:%S").to_string(),
            "22.05.2026 20:00:00"
        );
        assert!(result.missing_context.is_empty());
    }
}
