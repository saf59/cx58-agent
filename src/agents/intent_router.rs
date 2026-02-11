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

use rig::completion::Prompt; // Ollama uses OpenAI-compatible API
use anyhow::Result;
use tera::Context;
use std::sync::Arc;
use rig::client::CompletionClient;
use rig::providers::ollama;
use crate::helper::client;
use super::types::*;
use crate::localization::LocalizationManager;
use crate::templating::TemplateManager;

// TODO: According to plan.md Section 1 "Intent Classification Strategy", the router should
// implement multi-level routing with scope checking first, then intent classification.
// Consider adding a separate scope validation method before classification.

// TODO: According to plan.md "Classification Criteria", the router should recognize:
// - Scope Keywords: "object", "building", "construction", "site", "report", "photo", "project"
// - Action Keywords for different intents:
//   * Tree: "show", "list", "hierarchy", "structure", "objects"
//   * Reports: "reports", "photos", "images", "dates"
//   * Description: "describe", "what", "show me", "analyze"
//   * Comparison: "compare", "difference", "changes", "vs", "between"
//   * RAG: "why", "how", "explain", "what is", "purpose"
// Current implementation relies on LLM classification. Consider adding keyword-based
// pre-filtering or confidence scoring.

pub struct IntentRouter {
    client: ollama::Client,
    model: String,
    lang_manager: Arc<LocalizationManager>,
    template_manager: Arc<TemplateManager>,
    
    // TODO: According to plan.md Section 2 "Context Management Strategy", the router should
    // maintain conversation state. Consider adding:
    // - conversation_cache: Arc<ConversationCache> for multi-turn conversations
    // - context_validator: ContextValidator for validating required/optional context
}

// src/agents/intent_router.rs

impl IntentRouter {
    /// Creates a new IntentRouter instance.
    ///
    /// # Arguments
    /// * `_api_base` - API base URL (currently unused, marked with underscore)
    /// * `model` - The Ollama model name to use for classification
    /// * `lang_manager` - Shared localization manager for multi-language support
    /// * `template_manager` - Shared template manager for prompt generation
    ///
    /// # Architecture Notes
    /// According to plan.md, the Intent Router should validate context and route
    /// to appropriate workers. This constructor should potentially initialize
    /// additional components for context validation and conversation memory.
    pub fn new(
        _api_base: String,
        model: String,
        lang_manager: Arc<LocalizationManager>,
        template_manager: Arc<TemplateManager>,
    ) -> Self {
        let is_local = std::env::var("OLLAMA_LOCAL").unwrap_or( "false".to_string()) == "true";
        let client = client(is_local);

        // TODO: According to plan.md "Context Management Strategy", initialize:
        // - Context validator for required fields (user_id, chat_id, language)
        // - Optional context tracker (object_id, report_ids)
        // - Conversation memory store for multi-turn dialogues

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
    ///
    /// # Architecture Compliance
    /// This method implements the classification phase from plan.md Phase 1: Input Processing
    ///
    /// # Current Gaps
    /// - Does not implement explicit scope checking (in-scope vs out-of-scope)
    /// - Does not handle ambiguity with clarification requests
    /// - Does not validate optional context availability before classification
    pub async fn classify(
        &self,
        message: &str,
        context: &UserContext,
        conversation_history: &[String],
    ) -> Result<ClassificationResult> {
        // TODO: According to plan.md Phase 1 "Input Processing", implement:
        // Step 1: Validate Context
        //   - Check required fields: user_id, chat_id, language
        //   - If missing, return error requesting these fields
        //   - Flag missing optional context for potential clarification
        // This should happen BEFORE calling the LLM.

        // TODO: According to plan.md "Intent Classification Strategy", implement:
        // Step 1: Scope Check
        //   - Determine if query is in-scope (construction/monitoring related)
        //   - If out-of-scope, route to Rejection Handler
        //   - Only proceed with intent classification if in-scope
        // Current implementation goes straight to LLM classification.

        let lang = context.language.to_code();

        // Get system prompt from prompts directory (not FTL)
        let system_prompt = self.lang_manager
            .get_prompt(lang, "intent-router-system-prompt")?;

        // Build user prompt using Tera template
        let user_prompt = self.build_classification_prompt(
            message,
            context,
            conversation_history,
            lang,
        )?;
        
        println!("User prompt:\n{}", user_prompt);
        
        // TODO: According to plan.md Section 5 "Voice Command Handling", add:
        // - Noise filtering for voice input
        // - Language detection and validation against context.language
        // - Normalization of date/time expressions and ambiguous numbers
        // This preprocessing should occur before LLM classification.

        let agent = self.client
            .agent(&self.model)
            .preamble(&system_prompt)
            .temperature(0.1)  // Low temperature for consistent classification
            .max_tokens(2048)
            .build();

        // TODO: According to plan.md "Advanced Recommendations - Hybrid Routing",
        // some queries require multiple workers. Consider adding logic to detect
        // multi-step scenarios and flag them in the classification result.
        // Example: "Compare the last two reports for Building A" requires:
        // 1. Object identification (from "Building A")
        // 2. Report listing (last 2)
        // 3. Vision analysis (both reports)
        // 4. Comparison worker

        let response = agent.prompt(&user_prompt).await?;
        
        // Parse JSON response
        let cleaned = self.clean_json_response(&response);

        //tracing::info!("Cleaned JSON:\n{}", cleaned);

        // TODO: Add validation of ClassificationResult against expected schema
        // according to plan.md routing categories:
        // - Object Tree Request → ObjectTreeWorker
        // - Photo Report List → ReportListWorker
        // - Photo Description → VisionAnalysisWorker
        // - Photo Comparison → ComparisonWorker
        // - RAG Query → KnowledgeBaseWorker
        // - Out of Scope → RejectionHandler

        let result: ClassificationResult = serde_json::from_str(&cleaned)
            .map_err(|e| {
                // Use FTL for error messages (they're short)
                let mut ctx = Context::new();
                ctx.insert("error", &e.to_string());
                let error_msg = self.template_manager
                    .render(lang, "error-classification", ctx)
                    .unwrap_or_else(|_| self.lang_manager.get_msg(lang, "error-classification-fallback"));
                anyhow::anyhow!("{}\nResponse was: {}", error_msg, response)
            })?;

        // TODO: According to plan.md "Conversation Memory", update memory after classification:
        // - Store last query intent for follow-up questions
        // - Cache selected object_id for "show me more" queries
        // - Cache selected report_ids for "compare with previous" queries
        // - Store user preferences (preferred language, default period)

        // TODO: According to plan.md "Context Management Strategy", if the classification
        // determines that optional context is needed but missing:
        // - Set a flag in the result indicating context request needed
        // - Return information about what context to request from user
        // - The Orchestrator should handle the actual request to the user

        Ok(result)
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
    /// routing decisions. According to plan.md, it should provide enough context
    /// for the LLM to determine both intent and whether clarification is needed.
    fn build_classification_prompt(
        &self,
        message: &str,
        context: &UserContext,
        history: &[String],
        lang: &str,
    ) -> Result<String> {
        let mut ctx = Context::new();

        // TODO: According to plan.md "TaskParameters Interpretation", the prompt should
        // provide examples of how to interpret natural language into TaskParameters.
        // Examples should include:
        // - "Show me objects changed last week" → {last: true, period: Some(Week)}
        // - "Show all objects" → {last: false, all: true}
        // - "Show last 5 objects with changes this month" → {last: true, amount: Some(5), period: Some(Month)}

        ctx.insert("user_id", &context.user_id);
        ctx.insert("chat_id", &context.chat_id);
        ctx.insert("language", context.language.as_str());
        ctx.insert("object_id", &self.format_optional(&context.object_id, lang));
        ctx.insert("current_report_id", &self.format_optional(&context.current_report_id, lang));
        ctx.insert("previous_report_id", &self.format_optional(&context.previous_report_id, lang));

        // TODO: According to plan.md "Conversation Memory", the prompt should include:
        // - Last selected object_id from conversation memory (if different from context)
        // - Last selected report_ids from memory
        // - Last query intent for context ("user previously asked about...")
        // - User preferences for better personalization

        let history_text = if history.is_empty() {
            self.lang_manager.get_msg(lang, "no-conversation-history")
        } else {
            history.join("\n")
        };
        ctx.insert("conversation_history", &history_text);
        ctx.insert("user_message", message);

        // TODO: According to plan.md "Multi-Language Support", the prompt should:
        // - Include language-specific examples for better classification quality
        // - Provide technical term translations for construction domain
        // - Guide the LLM on handling mixed-language input

        // Use Tera template
        self.template_manager.render(lang, "intent-router-user-prompt", ctx)
    }

    /// Formats an optional value with localized status message.
    ///
    /// # Arguments
    /// * `opt` - Optional string value to format
    /// * `lang` - Language code for localization
    ///
    /// # Returns
    /// Formatted string showing the value or empty string if None
    ///
    /// # Note
    /// Returns empty string for None instead of "not set" message.
    /// This is intentional to keep prompts concise.
    fn format_optional(&self, opt: &Option<String>, lang: &str) -> String {
        match opt {
            Some(val) => {
                // Use FTL for simple messages
                let mut ctx = Context::new();
                ctx.insert("value", val);
                self.template_manager
                    .render(lang, "status-set", ctx)
                    .unwrap_or_else(|_| val.to_string())
            }
            //None => self.lang_manager.get_msg(lang, "status-not-set"),
            None => "".to_string()
        }
    }
    
    /// Cleans LLM response by removing markdown code blocks and trimming whitespace.
    ///
    /// # Arguments
    /// * `response` - Raw response string from the LLM
    ///
    /// # Returns
    /// Cleaned JSON string ready for parsing
    ///
    /// # Note
    /// Handles common LLM output patterns like ```json ... ``` wrapping.
    fn clean_json_response(&self, response: &str) -> String {
        // TODO: According to plan.md "Error Handling and Fallbacks", add more robust
        // JSON extraction logic:
        // - Handle multiple JSON objects in response
        // - Extract JSON even if surrounded by explanatory text
        // - Validate JSON structure before returning
        // - Log malformed responses for prompt improvement
        
        response.trim()
            // Remove markdown code blocks at start
            .trim_start_matches("```json")
            .trim_start_matches("```")
            // Remove markdown code blocks at end
            .trim_end_matches("```")
            // Remove extra whitespace
            .trim()
            .to_string()
    }
}

// ============================================================================
// ARCHITECTURAL DISCREPANCIES SUMMARY
// ============================================================================
//
// CRITICAL MISSING COMPONENTS (from plan.md):
//
// 1. CONTEXT VALIDATION (Phase 1: Input Processing)
//    - No explicit validation of required context before LLM call
//    - Missing: user_id, chat_id, language validation
//    - Missing: Error handling for missing required context
//    - Should implement: Context validator that runs before classify()
//
// 2. SCOPE CHECKING (Section 1: Intent Classification Strategy)
//    - No in-scope vs out-of-scope determination before classification
//    - Missing: Keyword-based scope detection
//    - Missing: Routing to Rejection Handler for out-of-scope queries
//    - Current: Relies entirely on LLM for scope determination
//
// 3. CONVERSATION MEMORY (Advanced Recommendations - Section B)
//    - No conversation state storage
//    - Missing: Last selected object_id tracking
//    - Missing: Last selected report_ids tracking
//    - Missing: Last query intent storage for follow-ups
//    - Missing: User preferences caching
//
// 4. AMBIGUITY HANDLING (Component Definition)
//    - Router should "handle ambiguity by requesting clarification"
//    - Missing: Logic to detect low-confidence classifications
//    - Missing: Mechanism to request user clarification
//    - Missing: Integration with Orchestrator for multi-turn clarification
//
// 5. MULTI-STEP SCENARIO DETECTION (Advanced Recommendations - Section A)
//    - No detection of queries requiring multiple workers
//    - Example: "Compare last two reports for Building A" needs:
//      * Object identification → Report listing → Vision analysis → Comparison
//    - Missing: Flag in ClassificationResult for multi-step workflows
//
// 6. VOICE COMMAND HANDLING (Section 4: Voice Command Handling)
//    - No STT noise filtering
//    - No language detection/validation against context
//    - No normalization of ambiguous voice input ("to" vs "two")
//    - No date/time expression normalization ("last week" variants)
//
// 7. OPTIONAL CONTEXT AWARENESS (Section 2: Context Management Strategy)
//    - classify() doesn't determine if optional context is needed
//    - Missing: Logic to request object_id when needed for query
//    - Missing: Logic to request report_ids when needed
//    - Should: Return context requirements in ClassificationResult
//
// RECOMMENDED ENHANCEMENTS:
//
// 8. KEYWORD-BASED PRE-FILTERING
//    - Add fast keyword matching before LLM call for common patterns
//    - Benefits: Reduced latency, lower LLM costs, more consistent routing
//    - Scope keywords: "object", "building", "construction", "site", etc.
//    - Action keywords per intent type (see plan.md Section 1)
//
// 9. CONFIDENCE SCORING
//    - Request confidence scores from LLM in ClassificationResult
//    - Implement threshold-based clarification requests
//    - High confidence → Execute directly
//    - Medium confidence → Request clarification
//    - Low confidence → Suggest related topics
//
// 10. LANGUAGE-SPECIFIC PROMPT OPTIMIZATION (Multi-Language Support)
//     - Separate prompt templates per language for better quality
//     - Construction terminology translation guides
//     - Language-specific classification examples
//
// 11. CACHING (Performance Optimization)
//     - Cache common query patterns and their classifications
//     - TTL-based invalidation for dynamic content
//     - Per-user preference caching
//
// 12. METRICS AND LOGGING
//     - Classification success/failure rates
//     - Intent distribution analytics
//     - Average classification latency
//     - Clarification request frequency
//
// ============================================================================
// IMPLEMENTATION PRIORITY:
// ============================================================================
// Priority 1 (Core Functionality):
// - Context validation (#1)
// - Scope checking (#2)
// - Optional context awareness (#7)
//
// Priority 2 (Enhanced Reliability):
// - Conversation memory (#3)
// - Ambiguity handling (#4)
// - Multi-step detection (#5)
//
// Priority 3 (User Experience):
// - Voice command handling (#6)
// - Keyword pre-filtering (#8)
// - Confidence scoring (#9)
//
// Priority 4 (Optimization):
// - Language-specific prompts (#10)
// - Caching (#11)
// - Metrics and logging (#12)
// ============================================================================
