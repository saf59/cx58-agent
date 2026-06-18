//! | `RagQuery` | Ask questions about project data using RAG |
//! | `OutOfScope` | Non-construction related queries |
//! | `Ambiguous` | Unclear queries requiring clarification |
//!
//! ## Request Processing Flow
//!
//! ```
//! 1. receive_agent_request()
//!    └─> Creates SSE channel for streaming
//!
//! 2. process_request()
//!    ├─> Initialize tracking (request_id, start_time)
//!    ├─> Parse language from request
//!    └─> Send initial progress: "analyzing"
//!
//! 3. classify_intent()
//!    ├─> Call IntentRouter::classify()
//!    ├─> Check for OutOfScope (early exit path)
//!    └─> Send progress
//!
//! 4. orchestrate_workflow()
//!    ├─> Call Orchestrator::decide_next_step()
//!    └─> Loop based on OrchestratorDecision:
//!        ├─> ExecuteWorker → execute_worker() → store in decision_results
//!        ├─> RequestContextFromUser → send prompt, return
//!        ├─> SendProgress → forward to SSE
//!        ├─> FormatAndReturn → use worker_results from decision → format_and_stream_response()
//!        └─> Reject → send error message
//!
//! 5. format_and_stream_response()
//!    ├─> Intent::DescribeReport → format_description()
//!    ├─> Intent::CompareReports → format_comparison()
//!    ├─> Intent::GetObjectTree → send ObjectTree chunk
//!    ├─> Intent::GetReportList → send ReportList chunk
//!    └─> Send Complete with total_time_ms
//! ```
//!
//! ## SSE Stream Events
//!
//! ```json
//! // Progress updates
//! {"chunk_type": "progress", "data": {"status": "analyzing", "percent": 10, "message": "Analyzing query..."}}
//!
//! // Data chunks serde:Value
//!
//! // Completion
//! {"chunk_type": "complete", "data": {"total_time_ms": 3500}}
//!
//! // Errors
//! {"chunk_type": "error", "data": {"message": "...", "code": "AGENT_ERROR"}}
//! ```
//!
//! ## Context Management
//!
//! The agent maintains context throughout the request lifecycle:
//! - **Required**: user_id, chat_id, language
//! - **Optional**: object_id, current_report_id, previous_report_id
//! - **Derived**: request_id (UUID v7), language enum
//!
//! Context flows through: AgentRequest → UserContext → WorkerContext
//!
//! ## Error Handling
//!
//! - Errors during processing send `StreamEvent::Error`, `StreamEvent::Completed` to SSE
//! - Tera template fallback used for error messages
//! - Request lifecycle terminates on first error
//!

use rig::providers::ollama;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;
use unicode_segmentation::UnicodeSegmentation;
use uuid::Uuid;

use super::{
    ChatAgent, ComparisonAgent, DescriptionAgent, DocumentAgent, ObjectAgent,
    intent_router::IntentRouter, orchestrator::Orchestrator, response_formatter::ResponseFormatter,
    types::*,
};

use crate::agents::agent_error::AgentError;
use crate::agents::documents_id_finder::DocumentsIdFinder;
use crate::agents::object_id_finder::ObjectIdFinder;
use crate::agents::stats::AgentStats;
use crate::localization::LocalizationManager;
use crate::model_settings::effective_ai_config;
use crate::templating::TemplateManager;
use crate::{
    AgentContext, AgentRequest, AiConfig, AppState, RequestManager, StreamEvent, append_history,
    load_session, save_session, save_session_with_history,
};

/// # MasterAgent
///
/// The main entry point for processing user queries in the construction site
/// monitoring system. Coordinates intent classification, workflow orchestration,
/// and response formatting to provide intelligent, multi-step responses via
/// Server-Sent Events (SSE) streaming.
///
/// ## Architecture
///
/// ```
/// ┌──────────────────────────────────────────────────────────────────────┐
/// │                            MasterAgent                               │
/// │  ┌────────────────┐  ┌──────────────┐  ┌──────────────────────────┐  │
/// │  │ IntentRouter   │→ │ Orchestrator │→ │ ResponseFormatter        │  │
/// │  │(Classification)│  │   (Workflow) │  │(Natural Language Output) │  │
/// │  └────────────────┘  └──────────────┘  └──────────────────────────┘  │
/// └──────────────────────────────────────────────────────────────────────┘
///                                    │
///                                    ▼
///                        ┌───────────────────────┐
///                        │     SSE Stream        │
///                        │  (Progress + Data)    │
///                        └───────────────────────┘
/// ```

const MAX_ORCHESTRATION_STEPS: u32 = 8;
pub struct MasterAgent {
    client: Arc<ollama::Client>,
    config: AiConfig,
    request_manager: Arc<RequestManager>,
    intent_router: Arc<IntentRouter>,
    orchestrator: Arc<Orchestrator>,
    formatter: Arc<ResponseFormatter>,
    lang_manager: Arc<LocalizationManager>,
    template_manager: Arc<TemplateManager>,
}

impl MasterAgent {
    pub fn new(client: Arc<ollama::Client>, config: AiConfig) -> Self {
        let lang_manager = Arc::new(LocalizationManager::new());
        let template_manager = Arc::new(TemplateManager::new());

        Self {
            client: client.clone(),
            config: config.clone(),
            request_manager: Arc::new(RequestManager::new()),
            intent_router: Arc::new(IntentRouter::new(
                client.clone(),
                config.chat_model.clone(),
                lang_manager.clone(),
                template_manager.clone(),
            )),
            orchestrator: Arc::new(Orchestrator::new(
                client.clone(),
                config.text_model.clone(),
                lang_manager.clone(),
                template_manager.clone(),
            )),
            formatter: Arc::new(ResponseFormatter::new(
                client.clone(),
                config.text_model.clone(),
                lang_manager.clone(),
                template_manager.clone(),
            )),
            lang_manager,
            template_manager,
        }
    }
    pub async fn cancel_request(&self, request_id: &str) -> bool {
        self.request_manager.cancel(request_id).await
    }

    /// Handles an agent request and returns an SSE stream of responses.
    pub async fn handle_request_stream(
        &self,
        state: Arc<AppState>,
        request: AgentRequest,
    ) -> mpsc::Receiver<StreamEvent> {
        tracing::debug!(
            "\nMessage: {:?}, object_id: {:?}, prev_leaf:{:?},next_leaf:{:?}",
            &request.message,
            &request.object_id,
            &request.prev_leaf,
            &request.next_leaf
        );
        let (tx, rx) = mpsc::channel(100);

        let lang_manager = self.lang_manager.clone();
        let state = state.clone();
        let agent = self.clone();
        let start_time = Instant::now();

        tokio::spawn(async move {
            let request_id = Uuid::now_v7().to_string();
            let mut request = request;
            let ai_config =
                effective_ai_config(&state.db, &request.user_id, &state.ai_config).await;
            let request_state = Arc::new(AppState {
                db: state.db.clone(),
                storage: state.storage.clone(),
                master_agent: state.master_agent.clone(),
                ai_config,
            });

            // Load session and fill missing fields from previous request.
            // history_strings is passed to IntentRouter so it can resolve
            // anaphoric references ("same as before", "that room again").
            let mut conversation_history: Vec<String> = Vec::new();
            if let Some(session) =
                load_session(&request_state.db, &request.user_id, &request.chat_id).await
            {
                // Extract history before partial moves of Option<String> fields.
                conversation_history = session.history_strings();
                if request.object_id.is_none() {
                    request.object_id = session.object_id;
                    if request.prev_leaf.is_none() {
                        request.prev_leaf = session.prev_leaf;
                    }
                    if request.next_leaf.is_none() {
                        request.next_leaf = session.next_leaf;
                    }
                }
            }

            let cancellation_token = agent.request_manager.register(request_id.clone()).await;
            let context = AgentContext::from_request(
                request_id.clone(),
                request.clone(),
                cancellation_token.clone(),
            );

            // Notify client that streaming has started.
            let _ = tx
                .send(StreamEvent::Started {
                    request_id: request_id.clone(),
                    timestamp: chrono::Utc::now().timestamp(),
                })
                .await;

            if let Err(e) = agent
                .process_request(
                    request_state,
                    context.clone(),
                    tx.clone(),
                    conversation_history,
                )
                .await
            {
                // Determine the user's language for the error message.
                let lang = Language::from_short(&context.language);
                let lang_code = lang.to_code();

                // Try to downcast to AgentError for a fully localized message;
                // fall back to Internal for generic errors.
                let agent_error = e.downcast::<AgentError>().unwrap_or_else(|e| {
                    Box::new(AgentError::Internal {
                        detail: e.to_string(),
                    })
                });

                let error_msg = agent_error.localized_message(lang_code, &lang_manager);

                let _ = tx
                    .send(StreamEvent::Error {
                        request_id: context.request_id.clone(),
                        error: error_msg,
                    })
                    .await;
                let _ = tx
                    .send(StreamEvent::Completed {
                        request_id: context.request_id.clone(),
                        total_time_ms: start_time.elapsed().as_millis() as u64,
                        stats: AgentStats::default(),
                    })
                    .await;
            }
        });

        rx
    }

    async fn process_request(
        &self,
        state: Arc<AppState>,
        context: AgentContext,
        tx: Sender<StreamEvent>,
        conversation_history: Vec<String>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let start_time = Instant::now();
        let mut stats = AgentStats::start();

        let user_context = context.to_user_context();
        let lang = user_context.language.clone();
        let lang_code = lang.to_code();

        let analyzing_msg = self.lang_manager.get_msg(lang_code, "progress-analyzing");
        tx.send(StreamEvent::Progress {
            request_id: context.request_id.clone(),
            status: "analyzing".to_string(),
            percent: 10,
            message: analyzing_msg,
        })
        .await?;
        let intent_start = Instant::now();
        let (classification, router_tokens) = self
            .intent_router
            .classify_with_model(
                &context.message,
                &user_context,
                &conversation_history,
                &state.ai_config.chat_model,
            )
            .await?;
        tracing::info!("Intent classification result: {:?}", &classification.intent);
        stats.record_router(
            router_tokens,
            Some(intent_start.elapsed().as_millis() as u64),
        );
        context.cancellation_token.check().await?;

        // Intent::Ambiguous handling path.
        // No explicit branch for Intent::Ambiguous here.
        // intentional: Orchestrator handles this via RequestContextFromUser

        if matches!(classification.intent, Intent::OutOfScope) {
            let message = self
                .formatter
                .format_out_of_scope_with_model(
                    &user_context.language,
                    &context.message,
                    &state.ai_config.text_model,
                )
                .await?;

            self.send_text_chunks(&tx, &message, &context.request_id)
                .await?;
            stats.finalize();
            tx.send(StreamEvent::Completed {
                request_id: context.request_id.clone(),
                total_time_ms: start_time.elapsed().as_millis() as u64,
                stats,
            })
            .await?;

            return Ok(());
        }

        // Ambiguous intent: the user's query is unclear — send a plain text
        // clarification request and exit. No orchestration needed.
        // Handled here (not in the Orchestrator) to avoid the loop entirely
        // and to use TextChunk instead of the semantically wrong ContextRequest.
        if matches!(classification.intent, Intent::Ambiguous) {
            let prompt = self
                .lang_manager
                .get_msg(lang_code, "context-request-clarification");
            self.send_text_chunks(&tx, &prompt, &context.request_id)
                .await?;
            stats.finalize();
            tx.send(StreamEvent::Completed {
                request_id: context.request_id.clone(),
                total_time_ms: start_time.elapsed().as_millis() as u64,
                stats,
            })
            .await?;
            return Ok(());
        }

        let validation_msg = self
            .lang_manager
            .get_msg(lang_code, "progress-context-validation");
        tx.send(StreamEvent::Progress {
            request_id: context.request_id.clone(),
            status: "context_validation".to_string(),
            percent: 30,
            message: validation_msg,
        })
        .await?;

        // Early context validation: check required fields before entering the
        // orchestration loop. Workers panic with internal errors when context is
        // missing; catching it here turns those crashes into a clean ContextRequest.
        // Fields that can be auto-resolved (object_identifier present in extracted
        // parameters, or task_params available for report resolution) are skipped —
        // the orchestrator loop will handle them via ObjectIdFinder / DocumentsIdFinder.
        if let Some((msg_key, hint_key, field)) = Self::missing_context_for_intent(
            &classification.intent,
            &user_context,
            &classification.extracted_parameters,
        ) {
            let prompt = self.lang_manager.get_msg_or(lang_code, msg_key, msg_key);
            let hint = self.lang_manager.get_msg(lang_code, hint_key);
            let suggestions = if hint.is_empty() || hint.starts_with("Missing message:") {
                vec![]
            } else {
                vec![hint]
            };
            tracing::info!(
                intent = ?classification.intent,
                ?field,
                "Context validation failed before orchestration loop"
            );
            tx.send(StreamEvent::ContextRequest {
                request_id: context.request_id.clone(),
                prompt,
                suggestions,
            })
            .await?;
            stats.finalize();
            tx.send(StreamEvent::Completed {
                request_id: context.request_id.clone(),
                total_time_ms: start_time.elapsed().as_millis() as u64,
                stats,
            })
            .await?;
            return Ok(());
        }

        let mut worker_results: Vec<WorkerResponse> = Vec::new();
        // Mutable so ObjectIdFinder / DocumentsIdFinder results can be written
        // back into context before the next orchestrator call.
        let mut current_context = user_context.clone();

        // If the user explicitly named an object in the message, discard any
        // object_id / report IDs that arrived from the session or the client.
        // Keeping stale IDs would cause ObjectIdFinder to be skipped and the
        // previous object's cached results to be returned instead of the new one.
        //
        // Examples of when this fires:
        //   - new chat, prev_leaf/next_leaf sent by frontend from old chat
        //   - user switches objects mid-conversation ("now show me Room 211")
        if classification
            .extracted_parameters
            .object_identifier
            .is_some()
        {
            if current_context.object_id.is_some() {
                tracing::info!(
                    object_identifier = ?classification.extracted_parameters.object_identifier,
                    old_object_id = ?current_context.object_id,
                    "object_identifier present — clearing stale object_id and report IDs from context"
                );
            }
            current_context.object_id = None;
            current_context.current_report_id = None;
            current_context.previous_report_id = None;
        } else if classification.extracted_parameters.task_params.is_some() {
            // No explicit object name, but time/count params given — report IDs
            // will be resolved by DocumentsIdFinder for the current object.
            current_context.current_report_id = None;
            current_context.previous_report_id = None;
        }

        let mut step_count = 0u32;

        loop {
            step_count += 1;
            if step_count > MAX_ORCHESTRATION_STEPS {
                tracing::error!(request_id = %context.request_id, "Orchestration loop exceeded max steps");
                return Err(AgentError::internal("Orchestration loop limit exceeded").into());
            }

            context.cancellation_token.check().await?;

            // Guard: CompareReports require DescribeReport
            if matches!(classification.intent, Intent::CompareReports)
                && current_context.current_report_id.is_some()
                && current_context.previous_report_id.is_some()
            {
                let has_describe = worker_results
                    .iter()
                    .any(|r| r.worker_type == WorkerType::DescribeReport);

                if !has_describe {
                    // Force DescribeReport execution without going through LLM orchestration.
                    // Safety: current_report_id is guaranteed present by
                    // missing_context_for_intent() which runs before this loop.
                    let Some(current_report_id) = current_context.current_report_id.clone() else {
                        // Orchestrator bypassed DocumentsIdFinder — emit ContextRequest safely.
                        let prompt = self.lang_manager.get_msg_or(
                            lang_code,
                            "context-request-select-report",
                            "context-request-select-report",
                        );
                        let hint = self
                            .lang_manager
                            .get_msg(lang_code, "context-request-select-report-hint");
                        let suggestions = if hint.is_empty() || hint.starts_with("Missing message:")
                        {
                            vec![]
                        } else {
                            vec![hint]
                        };
                        tracing::warn!(
                            "CompareReports guard: current_report_id missing after orchestration — \
                             emitting ContextRequest instead of panicking"
                        );
                        tx.send(StreamEvent::ContextRequest {
                            request_id: context.request_id.clone(),
                            prompt,
                            suggestions,
                        })
                        .await?;
                        stats.finalize();
                        tx.send(StreamEvent::Completed {
                            request_id: context.request_id.clone(),
                            total_time_ms: start_time.elapsed().as_millis() as u64,
                            stats,
                        })
                        .await?;
                        return Ok(());
                    };

                    let reports = ReportPair {
                        prev: current_report_id,
                        next: current_context.previous_report_id.clone(),
                    };
                    let worker_req = WorkerRequest {
                        worker_type: WorkerType::DescribeReport,
                        parameters: WorkerParameters::DescribeReport { reports },
                        context: WorkerContext {
                            user_id: current_context.user_id.clone(),
                            language: current_context.language.clone(),
                            request_id: context.request_id.clone(),
                        },
                    };

                    let mut worker_agent_context = context.clone();
                    worker_agent_context.object_id = current_context.object_id.clone();
                    worker_agent_context.prev_leaf = current_context.current_report_id.clone();
                    worker_agent_context.next_leaf = current_context.previous_report_id.clone();

                    let result = self
                        .execute_worker_via_agent(
                            state.clone(),
                            worker_agent_context,
                            worker_req,
                            tx.clone(),
                            &worker_results,
                        )
                        .await
                        .map_err(|e| {
                            tracing::error!("Worker execution failed: {}", e);
                            e
                        })?;

                    stats.record_worker(
                        &format!("{:?}", result.worker_type),
                        result.metadata.execution_time_ms,
                        result.metadata.llm_calls,
                        result.metadata.tokens_used,
                    );
                    worker_results.push(result);
                    // CompareReports guard always pushes DescribeReport results,
                    // no ID resolution happens here — but call for consistency.
                    Self::apply_resolution_to_context(
                        &mut current_context,
                        worker_results.last().unwrap(),
                    );
                    continue; // next iteration — DescribeReport result is now available
                }
            }
            let ready = Self::intent_ready(classification.intent.clone(), &worker_results);
            if !ready.is_empty() {
                tracing::info!(
                    "Intent ready for response formatting: {:?}",
                    &classification.intent
                );
                // Save session only on successful completion — not on every iteration.
                // Saving here guarantees the session reflects a fully resolved context.
                // Append current exchange to history so the next request can
                // resolve anaphoric references ("that room again", "compare those two").
                // We store the intent name as a compact assistant entry.
                let current_history = serde_json::json!(
                    conversation_history
                        .iter()
                        .enumerate()
                        .map(|(i, s)| {
                            let role = if i % 2 == 0 { "user" } else { "assistant" };
                            serde_json::json!({"role": role, "text": s})
                        })
                        .collect::<Vec<serde_json::Value>>()
                );
                let assistant_summary = format!("{:?}", classification.intent);
                let updated_history =
                    append_history(&current_history, &context.message, &assistant_summary);
                save_session_with_history(
                    &state.db,
                    &current_context.user_id,
                    &current_context.chat_id,
                    current_context.object_id.as_deref(),
                    current_context.current_report_id.as_deref(), // = prev_leaf
                    current_context.previous_report_id.as_deref(), // = next_leaf
                    Some(&updated_history),
                )
                .await;
                self.format_and_stream_response(&tx, &classification.intent, &ready, &context)
                    .await?;
                break;
            }
            let orch_start = Instant::now();
            let (decision, orch_tokens) = self
                .orchestrator
                .decide_next_step_with_model(
                    &classification,
                    &current_context,
                    &context.message,
                    &context.request_id.to_string(),
                    &worker_results,
                    &state.ai_config.text_model,
                )
                .await?;
            stats.record_orchestrator(orch_tokens, Some(orch_start.elapsed().as_millis() as u64));
            context.cancellation_token.check().await?;

            match decision {
                OrchestratorDecision::ExecuteWorker(mut worker_req) => {
                    worker_req.context.user_id = current_context.user_id.clone();
                    worker_req.context.language = current_context.language.clone();

                    tracing::info!(
                        "OrchestratorDecision::ExecuteWorker: {:?}",
                        &worker_req.worker_type
                    );

                    let progress_key = match worker_req.worker_type {
                        WorkerType::ObjectIdFinder => "progress-worker-finding-object",
                        WorkerType::DocumentsIdFinder => "progress-worker-finding-reports",
                        WorkerType::GetObjectTree => "progress-worker-loading-tree",
                        WorkerType::GetReportList => "progress-worker-loading-reports",
                        WorkerType::DescribeReport => "progress-worker-describing-report",
                        WorkerType::CompareReports => "progress-worker-comparing-reports",
                        WorkerType::RagQuery => "progress-worker-searching-knowledge",
                    };
                    let executing_msg = self.lang_manager.get_msg(lang_code, progress_key);

                    tx.send(StreamEvent::Progress {
                        request_id: context.request_id.clone(),
                        status: "executing_worker".to_string(),
                        percent: 50,
                        message: executing_msg,
                    })
                    .await?;

                    let mut worker_agent_context = context.clone();
                    worker_agent_context.object_id = current_context.object_id.clone();
                    worker_agent_context.prev_leaf = current_context.current_report_id.clone();
                    worker_agent_context.next_leaf = current_context.previous_report_id.clone();

                    let result = self.execute_worker_via_agent(
                        state.clone(), worker_agent_context, worker_req.clone(), tx.clone(),
                        &worker_results,
                    ).await.map_err(|e| {
                        tracing::error!(worker_type = ?worker_req.worker_type,"Worker execution failed: {}", e);
                        e
                    })?;

                    stats.record_worker(
                        &format!("{:?}", result.worker_type),
                        result.metadata.execution_time_ms,
                        result.metadata.llm_calls,
                        result.metadata.tokens_used,
                    );

                    worker_results.push(result);

                    // Propagate resolved IDs back into context so subsequent
                    // orchestrator calls and workers see the updated values.
                    Self::apply_resolution_to_context(
                        &mut current_context,
                        worker_results.last().unwrap(),
                    );
                }

                OrchestratorDecision::RequestContextFromUser {
                    missing_field,
                    prompt: orchestrator_prompt,
                    suggestions: orchestrator_suggestions,
                } => {
                    // Resolve the canonical localized prompt for the missing field.
                    // The orchestrator may supply its own prompt/suggestions, but we
                    // prefer the FTL-sourced messages for consistency and i18n.
                    //
                    // Three cases where this branch fires:
                    //  1. object_id missing  (any intent)
                    //  2. DescribeReport: no current_report_id AND no previous_report_id
                    //  3. CompareReports: both report IDs absent
                    let (msg_key, suggestions_key) = match missing_field {
                        ContextField::ObjectId => (
                            "context-request-select-object",
                            "context-request-select-object-hint",
                        ),
                        ContextField::CurrentReportId => (
                            "context-request-select-report",
                            "context-request-select-report-hint",
                        ),
                        ContextField::PreviousReportId => (
                            "context-request-select-previous-report",
                            "context-request-select-previous-report-hint",
                        ),
                    };

                    let prompt =
                        self.lang_manager
                            .get_msg_or(lang_code, msg_key, &orchestrator_prompt);

                    // Suggestions: prefer orchestrator-supplied list (it is context-aware);
                    // fall back to a single FTL hint string when the list is empty.
                    let suggestions = if !orchestrator_suggestions.is_empty() {
                        orchestrator_suggestions
                    } else {
                        let hint = self.lang_manager.get_msg(lang_code, suggestions_key);
                        if hint.is_empty() || hint.starts_with("Missing message:") {
                            vec![]
                        } else {
                            vec![hint]
                        }
                    };

                    tx.send(StreamEvent::ContextRequest {
                        request_id: context.request_id.clone(),
                        prompt,
                        suggestions,
                    })
                    .await?;
                    stats.finalize();
                    tx.send(StreamEvent::Completed {
                        request_id: context.request_id.clone(),
                        total_time_ms: start_time.elapsed().as_millis() as u64,
                        stats,
                    })
                    .await?;
                    return Ok(());
                }

                OrchestratorDecision::SendProgress {
                    status,
                    percent,
                    message,
                } => {
                    tx.send(StreamEvent::Progress {
                        request_id: context.request_id.clone(),
                        status,
                        percent,
                        message,
                    })
                    .await?;
                }

                OrchestratorDecision::FormatAndReturn => {
                    if matches!(classification.intent, Intent::RagQuery) {
                        // RagQuery: ChatAgent already streamed all TextChunks — just complete.
                        tracing::info!("RagQuery completed");
                    } else {
                        // Structured intents: format and stream immediately without looping.
                        // Previously this fell through to the next iteration for intent_ready —
                        // which wasted one orchestration step. Now we act here directly.
                        tracing::info!(
                            intent = ?classification.intent,
                            "FormatAndReturn received — formatting immediately"
                        );
                        let ready =
                            Self::intent_ready(classification.intent.clone(), &worker_results);
                        if !ready.is_empty() {
                            save_session(
                                &state.db,
                                &current_context.user_id,
                                &current_context.chat_id,
                                current_context.object_id.as_deref(),
                                current_context.current_report_id.as_deref(),
                                current_context.previous_report_id.as_deref(),
                            )
                            .await;
                            self.format_and_stream_response(
                                &tx,
                                &classification.intent,
                                &ready,
                                &context,
                            )
                            .await?;
                        } else {
                            tracing::warn!(
                                intent = ?classification.intent,
                                "FormatAndReturn but no ready worker results — nothing to format"
                            );
                        }
                    }
                    break;
                }

                OrchestratorDecision::Reject { reason, message: _ } => {
                    // Raw LLM message is discarded — route through the formatter
                    // so the response is always in the user's language via Tera.
                    tracing::warn!(reason = %reason, "Request rejected by orchestrator");
                    match self
                        .formatter
                        .format_out_of_scope_with_model(
                            &current_context.language,
                            &context.message,
                            &state.ai_config.text_model,
                        )
                        .await
                    {
                        Ok(formatted) => {
                            self.send_text_chunks(&tx, &formatted, &context.request_id)
                                .await?;
                        }
                        Err(e) => {
                            // Formatter failed — fall back to a plain FTL key.
                            tracing::error!("format_out_of_scope failed in Reject branch: {}", e);
                            let fallback = self
                                .lang_manager
                                .get_msg(lang_code, "error-request-rejected");
                            tx.send(StreamEvent::TextChunk {
                                request_id: context.request_id.clone(),
                                chunk: fallback,
                            })
                            .await
                            .unwrap_or_else(|e| {
                                tracing::error!("Failed to send rejection fallback: {}", e)
                            });
                        }
                    }
                    break;
                }
            }
        }
        stats.finalize();
        tx.send(StreamEvent::Completed {
            request_id: context.request_id.clone(),
            total_time_ms: start_time.elapsed().as_millis() as u64,
            stats: stats.clone(),
        })
        .await?;

        Ok(())
    }

    fn intent_ready(intent: Intent, worker_results: &[WorkerResponse]) -> Vec<WorkerResponse> {
        let worker_type = match intent {
            Intent::DescribeReport => Some(WorkerType::DescribeReport),
            Intent::CompareReports => Some(WorkerType::CompareReports),
            Intent::GetObjectTree => Some(WorkerType::GetObjectTree),
            Intent::GetReportList => Some(WorkerType::GetReportList),
            _ => None,
        };

        if let Some(wt) = worker_type {
            worker_results
                .iter()
                .filter(|r| r.worker_type == wt)
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    }

    // Map WorkerType to existing Task/Agent
    // Bridge worker execution to existing agents
    async fn execute_worker_via_agent(
        &self,
        state: Arc<AppState>,
        context: AgentContext,
        worker_request: WorkerRequest,
        event_tx: Sender<StreamEvent>,
        previous_results: &[WorkerResponse],
    ) -> Result<WorkerResponse, Box<dyn std::error::Error + Send + Sync>> {
        let start = Instant::now();
        let (result_data, tokens, llm_calls) = match worker_request.parameters {
            WorkerParameters::GetObjectTree { task_params } => {
                let agent =
                    ObjectAgent::new(context.clone(), event_tx.clone(), self.lang_manager.clone());
                // get object tree is a simple lookup, no LLM calls, so tokens and calls are 0
                let result = agent.execute(state, &task_params).await?;
                (result, None, 0u32)
            }
            WorkerParameters::ObjectIdFinder { object_name } => {
                let agent = ObjectIdFinder::new(context.clone());
                // get object id by name is a simple lookup, no LLM calls, so tokens and calls are 0
                let result: String = agent.execute(state, &object_name).await?;
                (json!(result), None, 0u32)
            }
            WorkerParameters::DocumentsIdFinder {
                task_params,
                object_id,
            } => {
                let agent = DocumentsIdFinder::new(context.clone());
                // get documents id by object id is a simple lookup, no LLM calls, so tokens and calls are 0
                let result: ReportPair = agent.execute(state, &task_params, &object_id).await?;
                tracing::debug!("DocumentsIdFinder result: {:?}", &result);
                (json!(result), None, 0u32)
            }
            WorkerParameters::GetReportList { task_params } => {
                let agent = DocumentAgent::new(
                    context.clone(),
                    event_tx.clone(),
                    self.lang_manager.clone(),
                );
                // get report list is a simple lookup, no LLM calls, so tokens and calls are 0
                let result = agent.execute(state, &task_params).await?;
                (result, None, 0u32)
            }
            WorkerParameters::DescribeReport { reports } => {
                let agent = DescriptionAgent::new(
                    self.client.clone(),
                    context.clone(),
                    event_tx.clone(),
                    self.lang_manager.clone(),
                    self.template_manager.clone(),
                );
                // it is heavy operation with LLM calls, so we count tokens and calls
                let (result, tokens, calls) = agent.execute_by_id(&state, &reports).await?;
                (result, tokens, calls)
            }
            WorkerParameters::CompareReports { reports: _ } => {
                let descriptions: Vec<Value> = previous_results
                    .iter()
                    .filter(|r| r.worker_type == WorkerType::DescribeReport)
                    .map(|r| r.data.clone())
                    .collect();

                if descriptions.is_empty() {
                    let err = AgentError::InsufficientDescriptions { found: 0 }.into();
                    tracing::error!("InsufficientDescriptions: {}", err);
                    return Err(err);
                }

                let agent = ComparisonAgent::new(
                    self.client.clone(),
                    context.clone(),
                    event_tx.clone(),
                    self.lang_manager.clone(),
                    self.template_manager.clone(),
                );
                // it is heavy operation with LLM calls, so we count tokens
                let (result, tokens) = agent.execute_comparison(&state, descriptions).await?;
                (result, tokens, 1u32)
            }
            WorkerParameters::RagQuery { query: _ } => {
                let agent = ChatAgent::new(
                    self.client.clone(),
                    context.clone(),
                    self.lang_manager.clone(),
                    event_tx.clone(),
                );

                let (result, tokens) = agent.execute(state, &context.message).await?;
                (json!({ "answer": result }), tokens, 1u32)
            }
        };

        // cache_hit: workers that can cache (DescribeReport, CompareReports) return
        // llm_calls == 0 when the result came from DB. All other workers never cache.
        let cache_hit = matches!(
            worker_request.worker_type,
            WorkerType::DescribeReport | WorkerType::CompareReports
        ) && llm_calls == 0;

        Ok(WorkerResponse {
            worker_type: worker_request.worker_type,
            status: WorkerStatus::Success,
            data: result_data,
            metadata: WorkerMetadata {
                execution_time_ms: start.elapsed().as_millis() as u64,
                data_source: "agent".to_string(),
                cache_hit,
                tokens_used: tokens,
                llm_calls,
            },
        })
    }
    async fn format_and_stream_response(
        &self,
        tx: &Sender<StreamEvent>,
        intent: &Intent,
        worker_results: &[WorkerResponse],
        context: &AgentContext,
    ) -> Result<(), AgentError> {
        let lang = Language::from_short(&context.language);
        let lang_code = lang.to_code();

        // Helper: find first result matching the expected worker type
        let find = |wt: WorkerType| worker_results.iter().find(|r| r.worker_type == wt);

        match intent {
            Intent::DescribeReport => match find(WorkerType::DescribeReport) {
                Some(result) => {
                    tx.send(StreamEvent::Description {
                        request_id: context.request_id.clone(),
                        data: result.data.clone(),
                    })
                    .await
                    .unwrap_or_else(|_| Self::failed_to_send(context));
                }
                None => self.empty_results(tx, intent, context, lang_code).await?,
            },
            Intent::CompareReports => match find(WorkerType::CompareReports) {
                Some(result) => {
                    tx.send(StreamEvent::Comparison {
                        request_id: context.request_id.clone(),
                        data: result.data.clone(),
                    })
                    .await
                    .unwrap_or_else(|_| Self::failed_to_send(context));
                }
                None => self.empty_results(tx, intent, context, lang_code).await?,
            },
            Intent::GetObjectTree => match find(WorkerType::GetObjectTree) {
                Some(result) => {
                    tx.send(StreamEvent::ObjectTree {
                        request_id: context.request_id.clone(),
                        data: result.data.clone(),
                    })
                    .await
                    .unwrap_or_else(|_| Self::failed_to_send(context));
                }
                None => self.empty_results(tx, intent, context, lang_code).await?,
            },
            Intent::GetReportList => match find(WorkerType::GetReportList) {
                Some(result) => {
                    tx.send(StreamEvent::ReportList {
                        request_id: context.request_id.clone(),
                        data: result.data.clone(),
                    })
                    .await
                    .unwrap_or_else(|_| Self::failed_to_send(context));
                }
                None => self.empty_results(tx, intent, context, lang_code).await?,
            },
            Intent::RagQuery => {
                if find(WorkerType::RagQuery).is_none() {
                    self.empty_results(tx, intent, context, lang_code).await?;
                }
                // ChatAgent already streamed TextChunks during execution
            }
            Intent::Ambiguous => {
                tracing::warn!(
                    request_id = %context.request_id,
                    "Unexpected: Ambiguous intent reached format_and_stream_response"
                );
                self.empty_results(tx, intent, context, lang_code).await?;
            }
            _ => {}
        }

        Ok(())
    }
    fn failed_to_send(context: &AgentContext) {
        tracing::error!(
            "Failed to send response for request_id: {}",
            context.request_id
        );
    }

    async fn send_text_chunks(
        &self,
        tx: &Sender<StreamEvent>,
        text: &str,
        request_id: &str,
    ) -> Result<(), AgentError> {
        let sentences: Vec<&str> = text.unicode_sentences().collect();

        for sentence in sentences {
            let trimmed = sentence.trim();
            if trimmed.is_empty() {
                continue;
            }

            tx.send(StreamEvent::TextChunk {
                request_id: request_id.to_string(),
                chunk: trimmed.to_string(),
            })
            .await
            .unwrap_or_else(|_| tracing::error!("Failed to send TextChunk: {}", trimmed));

            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }

        Ok(())
    }
    async fn empty_results(
        &self,
        tx: &Sender<StreamEvent>,
        intent: &Intent,
        context: &AgentContext,
        lang_code: &str,
    ) -> Result<(), AgentError> {
        tracing::warn!(
            "format_and_stream_response: no worker results for {:?}",
            intent
        );
        tx.send(StreamEvent::TextChunk {
            request_id: context.request_id.clone(),
            chunk: self.lang_manager.get_msg(lang_code, "error-no-results"),
        })
        .await
        .unwrap_or_else(|_| {
            tracing::error!(
                "empty_results: failed to send TextChunk for request {}",
                context.request_id
            )
        });
        Ok(())
    }

    /// Checks whether the context contains all fields required for `intent`.
    ///
    /// Called **before** the orchestration loop so missing context is caught early
    /// and returned as `StreamEvent::ContextRequest` instead of crashing inside a
    /// worker with an opaque internal error.
    ///
    /// # Auto-resolution bypass
    /// Fields that the orchestration loop can resolve automatically are skipped:
    /// - `object_id` is skipped when `extracted.object_identifier` is present —
    ///   `ObjectIdFinder` will resolve it in the first orchestration step.
    /// - Report IDs are skipped when `extracted.task_params` is present —
    ///   `DocumentsIdFinder` will resolve them after `object_id` is known.
    ///
    /// # Validation rules (non-resolvable cases only)
    /// - `GetReportList`, `DescribeReport`, `CompareReports` require `object_id`
    ///   when no `object_identifier` was extracted from the message.
    /// - `DescribeReport` requires at least one report ID when no `task_params`
    ///   were extracted (user gave no time/count hint at all).
    /// - `CompareReports` requires both report IDs under the same condition.
    ///
    /// Returns `Some((msg_key, hint_key, field))` when something is missing,
    /// `None` when all required context is present or can be auto-resolved.
    fn missing_context_for_intent(
        intent: &Intent,
        ctx: &UserContext,
        extracted: &ExtractedParameters,
    ) -> Option<(&'static str, &'static str, ContextField)> {
        let needs_object = matches!(
            intent,
            Intent::GetReportList | Intent::DescribeReport | Intent::CompareReports
        );

        // ObjectIdFinder can resolve object_id when the user named an object.
        let can_resolve_object = extracted.object_identifier.is_some();

        if needs_object && ctx.object_id.is_none() && !can_resolve_object {
            return Some((
                "context-request-select-object",
                "context-request-select-object-hint",
                ContextField::ObjectId,
            ));
        }

        // DocumentsIdFinder can resolve report IDs when task_params carry a
        // meaningful time / count hint (last, all, period, or amount).
        let can_resolve_reports = extracted.task_params.is_some();

        match intent {
            Intent::DescribeReport => {
                if ctx.current_report_id.is_none()
                    && ctx.previous_report_id.is_none()
                    && !can_resolve_reports
                {
                    return Some((
                        "context-request-select-report",
                        "context-request-select-report-hint",
                        ContextField::CurrentReportId,
                    ));
                }
            }
            Intent::CompareReports => {
                // Comparison requires exactly two reports to describe and then diff.
                // Only ask the user when DocumentsIdFinder cannot resolve them.
                if ctx.current_report_id.is_none() && !can_resolve_reports {
                    return Some((
                        "context-request-select-previous-report",
                        "context-request-select-previous-report-hint",
                        ContextField::CurrentReportId,
                    ));
                }
                if ctx.previous_report_id.is_none() && !can_resolve_reports {
                    return Some((
                        "context-request-select-second-report",
                        "context-request-select-second-report-hint",
                        ContextField::PreviousReportId,
                    ));
                }
            }
            _ => {}
        }

        None
    }

    /// Writes resolved IDs from `ObjectIdFinder` and `DocumentsIdFinder` results
    /// back into `context` so subsequent orchestrator calls and workers see the
    /// updated values without an extra round-trip.
    fn apply_resolution_to_context(context: &mut UserContext, result: &WorkerResponse) {
        match result.worker_type {
            WorkerType::ObjectIdFinder => {
                // ObjectIdFinder returns a plain UUID string as JSON value.
                if let Some(id) = result.data.as_str() {
                    tracing::info!(object_id = %id, "ObjectIdFinder: resolved object_id");
                    context.object_id = Some(id.to_string());
                }
            }
            WorkerType::DocumentsIdFinder => {
                // DocumentsIdFinder returns ReportPair { prev, next? }.
                if let Some(prev) = result.data["prev"].as_str() {
                    tracing::info!(current_report_id = %prev, "DocumentsIdFinder: resolved current_report_id");
                    context.current_report_id = Some(prev.to_string());
                } else {
                    tracing::warn!("DocumentsIdFinder: no 'prev' report_id found in result");
                }
                // next is Option<String> — only set when a second report exists.
                if let Some(next) = result.data["next"].as_str() {
                    tracing::info!(previous_report_id = %next, "DocumentsIdFinder: resolved previous_report_id");
                    context.previous_report_id = Some(next.to_string());
                } else {
                    tracing::info!(
                        "DocumentsIdFinder: no 'next' report_id found in result — this is expected when only one report matches the criteria"
                    );
                }
            }
            // All other workers do not affect context.
            _ => {}
        }
    }
}

impl Clone for MasterAgent {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            config: self.config.clone(),
            request_manager: self.request_manager.clone(),
            intent_router: self.intent_router.clone(),
            orchestrator: self.orchestrator.clone(),
            formatter: self.formatter.clone(),
            lang_manager: self.lang_manager.clone(),
            template_manager: self.template_manager.clone(),
        }
    }
}
