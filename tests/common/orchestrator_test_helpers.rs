use cx58_agent::agents::orchestrator::Orchestrator;
use cx58_agent::agents::types::*;
use cx58_agent::localization::LocalizationManager;
use cx58_agent::templating::TemplateManager;
use std::sync::Arc;
use cx58_agent::helper::client;

pub struct OrchestratorTestContext {
    pub orchestrator: Orchestrator,
    //pub lang_manager: Arc<LocalizationManager>,
}

impl OrchestratorTestContext {
    pub fn new() -> Self {
        let lang_manager = Arc::new(LocalizationManager::new());
        let template_manager = Arc::new(TemplateManager::new());
        let _ = tracing_subscriber::fmt()
            .with_test_writer()
            .try_init();
        let api_base = std::env::var("OLLAMA_API_BASE")
            .unwrap_or_else(|_| "http://localhost:11434".to_string());
        let model = std::env::var("OLLAMA_MODEL").unwrap_or("qwen3:14b".to_string());
        let is_local = std::env::var("OLLAMA_LOCAL").unwrap_or( "false".to_string()) == "true";
        let client = Arc::new(client(is_local));

        let orchestrator = Orchestrator::new(
            client,
            model,
            lang_manager.clone(),
            template_manager,
        );
        
        Self {
            orchestrator,
      //      lang_manager,
        }
    }
    
    /// Create a mock classification result
    pub fn create_classification(
        &self,
        intent: Intent,
        confidence: f32,
        task_params: Option<TaskParameters>,
        object_identifier: Option<String>,
        missing_context: Vec<ContextField>,
    ) -> ClassificationResult {
        ClassificationResult {
            intent,
            confidence,
            extracted_parameters: ExtractedParameters {
                task_params,
                object_identifier,
                time_reference: None,
                report_references: vec![],
            },
            missing_context,
            reasoning: "Test classification".to_string(),
        }
    }
    
    /// Create a user context
    pub fn create_context(
        &self,
        language: Language,
        object_id: Option<String>,
        current_report_id: Option<String>,
        previous_report_id: Option<String>,
    ) -> UserContext {
        UserContext {
            user_id: "test-user-123".to_string(),
            chat_id: format!("chat-{}", uuid::Uuid::now_v7()),
            language,
            object_id,
            current_report_id,
            previous_report_id,
        }
    }
    
    /// Create a mock worker response
    pub fn create_worker_response(
        &self,
        worker_type: WorkerType,
        status: WorkerStatus,
        data: serde_json::Value,
    ) -> WorkerResponse {
        WorkerResponse {
            worker_type,
            status,
            data,
            metadata: WorkerMetadata {
                execution_time_ms: 100,
                data_source: "test".to_string(),
                cache_hit: false,
            },
        }
    }
}