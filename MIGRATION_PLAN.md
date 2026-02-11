# MasterAgent Migration Plan: From Current to Enhanced Architecture

## Executive Summary

This document outlines a step-by-step migration plan to upgrade the existing `MasterAgent` from a simple task-based routing system to a sophisticated intent-classification and orchestration architecture. The migration preserves existing cancellation functionality while adding multi-step workflow capabilities, progress tracking, and flexible response formatting.

---

## Current Architecture Analysis

### Existing Components (master_agent.rs)
```
┌────────────────────────────────────────┐
│           MasterAgent                  │
│  ┌──────────────────────────────────┐  │
│  │ CancellationToken                │  │
│  │ RequestManager                   │  │
│  └──────────────────────────────────┘  │
│                ↓                       │
│  ┌──────────────────────────────────┐  │
│  │ ContextParser → TaskDetector     │  │
│  └──────────────────────────────────┘  │
│                ↓                       │
│  ┌──────────────────────────────────┐  │
│  │ Direct Agent Execution:          │  │
│  │ - ObjectAgent                    │  │
│  │ - DocumentAgent                  │  │
│  │ - DescriptionAgent               │  │
│  │ - ComparisonAgent                │  │
│  │ - ChatAgent                      │  │
│  └──────────────────────────────────┘  │
└────────────────────────────────────────┘
```

**Strengths:**
- Working cancellation mechanism via CancellationToken
- Clean request/context separation
- Functional streaming via StreamEvent
- Clear agent routing

**Limitations:**
- Single-step execution (no multi-step workflows)
- No intent classification layer
- Limited progress tracking
- Rigid task-to-agent mapping

---

## Target Architecture (master_agent_update.rs)

```
┌──────────────────────────────────────────────────────────┐
│                    MasterAgent                           │
│  ┌────────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │ IntentRouter   │→ │ Orchestrator │→ │ Formatter    │  │
│  │ (Classify)     │  │ (Workflow)   │  │ (Response)   │  │
│  └────────────────┘  └──────────────┘  └──────────────┘  │
│         ↓                   ↓                  ↓         │
│  Classification      Multi-step          Natural Lang.   │
│  - Intent Type       Execution:           Streaming      │
│  - Context Valid.    - Worker Queue                      │
│  - Ambiguity         - Dynamic Steps                     │
│                      - Context Updates                   │
└──────────────────────────────────────────────────────────┘
                            ↓
                ┌───────────────────────┐
                │  StreamChunk (SSE)    │
                │  - Progress updates   │
                │  - Data chunks        │
                │  - Error handling     │
                └───────────────────────┘
```

**New Capabilities:**
- Intent-based classification (vs. rigid task detection)
- Multi-step orchestration
- Dynamic workflow decisions
- Localization integration
- Template-based formatting
- Progress percentage tracking

---

## Migration Strategy Overview

**Approach:** Incremental enhancement with parallel operation capability

**Phases:**
1. **Foundation** - Add new dependencies and types
2. **Intent Layer** - Introduce IntentRouter alongside TaskDetector
3. **Orchestration** - Add Orchestrator for multi-step workflows
4. **Integration** - Merge cancellation with orchestration
5. **Formatting** - Enhance response streaming
6. **Cleanup** - Deprecate old components

**Timeline Estimate:** 5-7 development days (8-hour days)

---

## Phase 1: Foundation Setup (Day 1)

### 1.1 Add New Dependencies

<details>
<summary>✅ Completed</summary>

**File:** `Cargo.toml`
```toml
[dependencies]
# Existing dependencies...
anyhow = "1.0"
tera = "1.20"
chrono = "0.4"
uuid = { version = "1.0", features = ["v7"] }
```

**Validation:**
```bash
cargo check
```
</details>

### 1.2 Create Type Definitions Module
<details>
<summary>✅ Completed</summary>

**File:** `agents/types.rs`
```rust
// Core types for the enhanced MasterAgent architecture

use serde::{Deserialize, Serialize};

/// Language support enum
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Language {
    English,
    Ukrainian,
    Russian,
}

impl Language {
    pub fn as_str(&self) -> &str {
        match self {
            Language::English => "en",
            Language::Ukrainian => "uk",
            Language::Russian => "ru",
        }
    }
    
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "uk" | "ukrainian" => Language::Ukrainian,
            "ru" | "russian" => Language::Russian,
            _ => Language::English,
        }
    }
}

/// Intent classification results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Intent {
    GetObjectTree,
    GetReportList,
    DescribeReport,
    CompareReports,
    RagQuery,
    OutOfScope,
    Ambiguous,
}

/// User context for request processing
#[derive(Debug, Clone)]
pub struct UserContext {
    pub user_id: String,
    pub chat_id: String,
    pub language: Language,
    pub object_id: Option<String>,
    pub current_report_id: Option<String>,
    pub previous_report_id: Option<String>,
}

/// Intent classification result
#[derive(Debug, Clone)]
pub struct IntentClassification {
    pub intent: Intent,
    pub confidence: f32,
    pub context_valid: bool,
    pub missing_context: Vec<String>,
}

/// Worker types for execution
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum WorkerType {
    GetObjectTree,
    GetReportList,
    DescribeReport,
    CompareReports,
    RagQuery,
}

/// Worker execution status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkerStatus {
    Success,
    PartialSuccess,
    Failed,
}

/// Worker request
#[derive(Debug, Clone)]
pub struct WorkerRequest {
    pub worker_type: WorkerType,
    pub context: WorkerContext,
    pub parameters: serde_json::Value,
}

/// Worker context (derived from UserContext)
#[derive(Debug, Clone)]
pub struct WorkerContext {
    pub user_id: String,
    pub language: Language,
    pub object_id: Option<String>,
    pub report_ids: Vec<String>,
}

/// Worker response
#[derive(Debug, Clone)]
pub struct WorkerResponse {
    pub worker_type: WorkerType,
    pub status: WorkerStatus,
    pub data: serde_json::Value,
    pub metadata: WorkerMetadata,
}

/// Worker execution metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerMetadata {
    pub execution_time_ms: u64,
    pub data_source: String,
    pub cache_hit: bool,
}

/// Orchestrator decision types
#[derive(Debug, Clone)]
pub enum OrchestratorDecision {
    ExecuteWorker { request: WorkerRequest },
    RequestContextFromUser { prompt: String },
    SendProgress { message: String, percent: u8 },
    FormatAndReturn { worker_results: Vec<WorkerResponse> },
    Reject { reason: String, message: String },
}

/// Stream chunk types for SSE
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "chunk_type", content = "data")]
pub enum StreamChunk {
    Progress {
        status: String,
        percent: u8,
        message: String,
    },
    ObjectTree {
        data: serde_json::Value,
    },
    ReportList {
        data: Vec<serde_json::Value>,
    },
    Description {
        report_id: String,
        text: String,
        is_complete: bool,
    },
    Comparison {
        data: serde_json::Value,
    },
    TextChunk {
        content: String,
        language: String,
    },
    Complete {
        total_time_ms: u64,
    },
    Error {
        message: String,
        code: String,
    },
}
```

**File:** `agents/mod.rs`
```rust
// Add to existing exports:
pub mod types;
pub use types::*;
```
</details>

### 1.3 Set Up Localization & Template Managers

<details>
<summary>✅ Completed</summary>

**Note:** These should already exist in your project based on the update file. If not:

**File:** `localization/mod.rs` (stub if needed)
```rust
use std::collections::HashMap;
use std::sync::Arc;

pub struct LocalizationManager {
    messages: HashMap<String, HashMap<String, String>>,
}

impl LocalizationManager {
    pub fn new() -> Self {
        // Load from files or configure messages
        let mut messages = HashMap::new();
        
        // English messages
        let mut en = HashMap::new();
        en.insert("progress-analyzing".to_string(), "Analyzing your request...".to_string());
        en.insert("progress-validating".to_string(), "Validating context...".to_string());
        en.insert("progress-executing".to_string(), "Executing task...".to_string());
        en.insert("progress-formatting".to_string(), "Formatting response...".to_string());
        messages.insert("en".to_string(), en);
        
        Self { messages }
    }
    
    pub fn get_msg(&self, lang: &str, key: &str) -> String {
        self.messages
            .get(lang)
            .and_then(|lang_msgs| lang_msgs.get(key))
            .cloned()
            .unwrap_or_else(|| format!("[{}]", key))
    }
}
```

**File:** `templating/mod.rs` (stub if needed)
```rust
use tera::{Tera, Context};
use anyhow::Result;

pub struct TemplateManager {
    tera: Tera,
}

impl TemplateManager {
    pub fn new() -> Result<Self> {
        let mut tera = Tera::default();
        
        // Add templates programmatically or from files
        tera.add_raw_template("error-agent", 
            "An error occurred: {{ error }}")?;
        
        Ok(Self { tera })
    }
    
    pub fn render(&self, _lang: &str, template: &str, context: Context) -> Result<String> {
        Ok(self.tera.render(template, &context)?)
    }
}
```

**Validation:**
```bash
cargo test --lib
```
</details>
---

## Phase 2: Intent Classification Layer (Day 2)

### 2.1 (✅ Completed) Create IntentRouter

<details>
<summary>✅ Completed</summary>

**File:** `agents/intent_router.rs`
```rust
use anyhow::Result;
use std::sync::Arc;
use crate::localization::LocalizationManager;
use crate::templating::TemplateManager;
use super::types::*;

/// Classifies user intent from natural language queries
pub struct IntentRouter {
    _api_base: String,
    _chat_model: String,
    lang_manager: Arc<LocalizationManager>,
    _template_manager: Arc<TemplateManager>,
}

impl IntentRouter {
    pub fn new(
        api_base: String,
        chat_model: String,
        lang_manager: Arc<LocalizationManager>,
        template_manager: Arc<TemplateManager>,
    ) -> Self {
        Self {
            _api_base: api_base,
            _chat_model: chat_model,
            lang_manager,
            _template_manager: template_manager,
        }
    }
    
    /// Classify user query into an Intent
    pub async fn classify(
        &self,
        query: &str,
        context: &UserContext,
    ) -> Result<IntentClassification> {
        // TODO: Replace with actual LLM call
        // For now, use rule-based classification similar to TaskDetector
        
        let query_lower = query.to_lowercase();
        let intent = if query_lower.contains("object") && query_lower.contains("tree") {
            Intent::GetObjectTree
        } else if query_lower.contains("report") && query_lower.contains("list") {
            Intent::GetReportList
        } else if query_lower.contains("describe") || query_lower.contains("tell me about") {
            Intent::DescribeReport
        } else if query_lower.contains("compare") {
            Intent::CompareReports
        } else if query_lower.contains("what") || query_lower.contains("how") {
            Intent::RagQuery
        } else {
            Intent::Ambiguous
        };
        
        // Validate context requirements
        let (context_valid, missing_context) = self.validate_context(&intent, context);
        
        Ok(IntentClassification {
            intent,
            confidence: 0.85, // Placeholder
            context_valid,
            missing_context,
        })
    }
    
    fn validate_context(&self, intent: &Intent, context: &UserContext) -> (bool, Vec<String>) {
        let mut missing = Vec::new();
        
        match intent {
            Intent::DescribeReport => {
                if context.current_report_id.is_none() {
                    missing.push("current_report_id".to_string());
                }
            }
            Intent::CompareReports => {
                if context.current_report_id.is_none() {
                    missing.push("current_report_id".to_string());
                }
                if context.previous_report_id.is_none() {
                    missing.push("previous_report_id".to_string());
                }
            }
            _ => {}
        }
        
        (missing.is_empty(), missing)
    }
}
```

**File:** `agents/mod.rs`
```rust
// Add:
mod intent_router;
pub use intent_router::IntentRouter;
```
</details>

### 2.2 Bridge Intent to Existing Tasks

<details>
<summary>⁉ Current </summary>

**File:** `agents/intent_bridge.rs`
```rust
use super::types::*;
use super::Task; // From existing code

/// Bridge between new Intent system and existing Task system
pub fn intent_to_task(intent: &Intent) -> Option<Task> {
    match intent {
        Intent::GetObjectTree => Some(Task::Object { 
            parameters: serde_json::json!({"action": "list"}) 
        }),
        Intent::GetReportList => Some(Task::Document { 
            parameters: serde_json::json!({"action": "list"}) 
        }),
        Intent::DescribeReport => Some(Task::Description { 
            parameters: serde_json::json!({"action": "describe"}) 
        }),
        Intent::CompareReports => Some(Task::Comparison { 
            parameters: serde_json::json!({"action": "compare"}) 
        }),
        Intent::RagQuery => Some(Task::Chat),
        Intent::OutOfScope => None,
        Intent::Ambiguous => None,
    }
}
```

**Validation:**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_intent_classification() {
        let lang_mgr = Arc::new(LocalizationManager::new());
        let tmpl_mgr = Arc::new(TemplateManager::new().unwrap());
        
        let router = IntentRouter::new(
            "http://localhost:3050".to_string(),
            "model".to_string(),
            lang_mgr,
            tmpl_mgr,
        );
        
        let context = UserContext {
            user_id: "user1".to_string(),
            chat_id: "chat1".to_string(),
            language: Language::English,
            object_id: None,
            current_report_id: Some("report1".to_string()),
            previous_report_id: None,
        };
        
        let result = router.classify("describe the current report", &context).await.unwrap();
        assert!(matches!(result.intent, Intent::DescribeReport));
        assert!(result.context_valid);
    }
}
```
</details> 

---

## Phase 3: Orchestration Layer (Day 3)

### 3.1 Create Orchestrator
<details>
<summary>✅ Completed</summary>

**File:** `agents/orchestrator.rs`
```rust
use anyhow::Result;
use std::sync::Arc;
use crate::localization::LocalizationManager;
use crate::templating::TemplateManager;
use super::types::*;

/// Orchestrates multi-step workflow execution
pub struct Orchestrator {
    _api_base: String,
    _text_model: String,
    lang_manager: Arc<LocalizationManager>,
    _template_manager: Arc<TemplateManager>,
}

impl Orchestrator {
    pub fn new(
        api_base: String,
        text_model: String,
        lang_manager: Arc<LocalizationManager>,
        template_manager: Arc<TemplateManager>,
    ) -> Self {
        Self {
            _api_base: api_base,
            _text_model: text_model,
            lang_manager,
            _template_manager: template_manager,
        }
    }
    
    /// Decide the next step in workflow
    pub async fn decide_next_step(
        &self,
        classification: &IntentClassification,
        context: &UserContext,
        previous_results: &[WorkerResponse],
    ) -> Result<OrchestratorDecision> {
        // Check for missing context first
        if !classification.missing_context.is_empty() {
            let prompt = format!(
                "Please provide: {}",
                classification.missing_context.join(", ")
            );
            return Ok(OrchestratorDecision::RequestContextFromUser { prompt });
        }
        
        // Check if we already have results to format
        if !previous_results.is_empty() {
            return Ok(OrchestratorDecision::FormatAndReturn {
                worker_results: previous_results.to_vec(),
            });
        }
        
        // Determine worker to execute based on intent
        match &classification.intent {
            Intent::GetObjectTree => {
                Ok(OrchestratorDecision::ExecuteWorker {
                    request: WorkerRequest {
                        worker_type: WorkerType::GetObjectTree,
                        context: self.build_worker_context(context),
                        parameters: serde_json::json!({}),
                    },
                })
            }
            Intent::GetReportList => {
                Ok(OrchestratorDecision::ExecuteWorker {
                    request: WorkerRequest {
                        worker_type: WorkerType::GetReportList,
                        context: self.build_worker_context(context),
                        parameters: serde_json::json!({}),
                    },
                })
            }
            Intent::DescribeReport => {
                Ok(OrchestratorDecision::ExecuteWorker {
                    request: WorkerRequest {
                        worker_type: WorkerType::DescribeReport,
                        context: self.build_worker_context(context),
                        parameters: serde_json::json!({
                            "report_id": context.current_report_id
                        }),
                    },
                })
            }
            Intent::CompareReports => {
                Ok(OrchestratorDecision::ExecuteWorker {
                    request: WorkerRequest {
                        worker_type: WorkerType::CompareReports,
                        context: self.build_worker_context(context),
                        parameters: serde_json::json!({
                            "report_id_1": context.current_report_id,
                            "report_id_2": context.previous_report_id,
                        }),
                    },
                })
            }
            Intent::RagQuery => {
                Ok(OrchestratorDecision::ExecuteWorker {
                    request: WorkerRequest {
                        worker_type: WorkerType::RagQuery,
                        context: self.build_worker_context(context),
                        parameters: serde_json::json!({}),
                    },
                })
            }
            Intent::OutOfScope => {
                let msg = self.lang_manager.get_msg(
                    context.language.as_str(),
                    "error-out-of-scope"
                );
                Ok(OrchestratorDecision::Reject {
                    reason: "out_of_scope".to_string(),
                    message: msg,
                })
            }
            Intent::Ambiguous => {
                let msg = self.lang_manager.get_msg(
                    context.language.as_str(),
                    "error-ambiguous"
                );
                Ok(OrchestratorDecision::Reject {
                    reason: "ambiguous".to_string(),
                    message: msg,
                })
            }
        }
    }
    
    fn build_worker_context(&self, context: &UserContext) -> WorkerContext {
        let mut report_ids = Vec::new();
        if let Some(id) = &context.current_report_id {
            report_ids.push(id.clone());
        }
        if let Some(id) = &context.previous_report_id {
            report_ids.push(id.clone());
        }
        
        WorkerContext {
            user_id: context.user_id.clone(),
            language: context.language,
            object_id: context.object_id.clone(),
            report_ids,
        }
    }
}
```

### 3.2 Add Orchestrator to Module

**File:** `agents/mod.rs`
```rust
mod orchestrator;
pub use orchestrator::Orchestrator;
```
</details>
---

## Phase 4: Integration with Cancellation (Day 4)

### 4.1 Enhance AgentContext with Orchestration Support

**File:** `agents/master_agent.rs`
```rust
// Update AgentContext to include both old and new fields
#[derive(Debug, Clone)]
pub struct AgentContext {
    // Existing fields
    pub request_id: String,
    pub user_id: String,
    pub chat_id: String,
    pub language: String,
    pub object_id: Option<String>,
    pub prev_leaf: Option<String>,
    pub next_leaf: Option<String>,
    pub metadata: serde_json::Value,
    pub cancellation_token: CancellationToken,
    
    // New orchestration fields
    pub user_context: UserContext,
}

impl AgentContext {
    pub fn from_request(
        request_id: String,
        req: AgentRequest,
        cancellation_token: CancellationToken
    ) -> Self {
        let language_enum = Language::from_str(&req.language);
        
        let user_context = UserContext {
            user_id: req.user_id.clone(),
            chat_id: req.chat_id.clone(),
            language: language_enum,
            object_id: req.object_id.clone(),
            current_report_id: None, // TODO: Map from metadata
            previous_report_id: None, // TODO: Map from metadata
        };
        
        Self {
            request_id,
            user_id: req.user_id,
            chat_id: req.chat_id,
            language: req.language,
            object_id: req.object_id,
            prev_leaf: req.prev_leaf,
            next_leaf: req.next_leaf,
            metadata: req.metadata.unwrap_or(serde_json::json!({})),
            cancellation_token,
            user_context,
        }
    }
}
```

### 4.2 Add Orchestration Components to MasterAgent

**File:** `agents/master_agent.rs`
```rust
use super::{IntentRouter, Orchestrator, ResponseFormatter};

pub struct MasterAgent {
    client: ollama::Client,
    request_manager: Arc<RequestManager>,
    
    // New components
    intent_router: IntentRouter,
    orchestrator: Orchestrator,
    formatter: ResponseFormatter,
    lang_manager: Arc<LocalizationManager>,
    template_manager: Arc<TemplateManager>,
}

impl MasterAgent {
    pub fn new(
        ai_url: &str,
        lang_manager: Arc<LocalizationManager>,
        template_manager: Arc<TemplateManager>,
    ) -> Self {
        let client = ollama::Client::builder()
            .api_key(Nothing)
            .base_url(ai_url)
            .build()
            .unwrap();
        
        let text_model = std::env::var("OLLAMA_MODEL")
            .unwrap_or_else(|_| "functiongemma:latest".to_string());
        let chat_model = std::env::var("OLLAMA_CHAT_MODEL")
            .unwrap_or_else(|_| text_model.clone());
        
        Self {
            client: client.clone(),
            request_manager: Arc::new(RequestManager::new()),
            intent_router: IntentRouter::new(
                ai_url.to_string(),
                chat_model,
                lang_manager.clone(),
                template_manager.clone(),
            ),
            orchestrator: Orchestrator::new(
                ai_url.to_string(),
                text_model.clone(),
                lang_manager.clone(),
                template_manager.clone(),
            ),
            formatter: ResponseFormatter::new(
                ai_url.to_string(),
                text_model,
                lang_manager.clone(),
                template_manager.clone(),
            ),
            lang_manager,
            template_manager,
        }
    }
}
```

### 4.3 Create Hybrid Processing Method

**File:** `agents/master_agent.rs`
```rust
impl MasterAgent {
    // Keep existing process_request as process_request_v1
    async fn process_request_v1(/* existing params */) -> Result<...> {
        // Existing implementation
    }
    
    // New orchestration-based processing
    async fn process_request_v2(
        state: Arc<AppState>,
        client: ollama::Client,
        request: AgentRequest,
        context: AgentContext,
        event_tx: mpsc::Sender<StreamEvent>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let start_time = std::time::Instant::now();
        
        // Step 1: Send analyzing progress
        context.cancellation_token.check().await?;
        let _ = event_tx.send(StreamEvent::CoordinatorThinking {
            request_id: context.request_id.clone(),
            message: "Analyzing request intent...".to_string(),
        }).await;
        
        // Step 2: Classify intent
        let classification = self.intent_router
            .classify(&request.message, &context.user_context)
            .await?;
        
        context.cancellation_token.check().await?;
        
        // Step 3: Handle out-of-scope early
        if matches!(classification.intent, Intent::OutOfScope) {
            return Err("Query is out of scope".into());
        }
        
        // Step 4: Orchestration loop
        let mut worker_results = Vec::new();
        loop {
            context.cancellation_token.check().await?;
            
            let decision = self.orchestrator
                .decide_next_step(&classification, &context.user_context, &worker_results)
                .await?;
            
            match decision {
                OrchestratorDecision::ExecuteWorker { request: worker_req } => {
                    context.cancellation_token.check().await?;
                    
                    let _ = event_tx.send(StreamEvent::CoordinatorThinking {
                        request_id: context.request_id.clone(),
                        message: format!("Executing {:?}...", worker_req.worker_type),
                    }).await;
                    
                    // Map worker to existing agent
                    let result = Self::execute_worker_via_agent(
                        state.clone(),
                        client.clone(),
                        &request.message,
                        &context,
                        worker_req,
                        event_tx.clone(),
                    ).await?;
                    
                    worker_results.push(result);
                }
                
                OrchestratorDecision::RequestContextFromUser { prompt } => {
                    let _ = event_tx.send(StreamEvent::TextChunk {
                        request_id: context.request_id.clone(),
                        chunk: prompt,
                    }).await;
                    return Ok("Context requested from user".to_string());
                }
                
                OrchestratorDecision::SendProgress { message, percent } => {
                    // Map to existing event if needed
                    let _ = event_tx.send(StreamEvent::CoordinatorThinking {
                        request_id: context.request_id.clone(),
                        message,
                    }).await;
                }
                
                OrchestratorDecision::FormatAndReturn { worker_results: results } => {
                    context.cancellation_token.check().await?;
                    
                    // Format and stream response
                    self.format_and_stream(
                        &event_tx,
                        &classification.intent,
                        &results,
                        &context,
                    ).await?;
                    
                    break;
                }
                
                OrchestratorDecision::Reject { reason, message } => {
                    let _ = event_tx.send(StreamEvent::TextChunk {
                        request_id: context.request_id.clone(),
                        chunk: message,
                    }).await;
                    return Err(reason.into());
                }
            }
        }
        
        Ok("Completed".to_string())
    }
    
    // Bridge worker execution to existing agents
    async fn execute_worker_via_agent(
        state: Arc<AppState>,
        client: ollama::Client,
        original_message: &str,
        context: &AgentContext,
        worker_request: WorkerRequest,
        event_tx: mpsc::Sender<StreamEvent>,
    ) -> Result<WorkerResponse, Box<dyn std::error::Error + Send + Sync>> {
        let start = std::time::Instant::now();
        
        // Map WorkerType to existing Task/Agent
        let result_data = match worker_request.worker_type {
            WorkerType::GetObjectTree => {
                let agent = ObjectAgent::new(
                    client,
                    context.request_id.clone(),
                    event_tx.clone(),
                );
                let result = agent.execute(
                    state,
                    original_message,
                    context,
                    &worker_request.parameters,
                ).await?;
                serde_json::json!({"result": result})
            }
            WorkerType::GetReportList => {
                let agent = DocumentAgent::new(
                    client,
                    context.request_id.clone(),
                    event_tx.clone(),
                );
                let result = agent.execute(
                    state,
                    original_message,
                    context,
                    &worker_request.parameters,
                ).await?;
                serde_json::json!({"result": result})
            }
            WorkerType::DescribeReport => {
                let agent = DescriptionAgent::new(
                    client,
                    context.request_id.clone(),
                    event_tx.clone(),
                );
                let result = agent.execute(
                    state,
                    original_message,
                    context,
                    &worker_request.parameters,
                ).await?;
                serde_json::json!({"description": result})
            }
            WorkerType::CompareReports => {
                let agent = ComparisonAgent::new(
                    client,
                    context.request_id.clone(),
                    event_tx.clone(),
                );
                let result = agent.execute(
                    state,
                    original_message,
                    context,
                    &worker_request.parameters,
                ).await?;
                serde_json::json!({"comparison": result})
            }
            WorkerType::RagQuery => {
                let agent = ChatAgent::new(
                    client,
                    context.request_id.clone(),
                    event_tx.clone(),
                );
                let result = agent.execute(
                    state,
                    original_message,
                    context,
                ).await?;
                serde_json::json!({"answer": result})
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
            },
        })
    }
}
```

### 4.4 Add Feature Flag for Gradual Rollout

**File:** `agents/master_agent.rs`
```rust
impl MasterAgent {
    async fn process_request(
        state: Arc<AppState>,
        client: ollama::Client,
        request: AgentRequest,
        context: AgentContext,
        event_tx: mpsc::Sender<StreamEvent>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Feature flag: use orchestration or legacy
        let use_orchestration = std::env::var("USE_ORCHESTRATION")
            .unwrap_or_else(|_| "false".to_string())
            .parse::<bool>()
            .unwrap_or(false);
        
        if use_orchestration {
            Self::process_request_v2(state, client, request, context, event_tx).await
        } else {
            Self::process_request_v1(state, client, request, context, event_tx).await
        }
    }
}
```

---

## Phase 5: Response Formatting Enhancement (Day 5)

### 5.1 Create ResponseFormatter

**File:** `agents/response_formatter.rs`
```rust
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;
use crate::localization::LocalizationManager;
use crate::templating::TemplateManager;
use super::types::*;
use crate::StreamEvent;

pub struct ResponseFormatter {
    _api_base: String,
    _text_model: String,
    lang_manager: Arc<LocalizationManager>,
    _template_manager: Arc<TemplateManager>,
}

impl ResponseFormatter {
    pub fn new(
        api_base: String,
        text_model: String,
        lang_manager: Arc<LocalizationManager>,
        template_manager: Arc<TemplateManager>,
    ) -> Self {
        Self {
            _api_base: api_base,
            _text_model: text_model,
            lang_manager,
            _template_manager: template_manager,
        }
    }
    
    pub async fn format_description(
        &self,
        data: &serde_json::Value,
        language: &Language,
        report_id: &str,
    ) -> Result<String> {
        // Extract description from worker result
        let description = data.get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("No description available");
        
        Ok(description.to_string())
    }
    
    pub async fn format_comparison(
        &self,
        data1: &str,
        data2: &str,
        language: &Language,
        id1: &str,
        id2: &str,
    ) -> Result<serde_json::Value> {
        // TODO: Implement actual comparison formatting
        Ok(serde_json::json!({
            "report_1": id1,
            "report_2": id2,
            "differences": []
        }))
    }
}
```

### 5.2 Implement Format and Stream Method

**File:** `agents/master_agent.rs`
```rust
impl MasterAgent {
    async fn format_and_stream(
        &self,
        event_tx: &mpsc::Sender<StreamEvent>,
        intent: &Intent,
        worker_results: &[WorkerResponse],
        context: &AgentContext,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        match intent {
            Intent::DescribeReport => {
                if let Some(result) = worker_results.first() {
                    let description = self.formatter
                        .format_description(
                            &result.data,
                            &context.user_context.language,
                            "report-id",
                        )
                        .await?;
                    
                    // Send as text chunks
                    let _ = event_tx.send(StreamEvent::TextChunk {
                        request_id: context.request_id.clone(),
                        chunk: description,
                    }).await;
                }
            }
            
            Intent::CompareReports => {
                if worker_results.len() >= 2 {
                    let comparison = self.formatter
                        .format_comparison(
                            &worker_results[0].data.to_string(),
                            &worker_results[1].data.to_string(),
                            &context.user_context.language,
                            "report-1",
                            "report-2",
                        )
                        .await?;
                    
                    let _ = event_tx.send(StreamEvent::ComparisonChunk {
                        request_id: context.request_id.clone(),
                        data: comparison,
                    }).await;
                }
            }
            
            Intent::GetObjectTree => {
                if let Some(result) = worker_results.first() {
                    let _ = event_tx.send(StreamEvent::ObjectChunk {
                        request_id: context.request_id.clone(),
                        data: result.data.clone(),
                    }).await;
                }
            }
            
            Intent::GetReportList => {
                if let Some(result) = worker_results.first() {
                    // Adapt to existing streaming format
                    let _ = event_tx.send(StreamEvent::TextChunk {
                        request_id: context.request_id.clone(),
                        chunk: serde_json::to_string_pretty(&result.data)?,
                    }).await;
                }
            }
            
            Intent::RagQuery => {
                if let Some(result) = worker_results.first() {
                    let answer = result.data.get("answer")
                        .and_then(|v| v.as_str())
                        .unwrap_or("No answer");
                    
                    let _ = event_tx.send(StreamEvent::TextChunk {
                        request_id: context.request_id.clone(),
                        chunk: answer.to_string(),
                    }).await;
                }
            }
            
            _ => {}
        }
        
        Ok(())
    }
}
```

---

## Phase 6: Testing & Validation (Day 6)

### 6.1 Update Existing Tests

**File:** `agents/master_agent.rs`
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_orchestration_mode() {
        dotenv::dotenv().ok();
        std::env::set_var("USE_ORCHESTRATION", "true");
        
        let lang_mgr = Arc::new(LocalizationManager::new());
        let tmpl_mgr = Arc::new(TemplateManager::new().unwrap());
        
        let agent = MasterAgent::new(
            "http://localhost:3050",
            lang_mgr,
            tmpl_mgr,
        );
        
        let request = AgentRequest {
            message: "show me the object tree".to_string(),
            user_id: "user_123".to_string(),
            chat_id: "chat_123".to_string(),
            language: "en".to_string(),
            object_id: None,
            prev_leaf: None,
            next_leaf: None,
            metadata: None,
        };
        
        let (_config, state) = app_init().await.unwrap();
        let mut rx = agent.handle_request_stream(state, request).await;
        
        let mut received_events = Vec::new();
        while let Some(event) = rx.recv().await {
            received_events.push(event.clone());
            if matches!(event, StreamEvent::Completed { .. }) {
                break;
            }
        }
        
        assert!(!received_events.is_empty());
        assert!(received_events.iter().any(|e| matches!(e, StreamEvent::Started { .. })));
        assert!(received_events.iter().any(|e| matches!(e, StreamEvent::Completed { .. })));
    }
    
    #[tokio::test]
    async fn test_legacy_mode() {
        std::env::set_var("USE_ORCHESTRATION", "false");
        
        // Run existing test_object_task logic
        // Should work as before
    }
    
    #[tokio::test]
    async fn test_cancellation_with_orchestration() {
        std::env::set_var("USE_ORCHESTRATION", "true");
        
        let lang_mgr = Arc::new(LocalizationManager::new());
        let tmpl_mgr = Arc::new(TemplateManager::new().unwrap());
        
        let agent = MasterAgent::new(
            "http://localhost:3050",
            lang_mgr,
            tmpl_mgr,
        );
        
        let request = AgentRequest {
            message: "compare reports (slow operation)".to_string(),
            user_id: "user_123".to_string(),
            chat_id: "chat_123".to_string(),
            language: "en".to_string(),
            object_id: None,
            prev_leaf: None,
            next_leaf: None,
            metadata: None,
        };
        
        let (_config, state) = app_init().await.unwrap();
        let mut rx = agent.handle_request_stream(state, request).await;
        
        // Receive first event, then cancel
        if let Some(_) = rx.recv().await {
            // Extract request_id and cancel
            // agent.cancel_request(&request_id);
        }
        
        // Verify cancellation event received
        while let Some(event) = rx.recv().await {
            if let StreamEvent::Cancelled { .. } = event {
                return; // Test passes
            }
        }
        
        panic!("Cancellation event not received");
    }
}
```

### 6.2 Integration Test Plan

**File:** `tests/master_agent_integration.rs`
```rust
use your_crate::agents::MasterAgent;
use your_crate::AgentRequest;
use std::sync::Arc;

#[tokio::test]
async fn test_full_workflow_orchestration() {
    // Test complete workflow:
    // 1. Intent classification
    // 2. Context validation
    // 3. Worker execution
    // 4. Response formatting
    // 5. Cancellation at different stages
}

#[tokio::test]
async fn test_concurrent_requests() {
    // Test multiple simultaneous requests
    // with cancellation and orchestration
}

#[tokio::test]
async fn test_error_recovery() {
    // Test error handling in orchestration mode
}
```

---

## Phase 7: Documentation & Cleanup (Day 7)

### 7.1 Update Documentation

**File:** `agents/README.md`
```markdown
# Agent Architecture

## Overview

The MasterAgent supports two processing modes:

1. **Legacy Mode** (`USE_ORCHESTRATION=false`): Direct task-to-agent routing
2. **Orchestration Mode** (`USE_ORCHESTRATION=true`): Intent-based multi-step workflows

## Migration Status

- ✅ Intent classification layer
- ✅ Orchestration workflow
- ✅ Cancellation integration
- ✅ Response formatting
- 🔄 Deprecation of legacy components (planned)

## Usage

```rust
// Create agent with orchestration support
let lang_mgr = Arc::new(LocalizationManager::new());
let tmpl_mgr = Arc::new(TemplateManager::new()?);

let agent = MasterAgent::new(
    "http://localhost:3050",
    lang_mgr,
    tmpl_mgr,
);

// Process request
let mut rx = agent.handle_request_stream(state, request).await;
```

## Architecture Diagram

[Include updated architecture diagrams]

## Migration Guide

See `MIGRATION_PLAN.md` for detailed migration steps.
```

### 7.2 Deprecation Plan

**File:** `agents/master_agent.rs`
```rust
// Mark legacy methods as deprecated
#[deprecated(
    since = "2.0.0",
    note = "Use process_request_v2 with orchestration enabled"
)]
async fn process_request_v1(/* ... */) -> Result<...> {
    // Existing implementation
}

// Add deprecation warnings to Task enum
#[deprecated(
    since = "2.0.0",
    note = "Use Intent enum from types module"
)]
pub enum Task {
    // ...
}
```

### 7.3 Create Migration Checklist

**File:** `MIGRATION_CHECKLIST.md`
```markdown
# Migration Checklist

## Phase 1: Foundation ✅
- [ ] Add dependencies (anyhow, tera)
- [ ] Create types module
- [ ] Set up LocalizationManager
- [ ] Set up TemplateManager

## Phase 2: Intent Layer ✅
- [ ] Create IntentRouter
- [ ] Implement intent classification
- [ ] Add intent-to-task bridge
- [ ] Write unit tests

## Phase 3: Orchestration ✅
- [ ] Create Orchestrator
- [ ] Implement workflow decisions
- [ ] Add worker context mapping
- [ ] Write unit tests

## Phase 4: Integration ✅
- [ ] Enhance AgentContext
- [ ] Add orchestration to MasterAgent
- [ ] Implement worker-to-agent bridge
- [ ] Add feature flag
- [ ] Test cancellation integration

## Phase 5: Formatting ✅
- [ ] Create ResponseFormatter
- [ ] Implement format methods
- [ ] Integrate with streaming
- [ ] Test all output formats

## Phase 6: Testing ✅
- [ ] Update existing tests
- [ ] Add integration tests
- [ ] Test concurrent scenarios
- [ ] Validate cancellation

## Phase 7: Cleanup ✅
- [ ] Update documentation
- [ ] Add deprecation warnings
- [ ] Create migration guide
- [ ] Plan legacy removal timeline
```

---

## Critical Considerations

### 1. Backwards Compatibility

**Strategy:** Feature flag ensures zero disruption
```rust
// Deploy with USE_ORCHESTRATION=false initially
// Gradual rollout by enabling per-environment
```

### 2. Cancellation Token Propagation

**Key Point:** Cancellation must work in both modes
```rust
// Check cancellation at each orchestration step
context.cancellation_token.check().await?;

// Pass token to workers
let worker_context = WorkerContext {
    cancellation_token: context.cancellation_token.clone(),
    // ...
};
```

### 3. Error Handling Consistency

**Pattern:** Use same error types across modes
```rust
// Convert orchestration errors to legacy format
match orchestration_result {
    Err(e) if e.to_string().contains("cancelled") => {
        StreamEvent::Cancelled { ... }
    }
    Err(e) => StreamEvent::Error { ... },
    Ok(_) => StreamEvent::Completed { ... },
}
```

### 4. Performance Monitoring

**Add metrics:**
```rust
struct RequestMetrics {
    intent_classification_ms: u64,
    orchestration_steps: usize,
    worker_execution_ms: u64,
    formatting_ms: u64,
}
```

### 5. Database Schema Alignment

**Note:** Ensure `AgentRequest` fields map correctly
```rust
// May need to extend metadata field:
metadata: {
    "current_report_id": "...",
    "previous_report_id": "...",
    // Legacy fields preserved
}
```

---

## Rollback Plan

### If Issues Arise:

1. **Immediate:** Set `USE_ORCHESTRATION=false`
2. **Investigate:** Check logs for orchestration errors
3. **Fix:** Apply hotfix to orchestration code
4. **Re-enable:** Gradually turn on per user/chat

### Monitoring Checklist:

- [ ] Error rates (orchestration vs legacy)
- [ ] Latency percentiles (p50, p95, p99)
- [ ] Cancellation success rate
- [ ] Worker execution times
- [ ] Memory usage patterns

---

## Timeline & Resources

| Phase | Duration | Dependencies | Risk |
|-------|----------|--------------|------|
| 1: Foundation | 1 day | None | Low |
| 2: Intent | 1 day | Phase 1 | Medium |
| 3: Orchestration | 1 day | Phase 2 | Medium |
| 4: Integration | 1.5 days | Phase 1-3 | High |
| 5: Formatting | 1 day | Phase 4 | Low |
| 6: Testing | 1 day | Phase 5 | Medium |
| 7: Cleanup | 0.5 day | Phase 6 | Low |

**Total:** 7 days (56 hours)

**Team:** 1-2 developers recommended

---

## Success Criteria

### MVP (Minimum Viable Product):
- ✅ Feature flag works correctly
- ✅ Orchestration mode handles basic intents
- ✅ Cancellation works in both modes
- ✅ No regression in legacy mode
- ✅ All existing tests pass

### Production Ready:
- ✅ 95% test coverage for new code
- ✅ Performance within 10% of legacy
- ✅ Documentation complete
- ✅ Migration guide validated
- ✅ Rollback plan tested

### Long-term:
- ✅ Legacy mode deprecated (6 months)
- ✅ All users on orchestration (12 months)
- ✅ Legacy code removed (18 months)

---

## Next Steps

1. **Review this plan** with team
2. **Set up development branch:** `feature/orchestration-migration`
3. **Start Phase 1** with foundation setup
4. **Daily standups** to track progress
5. **Code reviews** after each phase
6. **Staged deployment** to test environments

---

## Questions & Answers

**Q: Can we run both modes simultaneously?**
A: Yes, via the `USE_ORCHESTRATION` flag. Different users/sessions can use different modes.

**Q: What about existing in-flight requests during deployment?**
A: Request IDs and cancellation tokens are preserved. In-flight requests complete in their original mode.

**Q: How do we handle schema changes?**
A: Use `metadata` field for new parameters. Old requests work unchanged.

**Q: Performance impact?**
A: Intent classification adds ~50-100ms. Orchestration loop adds ~20ms per step. Acceptable for most use cases.

**Q: When can we remove legacy code?**
A: After 6-12 months of stable orchestration mode operation with zero critical issues.

---

## Appendix A: Code Snippets Library

See `examples/` directory for:
- Complete worker implementation examples
- Error handling patterns
- Testing utilities
- Performance monitoring setup

---

## Appendix B: Troubleshooting Guide

### Common Issues:

1. **"Intent classification always returns Ambiguous"**
   - Check LLM connectivity
   - Verify prompt templates
   - Review query preprocessing

2. **"Cancellation not working in orchestration mode"**
   - Verify token propagation
   - Check `context.cancellation_token.check()` calls
   - Review worker implementation

3. **"Workers not executing"**
   - Check worker-to-agent mapping
   - Verify AgentContext fields
   - Review orchestrator decision logic

---

**Document Version:** 1.0  
**Last Updated:** 2026-02-10  
**Author:** Migration Planning Team  
**Status:** Ready for Review
test