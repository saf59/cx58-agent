use cx58_agent::agents::response_formatter::ResponseFormatter;
use cx58_agent::localization::LocalizationManager;
use cx58_agent::templating::TemplateManager;
use std::sync::Arc;
use cx58_agent::helper::client;

pub struct FormatterTestContext {
    pub formatter: ResponseFormatter,
}

impl FormatterTestContext {
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

        let formatter = ResponseFormatter::new(
            client,
            model,
            lang_manager.clone(),
            template_manager,
        );
        
        Self {
            formatter,
     //       lang_manager,
        }
    }
    
    /// Create mock vision analysis data
    pub fn create_vision_data(&self) -> serde_json::Value {
        serde_json::json!({
            "objects_detected": ["concrete", "rebar", "formwork"],
            "completion_estimate": "75%",
            "observations": [
                "Foundation walls visible on north and east sides",
                "Steel reinforcement installed",
                "Formwork in place for south wall"
            ]
        })
    }
    
    /// Create mock comparison data
    pub fn create_comparison_data(&self) -> (String, String) {
        let desc1 = "Foundation work at 65% completion. Concrete walls on north side. Rebar installation in progress.".to_string();
        let desc2 = "Foundation work at 80% completion. Concrete walls on north and east sides. Steel beams installed.".to_string();
        (desc1, desc2)
    }
}