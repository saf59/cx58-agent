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
use crate::agents::stats::AgentStats;
use crate::localization::LocalizationManager;
use crate::templating::TemplateManager;
use crate::{AgentContext, AgentRequest, AiConfig, AppState, RequestManager, StreamEvent};

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

const MAX_ORCHESTRATION_STEPS: u32 = 5;
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
        let (tx, rx) = mpsc::channel(100);

        let lang_manager = self.lang_manager.clone();
        let state = state.clone();
        let agent = self.clone();
        let start_time = Instant::now();

        tokio::spawn(async move {
            let request_id = Uuid::now_v7().to_string();
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
                .process_request(state, context.clone(), tx.clone())
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
                        stats : AgentStats::default()
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
            .classify(&context.message, &user_context, &[])
            .await?;
        tracing::info!("Intent classification result: {:?}", &classification.intent);
        stats.record_router(router_tokens, Some(intent_start.elapsed().as_millis() as u64));
        context.cancellation_token.check().await?;

        // Intent::Ambiguous handling path.
        // No explicit branch for Intent::Ambiguous here.
        // intentional: Orchestrator handles this via RequestContextFromUser

        if matches!(classification.intent, Intent::OutOfScope) {
            let message = self
                .formatter
                .format_out_of_scope(&user_context.language, &context.message)
                .await?;

            self.send_text_chunks(&tx, &message, &context.request_id).await?;
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
        if let Some((msg_key, hint_key, field)) =
            Self::missing_context_for_intent(&classification.intent, &user_context)
        {
            let prompt = {
                let ftl = self.lang_manager.get_msg(lang_code, msg_key);
                if ftl.is_empty() || ftl.starts_with("Missing message:") {
                    msg_key.to_string()
                } else {
                    ftl
                }
            };
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
        let current_context = user_context.clone();
        let mut step_count = 0u32;

        loop {
            step_count += 1;
            if step_count > MAX_ORCHESTRATION_STEPS {
                tracing::error!(request_id = %context.request_id, "Orchestration loop exceeded max steps");
                return Err(AgentError::internal("Orchestration loop limit exceeded").into());
            }

            context.cancellation_token.check().await?;

            // Guard: CompareReports require DescribeReport
            if matches!(classification.intent, Intent::CompareReports) {
                let has_describe = worker_results
                    .iter()
                    .any(|r| r.worker_type == WorkerType::DescribeReport);

                if !has_describe {
                    // Force DescribeReport execution without going through LLM orchestration.
                    // Safety: current_report_id is guaranteed present by
                    // missing_context_for_intent() which runs before this loop.
                    #[allow(clippy::expect_used)]
                    let reports = ReportPair {
                        prev: current_context
                            .current_report_id
                            .clone()
                            .expect("current_report_id guaranteed by context validation"),
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

                    let result = self
                        .execute_worker_via_agent(
                            state.clone(),
                            context.clone(),
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
                    continue; // next iteration — DescribeReport result is now available
                }
            }
            let ready = Self::intent_ready(classification.intent.clone(), &worker_results);
            if !ready.is_empty() {
                tracing::info!(
                    "Intent ready for response formatting: {:?}",
                    &classification.intent
                );
                self.format_and_stream_response(
                    &tx,
                    &classification.intent,
                    &ready,
                    &context,
                    //&current_context,
                )
                .await?;
                break;
            }
            let orch_start = Instant::now();
            let (decision, orch_tokens) = self
                .orchestrator
                .decide_next_step(
                    &classification,
                    &current_context,
                    &context.message,
                    &worker_results,
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

                    let worker_name = format!("{:?}", worker_req.worker_type);
                    let executing_msg = self.lang_manager
                        .get_msg_with_arg(lang_code, "progress-executing-worker", "worker", &worker_name);

                    tx.send(StreamEvent::Progress {
                        request_id: context.request_id.clone(),
                        status: "executing_worker".to_string(),
                        percent: 50,
                        message: executing_msg,
                    })
                    .await?;

                    let result = self.execute_worker_via_agent(
                        state.clone(), context.clone(), worker_req.clone(), tx.clone(),
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

                    let prompt = {
                        let ftl = self.lang_manager.get_msg(lang_code, msg_key);
                        if ftl.is_empty() || ftl.starts_with("Missing message:") {
                            orchestrator_prompt
                        } else {
                            ftl
                        }
                    };

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

                OrchestratorDecision::FormatAndReturn {
                    worker_results: _decision_results,
                } => {
                    if matches!(classification.intent, Intent::RagQuery) {
                        tracing::info!("RagQuery completed");
                        stats.finalize();
                        tx.send(StreamEvent::Completed {
                            request_id: context.request_id.clone(),
                            total_time_ms: start_time.elapsed().as_millis() as u64,
                            stats: stats.clone(),
                        }).await?;

                        break;
                    }
                    // For structured intents (DescribeReport, CompareReports, GetObjectTree,
                    // GetReportList) — formatting is handled exclusively by intent_ready check
                    // at the top of the loop. Worker results are already correct JSON objects
                    // and do not need LLM formatting. Just loop again so intent_ready fires.
                    tracing::info!(
                        intent = ?classification.intent,
                        "FormatAndReturn received — deferring to intent_ready on next iteration"
                    );
                    // Do NOT break here — let the next iteration handle it via intent_ready.
                    // step_count is already incremented so the loop guard will catch runaway loops.
                }

                OrchestratorDecision::Reject { reason, message } => {
                    tracing::warn!(reason = %reason, "Request rejected by orchestrator");
                    tx.send(StreamEvent::TextChunk {
                        request_id: context.request_id.clone(),
                        chunk: message,
                    })
                    .await
                    .unwrap_or_else(|e| tracing::error!("Failed to send rejection message: {}", e));
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
            WorkerParameters::GetObjectTree(task_params) => {
                let agent = ObjectAgent::new(context.clone());
                let result = agent.execute(state, &task_params).await?;
                (result, None, 0u32)
            }
            WorkerParameters::GetReportList {
                task_params,
            } => {
                let agent = DocumentAgent::new(
                    context.clone(),
                    event_tx.clone(),
                    self.lang_manager.clone(),
                );

                let result = agent
                    .execute(state, &task_params)
                    .await?;
                (result, None, 0u32)
            }
            WorkerParameters::DescribeReport { reports } => {
                let agent = DescriptionAgent::new(
                    self.client.clone(),
                    context.clone(),
                    event_tx.clone(),
                    self.lang_manager.clone(),
                    self.template_manager.clone()
                );

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
                //let descriptions_value = Value::Array(descriptions);
                let (result, tokens) = agent.execute_comparison(&state, descriptions).await?;
                (result, tokens, 1u32)
            }
            WorkerParameters::RagQuery { query: _ } => {
                let agent = ChatAgent::new(self.client.clone(), context.clone(), event_tx.clone());

                let (result, tokens) = agent.execute(state, &context.message).await?;
                (json!({ "answer": result }), tokens, 1u32)
            }
        };

        Ok(WorkerResponse {
            worker_type: worker_request.worker_type,
            status: WorkerStatus::Success,
            data: result_data,
            metadata: WorkerMetadata {
                execution_time_ms: start.elapsed().as_millis() as u64,
                data_source: "agent".to_string(),
                cache_hit: false,
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
    /// # Validation rules
    /// - `GetReportList`, `DescribeReport`, `CompareReports` all require `object_id`.
    /// - `DescribeReport` requires at least one of the two report IDs.
    /// - `CompareReports` requires **both** `current_report_id` and `previous_report_id`
    ///   because the comparison worker always describes two images. Missing either ID
    ///   causes `InsufficientDescriptions` inside `ComparisonAgent`.
    ///
    /// Returns `Some((msg_key, hint_key, field))` when something is missing,
    /// `None` when all required context is present.
    fn missing_context_for_intent(
        intent: &Intent,
        ctx: &UserContext,
    ) -> Option<(&'static str, &'static str, ContextField)> {
        let needs_object = matches!(
            intent,
            Intent::GetReportList | Intent::DescribeReport | Intent::CompareReports
        );
        if needs_object && ctx.object_id.is_none() {
            return Some((
                "context-request-select-object",
                "context-request-select-object-hint",
                ContextField::ObjectId,
            ));
        }

        match intent {
            Intent::DescribeReport => {
                if ctx.current_report_id.is_none() && ctx.previous_report_id.is_none() {
                    return Some((
                        "context-request-select-report",
                        "context-request-select-report-hint",
                        ContextField::CurrentReportId,
                    ));
                }
            }
            Intent::CompareReports => {
                // Comparison requires exactly two reports to describe and then diff.
                // Check current_report_id first; if that is present but previous is
                // absent, ask specifically for the second (older) report.
                if ctx.current_report_id.is_none() {
                    return Some((
                        "context-request-select-previous-report",
                        "context-request-select-previous-report-hint",
                        ContextField::CurrentReportId,
                    ));
                }
                if ctx.previous_report_id.is_none() {
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
