use tera::{Context, Tera};
use std::collections::HashMap;
use crate::agents::agent_error::AgentError;

const TEMPLATE_FILES: &[(&str, &str)] = &[
    ("intent-router-user-prompt", "intent_router_user.tera"),
    ("orchestrator-user-prompt", "orchestrator_user.tera"),
    ("comparison-user-prompt", "comparison_user.tera"),
    ("descriptor-user-prompt", "descriptor_user.tera"),
    ("formatter-description-prompt", "formatter_description.tera"),
    ("formatter-comparison-prompt", "formatter_comparison.tera"),
    ("formatter-out-of-scope-prompt", "formatter_out_of_scope.tera"),
];

pub struct TemplateManager {
    engines: HashMap<String, Tera>, // lang -> ready Tera with all templates loaded
}
impl Default for TemplateManager {
    fn default() -> Self {
        Self::new()
    }
}
impl TemplateManager {
    pub fn new() -> Self {
        let mut manager = Self { engines: HashMap::new() };
        manager.load_templates("en");
        manager.load_templates("de");
        manager
    }
    fn load_templates(&mut self, lang: &str) {
        let mut tera = Tera::default();

        for (template_id, filename) in TEMPLATE_FILES {
            if let Ok(content) = Self::load_template_file(lang, filename) {
                tera.add_raw_template(template_id, &content)
                    .unwrap_or_else(|e| tracing::error!(
                        "Failed to add template {} for {}: {}", template_id, lang, e
                    ));
            }
        }

        self.engines.insert(lang.to_string(), tera);
    }
    fn load_template_file(lang: &str, filename: &str) -> Result<String,AgentError> {
        let content = match (lang, filename) {
            ("en", "intent_router_user.tera") => include_str!("../locales/en/prompts/intent_router_user.tera"),
            ("en", "orchestrator_user.tera") => include_str!("../locales/en/prompts/orchestrator_user.tera"),
            ("en", "comparison_user.tera") => include_str!("../locales/en/prompts/comparison_user.tera"),
            ("en", "descriptor_user.tera") => include_str!("../locales/en/prompts/descriptor_user.tera"),
            ("en", "formatter_description.tera") => include_str!("../locales/en/prompts/formatter_description.tera"),
            ("en", "formatter_comparison.tera") => include_str!("../locales/en/prompts/formatter_comparison.tera"),
            ("en", "formatter_out_of_scope.tera") => include_str!("../locales/en/prompts/formatter_out_of_scope.tera"),

            ("de", "intent_router_user.tera") => include_str!("../locales/de/prompts/intent_router_user.tera"),
            ("de", "orchestrator_user.tera") => include_str!("../locales/de/prompts/orchestrator_user.tera"),
            ("de", "comparison_user.tera") => include_str!("../locales/de/prompts/comparison_user.tera"),
            ("de", "descriptor_user.tera") => include_str!("../locales/de/prompts/descriptor_user.tera"),
            ("de", "formatter_description.tera") => include_str!("../locales/de/prompts/formatter_description.tera"),
            ("de", "formatter_comparison.tera") => include_str!("../locales/de/prompts/formatter_comparison.tera"),
            ("de", "formatter_out_of_scope.tera") => include_str!("../locales/de/prompts/formatter_out_of_scope.tera"),

            _ =>  {
                let err = AgentError::internal(format!("Unknown template: {} for language {}", filename, lang));
                tracing::error!("{}", err);
                return Err(err);
            },
        };

        Ok(content.to_string())
    }
    pub fn render(&self, lang: &str, template_id: &str, context: Context) -> Result<String,AgentError> {
        let tera = self.engines
            .get(lang)
            .or_else(|| self.engines.get("en"))
            .ok_or_else(|| AgentError::internal(format!("No templates for language {}", lang)))?;

        let text = tera.render(template_id, &context)
            .map_err(|e| AgentError::TemplateRenderError { template: e.to_string() })?;
        Ok(text)
    }
}