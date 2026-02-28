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
//! - How to handle multistep scenarios
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

use super::types::*;
use crate::agents::agent_error::AgentError;
use crate::agents::agents_helper::{clean_json_response, extract_text_from_choice, format_optional};
use crate::localization::LocalizationManager;
use crate::templating::TemplateManager;
use rig::client::CompletionClient;
use rig::completion::CompletionModel;
use rig::providers::ollama;
use serde_json::Value;
use std::sync::Arc;
use tera::Context;
use uuid::Uuid;

//noinspection ALL
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
    client: Arc<ollama::Client>,

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
    /// * `model` - LLM model identifier to use for orchestration decisions
    /// * `lang_manager` - Shared localization manager for multi-language support
    /// * `template_manager` - Shared template manager for prompt generation
    ///
    /// # Returns
    ///
    /// A new `Orchestrator` instance configured with the specified model and managers
    ///
    /// # Example
    ///
    /// ```no_run
    /// let orchestrator = Orchestrator::new(
    ///     "llama3.2".to_string(),
    ///     lang_manager,
    ///     template_manager,
    /// );
    /// ```
    pub fn new(
        client: Arc<ollama::Client>,
        model: String,
        lang_manager: Arc<LocalizationManager>,
        template_manager: Arc<TemplateManager>,
    ) -> Self {
        tracing::info!("Creating Orchestrator");

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
    /// - [ ] Current implementation doesn't retry on failure

    pub async fn decide_next_step(
        &self,
        classification: &ClassificationResult,
        context: &UserContext,
        original_message: &str,
        worker_results: &[WorkerResponse],
    ) -> Result<(OrchestratorDecision, Option<u64>), AgentError> {
        let lang = context.language.to_code();

        // === SPECIAL CASE: Ambiguous Intent Handling ===
        // Routes to appropriate specialized worker
        // Handles ambiguity by requesting clarification
        //
        // When intent is ambiguous, immediately request clarification instead of
        // attempting to execute workers. Uses extracted object_identifier as suggestions
        // to help guide the user.
        if matches!(classification.intent, Intent::Ambiguous) {
            let prompt = self
                .lang_manager
                .get_msg(lang, "context-request-clarification");

            // Use extracted object_identifier as suggestions if available
            let suggestions = classification
                .extracted_parameters
                .object_identifier
                .clone()
                .map(|s| vec![s])
                .unwrap_or_default();

            return Ok((
                OrchestratorDecision::RequestContextFromUser {
                    missing_field: ContextField::CurrentReportId,
                    prompt,
                    suggestions,
                },
                Some(0u64),
            ));
        }

        // === LLM-Based Decision Generation ===

        // Get system prompt that defines orchestrator's role and decision format
        let system_prompt = self
            .lang_manager
            .get_prompt(lang, "orchestrator-system-prompt")?;

        // Build detailed user prompt with classification, context, and worker results
        let prompt = self.build_orchestrator_prompt(
            classification,
            context,
            original_message,
            worker_results,
            lang,
        )?;

        tracing::debug!("Orchestrator - Prompt: {}", prompt);
        let model = self.client.completion_model(&self.model);

        // Create LLM agent with low temperature (0.2) for consistent, predictable decisions
        // Low temperature is critical for structured orchestration decisions
        let request = model
            .completion_request(&prompt)
            .preamble(system_prompt)
            .temperature(0.2)
            .build();
        let response = model.completion(request).await?;

        let choice = response.choice; 
        let text = extract_text_from_choice(choice);

        if text.is_empty() {
            let msg = "Orchestrator LLM returned no text content";
            let err = AgentError::internal(msg);
            tracing::error!("Orchestrator LLM response error: {}", msg);
            return Err(err.into());
        }

        let tokens = Some(
            response.raw_response.prompt_eval_count.unwrap_or(0)
                + response.raw_response.eval_count.unwrap_or(0),
        );

        tracing::debug!("Orchestrator raw response:\n{}", text);

        // CleanMarkdown code fences and extract pure JSON
        let cleaned = clean_json_response(&text);

        tracing::debug!("Orchestrator cleaned JSON:\n{}", cleaned);

        // Parse JSON response
        let decision_json: Value = serde_json::from_str(&cleaned).map_err(|e| {
            let err  =AgentError::internal(format!(
                "Failed to parse orchestrator decision: {}\nCleaned: {}\nOriginal: {}",
                e, cleaned, text
            ));
            tracing::error!("Orchestrator parse JSON response: {}", err);
            err
        })?;

        let decision = self.parse_decision(decision_json, lang, context, worker_results)?;

        Ok((decision, tokens))
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

    fn build_orchestrator_prompt(
        &self,
        classification: &ClassificationResult,
        context: &UserContext,
        original_message: &str,
        worker_results: &[WorkerResponse],
        lang: &str,
    ) -> Result<String, AgentError> {
        let mut ctx = Context::new();
        ctx.insert("intent", &format!("{:?}", classification.intent));
        ctx.insert("confidence", &format!("{:.2}", classification.confidence));
        ctx.insert("original_message", original_message);

        // Insert user context (required fields)
        ctx.insert("user_id", &context.user_id);
        ctx.insert("chat_id", &context.chat_id);
        ctx.insert("language", context.language.as_str());

        // Insert optional context fields with localized "Not set" message
        ctx.insert(
            "object_id",
            &format_optional(&self.lang_manager.clone(), &context.object_id, lang),
        );
        ctx.insert(
            "current_report_id",
            &format_optional(
                &self.lang_manager.clone(),
                &context.current_report_id,
                lang,
            ),
        );
        ctx.insert(
            "previous_report_id",
            &format_optional(
                &self.lang_manager.clone(),
                &context.previous_report_id,
                lang,
            ),
        );

        ctx.insert(
            "extracted_parameters",
            &serde_json::to_string_pretty(&classification.extracted_parameters).unwrap_or_else(|_| "{}".to_string()),
        );
        ctx.insert(
            "missing_context",
            &serde_json::to_string(&classification.missing_context).unwrap_or_default(),
        );

        let results: Vec<Value> = worker_results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "worker_type": format!("{:?}", r.worker_type),
                    "status": format!("{:?}", r.status),
                })
            })
            .collect();

        ctx.insert("worker_results", &results);
        // Render template with all context variables
        self.template_manager
            .render(lang, "orchestrator-user-prompt", ctx)
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
    
    fn parse_decision(
        &self,
        decision_json: Value,
        lang: &str,
        context: &UserContext,
        worker_results: &[WorkerResponse],
    ) -> Result<OrchestratorDecision, AgentError> {
        // Generate unique request ID for tracking
        let request_id = Uuid::now_v7().to_string();

        // Extract decision type from JSON
        let decision_type = decision_json["decision"]
            .as_str()
            .ok_or_else(|| {
                let err = AgentError::internal("Missing decision type");
                tracing::error!("Orchestrator parse decision_json: {}", err);
                err
            })?;

        // Extract action data (decision-specific parameters)
        let action_data = decision_json.get("action")
            .or_else(|| decision_json.get("action_data"))
            .unwrap_or(&Value::Null);

        match decision_type {
            // === ExecuteWorker: Dispatch to Specialized Worker ===
            "ExecuteWorker" => {
                // Parse worker type (must be one of the 5 specialized workers)
                let worker_type_str = action_data["worker_type"]
                    .as_str()
                    .ok_or_else(|| {
                        let err = AgentError::internal("Missing worker_type");
                        tracing::error!("Orchestrator parse action_data: {}", err);
                        err
                    })?;

                // Build worker-specific parameters based on type
                // Each worker has different parameter requirements
                let parameters = match worker_type_str {
                    // Object Tree Worker: Queries PostgreSQL hierarchical data
                    "GetObjectTree" | "GET_OBJECT_TREE" => {
                        let task_params: TaskParameters = serde_json::from_value(
                            action_data["parameters"]["task_params"].clone(),
                        ).map_err(|e| {
                            let err = AgentError::internal("Missing or invalid TaskParameters for GetObjectTree");
                            tracing::error!("Orchestrator parse task_params: {} {}", err, e);
                            err
                        })?;
                        WorkerParameters::GetObjectTree(task_params)
                    }

                    // Report List Worker: Retrieves photo reports with date filtering
                    "GetReportList" | "GET_REPORT_LIST" => {
                        let task_params: TaskParameters = serde_json::from_value(
                            action_data["parameters"]["task_params"].clone(),
                        ).map_err(|e| {
                                let err = AgentError::internal("Missing or invalid TaskParameters for GetReportList");
                                tracing::error!("Orchestrator parse task_params: {} {}", err, e);
                                err
                            })?;
                        WorkerParameters::GetReportList {
                            task_params,
                        }
                    }

                    // Vision Analysis Worker: Processes single image from S3
                    "DescribeReport" | "DESCRIBE_REPORT" => {
                        let current_report_id = action_data["parameters"]["reports"]["prev"]
                            .as_str()
                            .ok_or_else(|| {
                                let err = AgentError::internal("Missing prev report_id");
                                tracing::error!("Orchestrator parse parameters: {}", err);
                                err
                            })?
                            .to_string();

                        // Validate report_id is not empty
                        if current_report_id.is_empty() {
                            let err = AgentError::internal(
                                self.lang_manager.get_msg(lang, "error-empty-report-id"),
                            );
                            tracing::error!("Orchestrator parse current_report_id: {}", err);
                            return Err(err);
                        }

                        // Previous report is optional (for comparing with previous)
                        let previous_report_id = action_data["parameters"]["reports"]["next"]
                            .as_str()
                            .map(|s| s.to_string());

                        let reports = ReportPair {
                            prev: current_report_id,
                            next: previous_report_id,
                        };

                        WorkerParameters::DescribeReport { reports }
                    }

                    // Comparison Worker: Analyzes differences between two reports
                    "CompareReports" | "COMPARE_REPORTS" => {
                        WorkerParameters::CompareReports {
                            reports: Value::Null,
                        }
                    }

                    // Knowledge Base Worker: RAG retrieval for project questions
                    "RagQuery" | "RAG_QUERY" => {
                        let query = action_data["parameters"]["query"]
                            .as_str()
                            .ok_or_else(|| {
                                let err = AgentError::internal("Missing query for RagQuery worker");
                                tracing::error!("Orchestrator parse parameters: {}", err);
                                err
                            })?
                            .to_string();
                        WorkerParameters::RagQuery { query }
                    }

                    // Unknown worker type - return error
                    _ => {
                        let msg = self.lang_manager.get_msg(lang, "error-unknown-worker");
                        let err = AgentError::internal(msg);
                        tracing::error!("Orchestrator parse worker_type: {}", err);
                        return Err(err);
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
                        let err = AgentError::internal(msg);
                        tracing::error!("Orchestrator parse worker_type: {}", err);
                        return Err(err.into());
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
                        let err = AgentError::internal("Empty missing_field string");
                        tracing::error!("Orchestrator parse missing_field: {}", err);
                        return Err(err);
                    }

                    let first_field = s.split(',').next().unwrap_or("");

                    match first_field.trim() {
                        "ObjectId" | "OBJECT_ID" => ContextField::ObjectId,
                        "CurrentReportId" | "CURRENT_REPORT_ID" => ContextField::CurrentReportId,
                        "PreviousReportId" | "PREVIOUS_REPORT_ID" => ContextField::PreviousReportId,
                        _ => {
                            let msg = self
                                .lang_manager
                                .get_msg(lang, "error-unknown-context-field");
                            let err = AgentError::internal(format!("{}:{}", msg, first_field));
                            tracing::error!("Orchestrator parse first_field: {}", err);
                            return Err(err);
                        }
                    }
                } else if missing_field_raw.is_array() {
                    // Handle array format - take first element
                    let arr = match missing_field_raw.as_array() {
                        Some(a) if !a.is_empty() => a,
                        _ => {
                            let err = AgentError::internal("Empty or invalid missing_field array");
                            tracing::error!("Orchestrator parse missing_field array: {}", err);
                            return Err(err);
                        }
                    };
                    let s = arr[0].as_str().unwrap_or("");
                    match s {
                        "ObjectId" => ContextField::ObjectId,
                        "CurrentReportId" => ContextField::CurrentReportId,
                        "PreviousReportId" => ContextField::PreviousReportId,
                        _ => {
                            let err = AgentError::internal(format!("Unknown context field in array: {}", s));
                            tracing::error!("Orchestrator parse missing_field array element: {}", err);
                            return Err(err);
                        }
                    }
                } else {
                    let err = AgentError::internal("Missing or invalid missing_field format");
                    tracing::error!("Orchestrator parse missing_field: {}", err);
                    return Err(err);
                };

                // Get default localized prompt for this context field
                let default_prompt = match missing_field {
                    ContextField::ObjectId => {
                        self.lang_manager.get_msg(lang, "context-request-object-id")
                    }
                    ContextField::CurrentReportId => self
                        .lang_manager
                        .get_msg(lang, "context-request-current-report"),
                    ContextField::PreviousReportId => self
                        .lang_manager
                        .get_msg(lang, "context-request-previous-report"),
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
            "SendProgress" => Ok(OrchestratorDecision::SendProgress {
                status: action_data["status"]
                    .as_str()
                    .unwrap_or(&self.lang_manager.get_msg(lang, "processing"))
                    .to_string(),
                percent: action_data["percent"].as_u64().unwrap_or(50) as u8,
                message: action_data["message"]
                    .as_str()
                    .unwrap_or(&self.lang_manager.get_msg(lang, "processing"))
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
                    .unwrap_or(&self.lang_manager.get_msg(lang, "unknown"))
                    .to_string(),
                message: action_data["message"]
                    .as_str()
                    .unwrap_or(&self.lang_manager.get_msg(lang, "orchestrator-cannot-process"))
                    .to_string(),
            }),

            // Unknown decision type - return error with localized message
            _ => {
                let mut ctx = Context::new();
                ctx.insert("decision_type", decision_type);
                let msg = self.lang_manager
                    .get_msg_with_arg(lang, "error-unknown-decision", "decision_type", decision_type);
                let err = AgentError::internal(msg);
                tracing::error!("Orchestrator parse decision_type: {}", err);
                Err(err.into())
            }
        }
    }
}
