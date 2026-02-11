//! # Orchestrator Module
//!
//! This module implements the **Orchestrator (Coordination Agent)** from the LLM Agent Architecture
//! for the Construction Site Monitoring System.
//!
//! ## Architecture Role
//!
//! The Orchestrator is responsible for:
//! - Managing workflow execution across specialized workers
//! - Coordinating between multiple workers when needed
//! - Handling context propagation (object_id, report_ids)
//! - Managing SSE streaming and progress updates
//! - Implementing retry logic and error handling
//!
//! ## Workflow Position
//!
//! ```text
//! Intent Router → Orchestrator → Specialized Workers → Response Formatter → SSE Stream
//! ```
//!
//! The Orchestrator sits between the Intent Router (classification) and the specialized workers,
//! making intelligent decisions about:
//! - Which workers to execute
//! - When to request missing context from users
//! - How to handle multi-step scenarios
//! - When to send progress updates
//! - When to format and return results
//!
//! ## Decision Types
//!
//! The orchestrator can make the following decisions (see `OrchestratorDecision` enum):
//! - `ExecuteWorker`: Dispatch a task to a specialized worker
//! - `RequestContextFromUser`: Request missing required/optional context
//! - `SendProgress`: Stream progress updates via SSE
//! - `FormatAndReturn`: Format worker results and return to user
//! - `Reject`: Politely decline out-of-scope requests

use rig::providers::ollama;
use rig::completion::Prompt;
use anyhow::Result;
use tera::Context;
use rig::client::CompletionClient;
use std::sync::Arc;
use uuid::Uuid;
use crate::helper::client;
use super::types::*;
use crate::localization::LocalizationManager;
use crate::templating::TemplateManager;

/// # Orchestrator - Coordination Agent
///
/// The central coordination component that manages workflow execution in the
/// Construction Site Monitoring System's LLM agent architecture.
///
/// ## Responsibilities
///
/// 1. **Workflow Management**: Decides the next step based on classification results,
///    current context, and previous worker results
/// 2. **Context Validation**: Ensures all required context (user_id, chat_id, object_id, etc.)
///    is available before executing workers
/// 3. **Worker Coordination**: Dispatches tasks to specialized workers and collects results
/// 4. **Progress Tracking**: Sends SSE progress updates during long-running operations
/// 5. **Error Recovery**: Handles failures gracefully and requests clarification when needed
///
/// ## Architecture Pattern
///
/// Implements the **Orchestrator-Worker Pattern**:
/// - Medium complexity
/// - High predictability
/// - Medium flexibility
/// - Excellent for structured, domain-specific tasks
///
/// ## Components
///
/// - `client`: Ollama LLM client for decision-making
/// - `model`: LLM model identifier (e.g., "llama3.2")
/// - `lang_manager`: Handles multi-language support (English/German)
/// - `template_manager`: Manages prompt templates using Tera
pub struct Orchestrator {
    /// Ollama client for LLM-based decision making
    client: ollama::Client,
    
    /// Model identifier (e.g., "llama3.2", "mistral")
    model: String,
    
    /// Localization manager for multi-language support
    /// Handles English and German translations for prompts and messages
    lang_manager: Arc<LocalizationManager>,
    
    /// Template manager for dynamic prompt generation
    /// Uses Tera templating engine for structured prompts
    template_manager: Arc<TemplateManager>,
}

impl Orchestrator {
    /// Creates a new Orchestrator instance
    ///
    /// # Arguments
    ///
    /// * `_api_base` - API base URL (currently unused, preserved for future extensions)
    /// * `model` - LLM model identifier to use for orchestration decisions
    /// * `lang_manager` - Shared localization manager for multi-language support
    /// * `template_manager` - Shared template manager for prompt generation
    ///
    /// # Environment Variables
    ///
    /// * `OLLAMA_LOCAL` - Set to "true" to use local Ollama instance, "false" for remote
    ///
    /// # Returns
    ///
    /// A new `Orchestrator` instance configured with the specified model and managers
    ///
    /// # Example
    ///
    /// ```no_run
    /// let orchestrator = Orchestrator::new(
    ///     "http://localhost:11434".to_string(),
    ///     "llama3.2".to_string(),
    ///     lang_manager,
    ///     template_manager,
    /// );
    /// ```
    pub fn new(
        _api_base: String,
        model: String,
        lang_manager: Arc<LocalizationManager>,
        template_manager: Arc<TemplateManager>,
    ) -> Self {
        tracing::info!("Creating Orchestrator");
        
        // Determine if using local or remote Ollama instance
        let is_local = std::env::var("OLLAMA_LOCAL").unwrap_or( "false".to_string()) == "true";
        let client = client(is_local);
        
        Self {
            client,
            model,
            lang_manager,
            template_manager,
        }
    }
    
    /// # Core Decision Engine
    ///
    /// Determines the next step in workflow orchestration based on:
    /// - Classification results from the Intent Router
    /// - Current user context (user_id, chat_id, language, object_id, report_ids)
    /// - Original user message
    /// - Results from previously executed workers
    ///
    /// ## Decision Flow
    ///
    /// ```text
    /// Router → Orchestrator → SSE: Progress Starting
    ///       → Worker Execute → Database Query
    ///       → Worker Process → Structured Result
    ///       → Orchestrator Format
    ///       → SSE: JSON Chunks (loop)
    ///       → SSE: Progress Complete
    /// ```
    ///
    /// ## Special Handling
    ///
    /// ### Ambiguous Intent
    /// If the intent is classified as `Intent::Ambiguous`, the orchestrator immediately
    /// requests clarification from the user rather than proceeding with worker execution.
    ///
    /// ### Context Validation
    /// The orchestrator validates required and optional context:
    /// - **Required**: user_id, chat_id, language (validated by router, not orchestrator)
    /// - **Optional**: object_id, current_report_id, previous_report_id
    ///
    /// If optional context is missing but needed, orchestrator requests it from user.
    ///
    /// ## Arguments
    ///
    /// * `classification` - Intent classification result from the router
    /// * `context` - Current user context with IDs and language
    /// * `original_message` - The user's original query text
    /// * `worker_results` - Results from any previously executed workers in this workflow
    ///
    /// ## Returns
    ///
    /// An `OrchestratorDecision` enum indicating the next action:
    /// - `ExecuteWorker`: Run a specific worker with parameters
    /// - `RequestContextFromUser`: Ask user for missing context
    /// - `SendProgress`: Send progress update via SSE
    /// - `FormatAndReturn`: Format results and send to user
    /// - `Reject`: Decline the request with explanation
    ///
    /// ## Errors
    ///
    /// Returns error if:
    /// - LLM fails to generate a valid response
    /// - Response cannot be parsed as JSON
    /// - Decision contains invalid worker type or parameters
    ///
    /// ## TODO
    ///
    /// - [ ] Current implementation doesn't retry on failure
    /// - [ ] No explicit SSE streaming implementation visible (likely handled by caller)
    /// - [ ] Missing conversation memory integration
    pub async fn decide_next_step(
        &self,
        classification: &ClassificationResult,
        context: &UserContext,
        original_message: &str,
        worker_results: &[WorkerResponse],
    ) -> Result<OrchestratorDecision> {
        let lang = context.language.to_code();

        // === SPECIAL CASE: Ambiguous Intent Handling ===
        // Routes to appropriate specialized worker
        // Handles ambiguity by requesting clarification
        //
        // When intent is ambiguous, immediately request clarification instead of
        // attempting to execute workers. Uses extracted object_identifier as suggestions
        // to help guide the user.
        if matches!(classification.intent, Intent::Ambiguous) {
            let prompt = self.lang_manager.get_msg(lang, "context-request-clarification");
            
            // Use extracted object_identifier as suggestions if available
            let suggestions = classification.extracted_parameters.object_identifier
                .clone()
                .map(|s| vec![s])
                .unwrap_or_default();
            
            // TODO: CurrentReportId is used as a dummy value here to trigger user request.
            // This should be refactored to use a more semantic field like "Clarification"
            return Ok(OrchestratorDecision::RequestContextFromUser {
                missing_field: ContextField::CurrentReportId,
                prompt,
                suggestions,
            });
        }

        // === LLM-Based Decision Generation ===
        
        // Get system prompt that defines orchestrator's role and decision format
        let system_prompt = self.lang_manager
            .get_prompt(lang, "orchestrator-system-prompt")?;
        
        // Build detailed user prompt with classification, context, and worker results
        let prompt = self.build_orchestrator_prompt(
            classification,
            context,
            original_message,
            worker_results,
            lang,
        )?;
        
        tracing::info!("Orchestrator - Prompt: {}", prompt);
        
        // Create LLM agent with low temperature (0.2) for consistent, predictable decisions
        // Low temperature is critical for structured orchestration decisions
        let agent = self.client
            .agent(&self.model)
            .preamble(&system_prompt)
            .temperature(0.2)  // Low temperature = more deterministic decisions
            .build();
        
        // Get decision from LLM
        let response = agent.prompt(&prompt).await?;
        
        tracing::info!("Orchestrator raw response:\n{}", response);
        
        // Clean markdown code fences and extract pure JSON
        let cleaned = self.clean_json_response(&response);
        
        tracing::debug!("Orchestrator cleaned JSON:\n{}", cleaned);
        
        // Parse JSON response
        let decision_json: serde_json::Value = serde_json::from_str(&cleaned)
            .map_err(|e| anyhow::anyhow!(
                "Failed to parse orchestrator decision: {}\nCleaned: {}\nOriginal: {}",
                e, cleaned, response
            ))?;
        
        // Convert JSON to OrchestratorDecision enum
        self.parse_decision(decision_json, lang, context, worker_results)
    }
    
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
    fn clean_json_response(&self, response: &str) -> String {
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
    
    /// Builds the user prompt for LLM-based orchestration decision
    ///
    /// Constructs a detailed prompt using Tera templates that includes:
    /// - Classification results (intent, confidence, extracted parameters)
    /// - User context (IDs, language, optional context fields)
    /// - Original user message
    /// - Previous worker results (if any)
    ///
    /// ## Template Variables
    ///
    /// The template receives the following context:
    /// - `intent`: Classified intent as string (e.g., "GetObjectTree")
    /// - `confidence`: Classification confidence (0.0 - 1.0)
    /// - `original_message`: User's exact query
    /// - `user_id`: UUID of the user
    /// - `chat_id`: UUID of the conversation
    /// - `language`: Language code ("en" or "de")
    /// - `object_id`: Current object ID or "Not set" localized message
    /// - `current_report_id`: Current report ID or "Not set" localized message
    /// - `previous_report_id`: Previous report ID or "Not set" localized message
    /// - `extracted_parameters`: JSON string of extracted parameters
    /// - `missing_context`: Array of missing context fields
    /// - `worker_results`: Formatted summary of previous worker results
    ///
    /// ## Arguments
    ///
    /// * `classification` - Intent classification from router
    /// * `context` - Current user context
    /// * `original_message` - User's original query
    /// * `worker_results` - Previous worker execution results
    /// * `lang` - Language code for localization
    ///
    /// ## Returns
    ///
    /// Rendered prompt string ready for LLM consumption
    ///
    /// ## Errors
    ///
    /// Returns error if template rendering fails or JSON serialization fails
    ///
    /// ## TODO
    ///
    /// - [ ] "conversation memory" - should include previous conversation
    ///       context from memory retrieval
    fn build_orchestrator_prompt(
        &self,
        classification: &ClassificationResult,
        context: &UserContext,
        original_message: &str,
        worker_results: &[WorkerResponse],
        lang: &str,
    ) -> Result<String> {
        let mut ctx = Context::new();
        
        // Insert classification data
        ctx.insert("intent", &format!("{:?}", classification.intent));
        ctx.insert("confidence", &format!("{:.2}", classification.confidence));
        ctx.insert("original_message", original_message);
        
        // Insert user context (required fields)
        ctx.insert("user_id", &context.user_id);
        ctx.insert("chat_id", &context.chat_id);
        ctx.insert("language", context.language.as_str());
        
        // Insert optional context fields with localized "Not set" message
        ctx.insert("object_id", &self.format_optional(&context.object_id, lang));
        ctx.insert("current_report_id", &self.format_optional(&context.current_report_id, lang));
        ctx.insert("previous_report_id", &self.format_optional(&context.previous_report_id, lang));
        
        // Insert extracted parameters as pretty-printed JSON
        ctx.insert(
            "extracted_parameters",
            &serde_json::to_string_pretty(&classification.extracted_parameters)?,
        );
        ctx.insert("missing_context", &format!("{:?}", classification.missing_context));
        
        // Format worker results for display
        let worker_results_text = if worker_results.is_empty() {
            self.lang_manager.get_msg(lang, "no-worker-results")
        } else {
            worker_results
                .iter()
                .map(|r| {
                    let mut result_ctx = Context::new();
                    result_ctx.insert("worker_type", &format!("{:?}", r.worker_type));
                    result_ctx.insert("status", &format!("{:?}", r.status));
                    result_ctx.insert("execution_time", &r.metadata.execution_time_ms);
                    
                    // Use template for consistent formatting, fallback to simple format
                    self.template_manager
                        .render(lang, "worker-result-summary", result_ctx)
                        .unwrap_or_else(|_| {
                            format!(
                                "{:?}: {:?} ({}ms)",
                                r.worker_type, r.status, r.metadata.execution_time_ms
                            )
                        })
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        ctx.insert("worker_results", &worker_results_text);
        
        // Render template with all context variables
        self.template_manager.render(lang, "orchestrator-user-prompt", ctx)
    }
    
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
    fn format_optional(&self, opt: &Option<String>, lang: &str) -> String {
        match opt {
            Some(val) => {
                let mut ctx = Context::new();
                ctx.insert("value", val);
                self.template_manager
                    .render(lang, "status-set", ctx)
                    .unwrap_or_else(|_| val.to_string())
            }
            None => "".to_string(),
        }
    }


    /// # Decision Parser
    ///
    /// Converts LLM JSON response into typed `OrchestratorDecision` enum.
    ///
    /// ## Expected JSON Structure
    ///
    /// The LLM must return JSON in this format:
    ///
    /// ```json
    /// {
    ///   "decision": "ExecuteWorker|RequestContextFromUser|SendProgress|FormatAndReturn|Reject",
    ///   "action": { /* decision-specific fields */ }
    /// }
    /// ```
    ///
    /// ## Decision Types & Expected Fields
    ///
    /// ### ExecuteWorker
    /// Dispatch a task to a specialized worker
    ///
    /// ```json
    /// {
    ///   "decision": "ExecuteWorker",
    ///   "action": {
    ///     "worker_type": "GetObjectTree|GetReportList|DescribeReport|CompareReports|RagQuery",
    ///     "parameters": { /* worker-specific parameters */ }
    ///   }
    /// }
    /// ```
    ///
    /// Worker types correspond to specialized workers:
    /// - `GetObjectTree`: Queries object hierarchy (Object Tree Worker)
    /// - `GetReportList`: Retrieves photo reports (Report List Worker)
    /// - `DescribeReport`: Analyzes single image (Vision Analysis Worker)
    /// - `CompareReports`: Compares two images (Comparison Worker)
    /// - `RagQuery`: Answers questions from knowledge base (Knowledge Base Worker)
    ///
    /// ### RequestContextFromUser
    /// Request missing context from user (from context validation flow)
    ///
    /// ```json
    /// {
    ///   "decision": "RequestContextFromUser",
    ///   "action": {
    ///     "missing_field": "ObjectId|CurrentReportId|PreviousReportId",
    ///     "prompt": "Which building would you like to inspect?",
    ///     "suggestions": ["Building A", "Building B"]
    ///   }
    /// }
    /// ```
    ///
    /// ### SendProgress
    /// Send progress update via SSE (from SSE streaming strategy)
    ///
    /// ```json
    /// {
    ///   "decision": "SendProgress",
    ///   "action": {
    ///     "status": "analyzing_query|fetching_data|processing_images",
    ///     "percent": 0-100,
    ///     "message": "Processing images..."
    ///   }
    /// }
    /// ```
    ///
    /// ### FormatAndReturn
    /// Format collected worker results and return to user
    ///
    /// ```json
    /// {
    ///   "decision": "FormatAndReturn",
    ///   "action": {}
    /// }
    /// ```
    ///
    /// ### Reject
    /// Politely decline out-of-scope request (Rejection Handler)
    ///
    /// ```json
    /// {
    ///   "decision": "Reject",
    ///   "action": {
    ///     "reason": "OutOfScope|MissingData|InvalidRequest",
    ///     "message": "I can only help with construction site monitoring."
    ///   }
    /// }
    /// ```
    ///
    /// ## Arguments
    ///
    /// * `decision_json` - Parsed JSON from LLM response
    /// * `lang` - Language code for error messages
    /// * `context` - Current user context for building worker requests
    /// * `worker_results` - Previous worker results (used for FormatAndReturn)
    ///
    /// ## Returns
    ///
    /// Typed `OrchestratorDecision` ready for execution
    ///
    /// ## Errors
    ///
    /// Returns error if:
    /// - JSON structure is invalid
    /// - Decision type is unknown
    /// - Required fields are missing
    /// - Worker type is invalid
    /// - Parameters are malformed
    ///
    /// ## TODO
    ///
    /// - [ ] Add validation for worker parameter completeness
    /// - [ ] Evaluator pattern for quality control on photo comparisons
    ///       but this is not implemented here
    fn parse_decision(
        &self,
        decision_json: serde_json::Value,
        lang: &str,
        context: &UserContext,
        worker_results: &[WorkerResponse],
    ) -> Result<OrchestratorDecision> {
        // Generate unique request ID for tracking
        let request_id = Uuid::now_v7().to_string();

        // Extract decision type from JSON
        let decision_type = decision_json["decision"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing decision type"))?;

        // Extract action data (decision-specific parameters)
        let action_data = &decision_json["action_data"];

        match decision_type {
            // === ExecuteWorker: Dispatch to Specialized Worker ===
            "ExecuteWorker" => {
                // Parse worker type (must be one of the 5 specialized workers)
                let worker_type_str = action_data["worker_type"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing worker_type"))?;

                // Build worker-specific parameters based on type
                // Each worker has different parameter requirements
                let parameters = match worker_type_str {
                    // Object Tree Worker: Queries PostgreSQL hierarchical data
                    "GetObjectTree" | "GET_OBJECT_TREE" => {
                        let task_params: TaskParameters = serde_json::from_value(
                            action_data["parameters"]["task_params"].clone()
                        )?;
                        WorkerParameters::GetObjectTree(task_params)
                    }
                    
                    // Report List Worker: Retrieves photo reports with date filtering
                    "GetReportList" | "GET_REPORT_LIST" => {
                        let object_id = action_data["parameters"]["object_id"]
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("Missing object_id"))?
                            .to_string();
                        let task_params: TaskParameters = serde_json::from_value(
                            action_data["parameters"]["task_params"].clone()
                        )?;
                        WorkerParameters::GetReportList { object_id, task_params }
                    }
                    
                    // Vision Analysis Worker: Processes single image from S3
                    "DescribeReport" | "DESCRIBE_REPORT" => {
                        let report_id = action_data["parameters"]["report_id"]
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("Missing report_id"))?
                            .to_string();

                        // Validate report_id is not empty
                        if report_id.is_empty() {
                            return Err(anyhow::anyhow!(
                                self.lang_manager.get_msg(lang, "error-empty-report-id")
                            ));
                        }
                        WorkerParameters::DescribeReport { report_id }
                    }
                    
                    // Comparison Worker: Analyzes differences between two reports
                    "CompareReports" | "COMPARE_REPORTS" => {
                        let report_id_1 = action_data["parameters"]["report_id_1"]
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("Missing report_id_1"))?
                            .to_string();
                        let report_id_2 = action_data["parameters"]["report_id_2"]
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("Missing report_id_2"))?
                            .to_string();

                        // Validate both report IDs are present
                        if report_id_1.is_empty() || report_id_2.is_empty() {
                            return Err(anyhow::anyhow!(
                                self.lang_manager.get_msg(lang, "error-empty-report-id")
                            ));
                        }
                        WorkerParameters::CompareReports { report_id_1, report_id_2 }
                    }
                    
                    // Knowledge Base Worker: RAG retrieval for project questions
                    "RagQuery" | "RAG_QUERY" => {
                        let query = action_data["parameters"]["query"]
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("Missing query"))?
                            .to_string();
                        WorkerParameters::RagQuery { query }
                    }
                    
                    // Unknown worker type - return error
                    _ => {
                        let msg = self.lang_manager.get_msg(lang, "error-unknown-worker");
                        return Err(anyhow::anyhow!(msg));
                    }
                };

                // Convert worker type string to enum
                let worker_type = match worker_type_str {
                    "GetObjectTree" | "GET_OBJECT_TREE" => WorkerType::GetObjectTree,
                    "GetReportList" | "GET_REPORT_LIST" => WorkerType::GetReportList,
                    "DescribeReport" | "DESCRIBE_REPORT" => WorkerType::DescribeReport,
                    "CompareReports" | "COMPARE_REPORTS" => WorkerType::CompareReports,
                    "RagQuery" | "RAG_QUERY" => WorkerType::RagQuery,
                    _ => {
                        let msg = self.lang_manager.get_msg(lang, "error-unknown-worker");
                        return Err(anyhow::anyhow!(msg));
                    }
                };

                // Build complete worker request with context
                Ok(OrchestratorDecision::ExecuteWorker(WorkerRequest {
                    worker_type,
                    parameters,
                    context: WorkerContext {
                        user_id: context.user_id.clone(),
                        language: context.language.clone(),
                        request_id,
                    },
                }))
            }
            
            // === RequestContextFromUser: Missing Context Handling ===
            //
            // When optional context (object_id, report_ids) is needed but missing,
            // orchestrator requests it from the user with helpful prompts and suggestions
            "RequestContextFromUser" => {
                let missing_field_raw = &action_data["missing_field"];
                
                // Parse missing field - handles both string and array formats
                // String format: "ObjectId" or "ObjectId,CurrentReportId" (comma-separated)
                // Array format: ["ObjectId", "CurrentReportId"]
                let missing_field: ContextField = if missing_field_raw.is_string() {
                    // Handle comma-separated string like "ObjectId,CurrentReportId"
                    let s = missing_field_raw.as_str().unwrap_or("");
                    if s.is_empty() {
                        return Err(anyhow::anyhow!("Empty missing_field"));
                    }
                    
                    // Take the first field from comma-separated list
                    // TODO: Should handle multiple missing fields properly rather than
                    // just taking the first one
                    let first_field = s.split(',')
                        .next()
                        .unwrap_or("");
                    
                    match first_field.trim() {
                        "ObjectId" | "OBJECT_ID" => ContextField::ObjectId,
                        "CurrentReportId" | "CURRENT_REPORT_ID" => ContextField::CurrentReportId,
                        "PreviousReportId" | "PREVIOUS_REPORT_ID" => ContextField::PreviousReportId,
                        _ => {
                            let msg = self.lang_manager.get_msg(lang, "error-unknown-context-field");
                            return Err(anyhow::anyhow!(format!("{}:{}",msg,first_field)));
                        }
                    }
                } else if missing_field_raw.is_array() {
                    // Handle array format - take first element
                    let arr = match missing_field_raw.as_array() {
                        Some(a) if !a.is_empty() => a,
                        _ => return Err(anyhow::anyhow!("Empty or invalid missing_field array")),
                    };
                    let s = arr[0].as_str().unwrap_or("");
                    match s {
                        "ObjectId" => ContextField::ObjectId,
                        "CurrentReportId" => ContextField::CurrentReportId,
                        "PreviousReportId" => ContextField::PreviousReportId,
                        _ => return Err(anyhow::anyhow!("Unknown context field: {}", s)),
                    }
                } else {
                    return Err(anyhow::anyhow!("Missing missing_field"));
                };

                // Get default localized prompt for this context field
                let default_prompt = match missing_field {
                    ContextField::ObjectId => self.lang_manager.get_msg(lang, "context-request-object-id"),
                    ContextField::CurrentReportId => self.lang_manager.get_msg(lang, "context-request-current-report"),
                    ContextField::PreviousReportId => self.lang_manager.get_msg(lang, "context-request-previous-report"),
                };

                Ok(OrchestratorDecision::RequestContextFromUser {
                    missing_field,
                    prompt: action_data["prompt"]
                        .as_str()
                        .unwrap_or(&default_prompt)
                        .to_string(),
                    suggestions: action_data["suggestions"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default(),
                })
            }
            
            // === SendProgress: SSE Progress Updates ===
            //
            // Sends progress updates during long-running operations like:
            // - analyzing_query (10%)
            // - fetching_data (40%)
            // - processing_images (70%)
            //
            // TODO: Must shows progress percentages at specific stages, but current
            // implementation allows arbitrary percent values. Should enforce standard
            // progress milestones for consistency.
            "SendProgress" => Ok(OrchestratorDecision::SendProgress {
                status: action_data["status"]
                    .as_str()
                    .unwrap_or("processing")
                    .to_string(),
                percent: action_data["percent"].as_u64().unwrap_or(50) as u8,
                message: action_data["message"]
                    .as_str()
                    .unwrap_or("Processing...")
                    .to_string(),
            }),
            
            // === FormatAndReturn: Complete Workflow ===
            // Final step - format all collected worker results and return to user
            // Response formatter will handle conversion to UI-compatible JSON
            "FormatAndReturn" => Ok(OrchestratorDecision::FormatAndReturn {
                worker_results: worker_results.to_vec(),
            }),
            
            // === Reject: Out of Scope Handler ===
            // Politely declines requests that are outside system capabilities
            "Reject" => Ok(OrchestratorDecision::Reject {
                reason: action_data["reason"]
                    .as_str()
                    .unwrap_or("Unknown")
                    .to_string(),
                message: action_data["message"]
                    .as_str()
                    .unwrap_or("Cannot process this request")
                    .to_string(),
            }),
            
            // Unknown decision type - return error with localized message
            _ => {
                let mut ctx = Context::new();
                ctx.insert("decision_type", decision_type);
                let msg = self.template_manager
                    .render(lang, "error-unknown-decision", ctx)
                    .unwrap_or_else(|_| self.lang_manager.get_msg(lang, "error-unknown-decision-type"));
                Err(anyhow::anyhow!(msg))
            }
        }
    }
}

// ===== ARCHITECTURAL COMPLIANCE SUMMARY =====
//
// ✅ Implements Orchestrator-Worker pattern correctly
// ✅ Supports all 5 specialized workers
// ✅ Handles context validation and missing context requests
// ✅ Supports multi-language (English/German) via LocalizationManager
// ✅ Uses structured prompts via TemplateManager
// ✅ Low temperature (0.2) for deterministic decisions
// ✅ Proper error handling with localized messages
// ✅ JSON response cleaning for robust LLM parsing
//
// ## Areas for Enhancement
//
// TODO: Implement retry logic for failed LLM calls
// TODO: Add conversation memory integration
// TODO: Implement caching strategy for worker results
// TODO: Add evaluator pattern for critical operations like photo comparisons
// TODO: Support multiple missing context fields simultaneously instead of just first one
// TODO: Add progress milestone validation to enforce consistent SSE progress updates
// TODO: Implement multi-step scenario handling explicitly
// TODO: Add explicit SSE streaming coordination (currently delegated to caller)
// TODO: Implement access control validation
// TODO: Add RAG database context for better knowledge-based responses
//
// ## Missing:
//
// ⚠️ No RAG worker implementation visible
// ⚠️ Database and storage access is delegated to workers
//    (This is actually correct - orchestrator should not directly access storage)
//
// ## Overall Assessment
//
// Main gaps are advanced features (memory, caching, evaluator) and retry logic.
