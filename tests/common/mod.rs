#![allow(unused)]

use std::sync::Arc;
use cx58_agent::agents::intent_router::IntentRouter;
use cx58_agent::agents::types::*;
use cx58_agent::localization::LocalizationManager;
use cx58_agent::templating::TemplateManager;

pub mod formatter_test_helpers;
pub mod orchestrator_test_helpers;

pub struct TestContext {
    pub intent_router: IntentRouter,
    #[allow(dead_code)]
    pub lang_manager: Arc<LocalizationManager>,
}

impl TestContext {
    pub fn new() -> Self {
        // Initialize localization
        let lang_manager = Arc::new(LocalizationManager::new());
        let template_manager = Arc::new(TemplateManager::new());
        let _ = tracing_subscriber::fmt()
            .with_test_writer()
            .try_init();
        // Use Ollama with FunctionGemma
        let api_base = std::env::var("OLLAMA_API_BASE")
            .unwrap_or_else(|_| "http://localhost:11434".to_string());
        //let model =    "functiongemma:latest".to_string();
        let model = std::env::var("OLLAMA_MODEL").unwrap_or("qwen3:14b".to_string());

        let intent_router = IntentRouter::new(
            api_base,
            model,
            lang_manager.clone(),
            template_manager,
        );

        Self {
            intent_router,
            lang_manager,
        }
    }

    pub fn create_context(
        &self,
        user_id: &str,
        language: Language,
        object_id: Option<String>,
        current_report_id: Option<String>,
        previous_report_id: Option<String>,
    ) -> UserContext {
        UserContext {
            user_id: user_id.to_string(),
            chat_id: format!("chat-{}", uuid::Uuid::now_v7()),
            language,
            object_id,
            current_report_id,
            previous_report_id,
        }
    }
}