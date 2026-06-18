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
        normalize_classification_result(&mut result, context);

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

fn normalize_classification_result(result: &mut ClassificationResult, context: &UserContext) {
    if let Some(object_identifier) = result.extracted_parameters.object_identifier.as_deref() {
        let normalized = object_identifier.trim();
        if normalized.is_empty()
            || normalized.eq_ignore_ascii_case("null")
            || normalized.eq_ignore_ascii_case("not_set")
            || normalized.eq_ignore_ascii_case("none")
        {
            result.extracted_parameters.object_identifier = None;
        }
    }

    result.missing_context.retain(|field| match field {
        ContextField::ObjectId => context.object_id.is_none(),
        ContextField::CurrentReportId => context.current_report_id.is_none(),
        ContextField::PreviousReportId => context.previous_report_id.is_none(),
    });
}
