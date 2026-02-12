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
//! // Data chunks (intent-specific)
//! {"chunk_type": "object_tree", "data": {"objects": [...]}}
//! {"chunk_type": "report_list", "data": [{"id": "1", "date": "2024-01-01"}]}
//! {"chunk_type": "description", "data": {"report_id": "...", "text": "...", "is_complete": true}}
//! {"chunk_type": "comparison", "data": {"differences": [...]}}
//! {"chunk_type": "text_chunk", "data": {"content": "...", "language": "en"}}
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
//! - Errors during processing send `StreamChunk::Error` to SSE
//! - Tera template fallback used for error messages
//! - Request lifecycle terminates on first error
//!
//! ## Thread Safety
//!
//! - Uses `Arc` for shared state (LocalizationManager, TemplateManager)
//! - SSE channel created per request (100 buffer capacity)
//! - `Clone` implementation creates new agent instances (production should use Arc)
//!
//! TODO (Performance Alignment):
//! - caching layers (ObjectTree, ReportList, Image Description).
//!   No caching mechanism is implemented here.

use anyhow::Result;
use rig::providers::ollama;
use std::sync::Arc;
use std::time::Instant;
use tera::Context;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::{
    intent_router::IntentRouter,
    orchestrator::Orchestrator,
    response_formatter::ResponseFormatter,
    types::*,
};

use crate::localization::LocalizationManager;
use crate::templating::TemplateManager;
use crate::{AiConfig, AgentRequest, AppState, RequestManager, AgentContext};

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
///
/// TODO (Architecture Gap):
/// - Rejection Handler as a specialized worker.
///   Here, rejection is handled inline via OrchestratorDecision::Reject.
/// - Knowledge Base Worker (RAG) as a first-class worker.
///   RagQuery worker_type exists but is not fully formatted downstream.
/// - No Evaluator/Compliance agent layer for quality control.
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
    pub fn new(
        client: Arc<ollama::Client>,
        config: AiConfig,
    ) -> Self {
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

    /// Handles an agent request and returns an SSE stream of responses.
    ///
    /// TODO (Scalability):
    /// - streaming progress percentages aligned to workflow phases.
    ///   Current percentages are static (10, 30, 50, 80).
    /// - No backpressure monitoring on channel capacity.
    pub async fn handle_request_stream(
        &self,
        state: Arc<AppState>,
        request: AgentRequest,
    ) -> mpsc::Receiver<StreamChunk> {
        let (tx, rx) = mpsc::channel(100);

        let template_manager = self.template_manager.clone();
        let state = state.clone();
        let agent = self.clone();
        tokio::spawn(async move {
            let request_id  = Uuid::now_v7().to_string();
            let cancellation_token = agent.request_manager.register(request_id.clone()).await;
            let context = AgentContext::from_request(request_id.clone(), request.clone(), cancellation_token.clone());

            if let Err(e) = agent.process_request(state, context, tx.clone()).await {
                let mut ctx = Context::new();
                ctx.insert("error", &e.to_string());

                let error_msg = template_manager
                    .render("en", "error-agent", ctx)
                    .unwrap_or_else(|_| format!("Agent error: {}", e));

                let _ = tx.send(StreamChunk::Error {
                    message: error_msg,
                    code: "AGENT_ERROR".to_string(),
                }).await;
            }
        });

        rx
    }

    async fn process_request(
        &self,
        state: Arc<AppState>,
        context: AgentContext,
        tx: mpsc::Sender<StreamChunk>,
    ) -> Result<()> {
        let start_time = Instant::now();

        let lang = Language::from_short(&context.language);
        let lang_code = lang.to_code();
        
        let analyzing_msg = self.lang_manager.get_msg(lang_code, "progress-analyzing");
        tx.send(StreamChunk::Progress {
            request_id: context.request_id.clone(),
            status: "analyzing".to_string(),
            percent: 10,
            message: analyzing_msg,
        }).await?;
        
        let user_context = UserContext {
            user_id: context.user_id.clone(),
            chat_id: context.chat_id.clone(),
            language: lang.clone(),
            object_id: context.object_id.clone(),
            current_report_id: context.next_leaf.clone(),
            previous_report_id: context.prev_leaf.clone(),
        };
        
        let classification = self.intent_router
            .classify(&context.message, &user_context, &[])
            .await?;
        
        // TODO (Ambiguity Gap):
        // Intent::Ambiguous handling path.
        // No explicit branch for Intent::Ambiguous here.
        
        if matches!(classification.intent, Intent::OutOfScope) {
            let message = self.formatter
                .format_out_of_scope(&user_context.language, &context.message)
                .await?;
            
            self.send_text_chunks(&tx, &message, &user_context.language).await?;
            
            tx.send(StreamChunk::Complete {
                total_time_ms: start_time.elapsed().as_millis() as u64,
            }).await?;
            
            return Ok(());
        }
        
        let validation_msg = self.lang_manager.get_msg(lang_code, "progress-context-validation");
        tx.send(StreamChunk::Progress {
            request_id: context.request_id.clone(),
            status: "context_validation".to_string(),
            percent: 30,
            message: validation_msg,
        }).await?;
        
        let mut worker_results = Vec::new();
        let current_context = user_context.clone();
        
        loop {
            let decision = self.orchestrator
                .decide_next_step(
                    &classification,
                    &current_context,
                    &context.message,
                    &worker_results,
                )
                .await?;
            
            match decision {
                OrchestratorDecision::ExecuteWorker(mut worker_req) => {
                    worker_req.context.user_id = current_context.user_id.clone();
                    worker_req.context.language = current_context.language.clone();
                    
                    let mut ctx = Context::new();
                    ctx.insert("worker_type", &format!("{:?}", worker_req.worker_type));
                    let executing_msg = self.template_manager
                        .render(lang_code, "progress-executing-worker", ctx)
                        .unwrap_or_else(|_| format!("Executing {:?}...", worker_req.worker_type));
                    
                    tx.send(StreamChunk::Progress {
                        request_id: context.request_id.clone(),
                        status: "executing_worker".to_string(),
                        percent: 50,
                        message: executing_msg,
                    }).await?;
                    
                    let result = self.execute_worker(&state, worker_req).await?;
                    worker_results.push(result);
                }
                
                OrchestratorDecision::RequestContextFromUser { missing_field: _, prompt, suggestions } => {
                    // TODO:
                    // structured clarification flow.
                    // Current implementation concatenates suggestions as plain text.
                    
                    let prompt_with_suggestions = if !suggestions.is_empty() {
                        format!("{} Suggestions: {}", prompt, suggestions.join(", "))
                    } else {
                        prompt
                    };
                    tx.send(StreamChunk::TextChunk {
                        content: prompt_with_suggestions,
                        language: current_context.language.as_str().to_string(),
                    }).await?;
                    
                    tx.send(StreamChunk::Complete {
                        total_time_ms: start_time.elapsed().as_millis() as u64,
                    }).await?;
                    return Ok(());
                }
                
                OrchestratorDecision::SendProgress { status, percent, message } => {
                    tx.send(StreamChunk::Progress {
                        request_id: context.request_id.clone(),
                        status,
                        percent,
                        message,
                    }).await?;
                }
                
                OrchestratorDecision::FormatAndReturn { worker_results: decision_results } => {
                    let formatting_msg = self.lang_manager.get_msg(lang_code, "progress-formatting");
                    tx.send(StreamChunk::Progress {
                        request_id: context.request_id.clone(),
                        status: "formatting".to_string(),
                        percent: 80,
                        message: formatting_msg,
                    }).await?;

                    self.format_and_stream_response(
                        &tx,
                        &classification.intent,
                        &decision_results,
                        &current_context,
                    ).await?;

                    break;
                }
                
                OrchestratorDecision::Reject { reason: _, message } => {
                    tx.send(StreamChunk::TextChunk {
                        content: message,
                        language: current_context.language.as_str().to_string(),
                    }).await?;
                    break;
                }
            }
        }
        
        tx.send(StreamChunk::Complete {
            total_time_ms: start_time.elapsed().as_millis() as u64,
        }).await?;
        
        Ok(())
    }
    
    /// TODO (Worker Gap):
    /// - Current implementation is stubbed with mock JSON.
    /// - No retry logic implemented despite documentation claim.
    async fn execute_worker(
        &self,
        _state: &Arc<AppState>,
        request: WorkerRequest,
    ) -> Result<WorkerResponse> {
        // TODO: Implement retry logic when actual workers are added
        let start = Instant::now();

        let data = match request.worker_type {
            WorkerType::GetObjectTree => {
                serde_json::json!({
                    "objects": []
                })
            }
            WorkerType::GetReportList => {
                serde_json::json!({
                    "reports": []
                })
            }
            WorkerType::DescribeReport => {
                serde_json::json!({
                    "description": "Sample description"
                })
            }
            WorkerType::CompareReports => {
                serde_json::json!({
                    "differences": []
                })
            }
            WorkerType::RagQuery => {
                serde_json::json!({
                    "answer": "Sample answer"
                })
            }
        };

        Ok(WorkerResponse {
            worker_type: request.worker_type,
            status: WorkerStatus::Success,
            data,
            metadata: WorkerMetadata {
                execution_time_ms: start.elapsed().as_millis() as u64,
                data_source: "database".to_string(),
                cache_hit: false,
            },
        })
    }
    
    async fn format_and_stream_response(
        &self,
        tx: &mpsc::Sender<StreamChunk>,
        intent: &Intent,
        worker_results: &[WorkerResponse],
        context: &UserContext,
    ) -> Result<()> {
        match intent {
            Intent::DescribeReport => {
                if let Some(result) = worker_results.first() {
                    let description = self.formatter
                        .format_description(&result.data, &context.language, "report-id")
                        .await?;
                    
                    self.send_text_chunks(tx, &description, &context.language).await?;
                }
            }
            
            Intent::CompareReports => {
                if worker_results.len() >= 2 {
                    let comparison = self.formatter
                        .format_comparison(
                            &worker_results[0].data.to_string(),
                            &worker_results[1].data.to_string(),
                            &context.language,
                            "report-1",
                            "report-2",
                        )
                        .await?;
                    
                    tx.send(StreamChunk::Comparison {
                        data: comparison,
                    }).await?;
                }
            }
            
            Intent::GetObjectTree => {
                if let Some(result) = worker_results.first() {
                    tx.send(StreamChunk::ObjectTree {
                        data: result.data.clone(),
                    }).await?;
                }
            }
            
            Intent::GetReportList => {
                if let Some(result) = worker_results.first() {
                    let reports = result.data["reports"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    
                    tx.send(StreamChunk::ReportList {
                        data: reports,
                    }).await?;
                }
            }
            
            // TODO:
            // Missing explicit handling for Intent::RagQuery.
            // Knowledge Base Worker with citation support.
            // Also missing fallback handling for Ambiguous intent.
            
            _ => {}
        }
        
        Ok(())
    }
    
    async fn send_text_chunks(
        &self,
        tx: &mpsc::Sender<StreamChunk>,
        text: &str,
        language: &Language,
    ) -> Result<()> {
        // TODO:
        // chunk streaming by semantic units.
        // Current implementation splits by ". " which is naive
        // and may break abbreviations or non-English punctuation.
        
        let sentences: Vec<&str> = text.split(". ").collect();
        
        for sentence in sentences {
            if !sentence.trim().is_empty() {
                tx.send(StreamChunk::TextChunk {
                    content: format!("{}. ", sentence.trim()),
                    language: language.as_str().to_string(),
                }).await?;
                
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            }
        }
        
        Ok(())
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
