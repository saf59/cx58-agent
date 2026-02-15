use tera::{Context, Tera};
use anyhow::Result;
use std::collections::HashMap;

const TEMPLATE_FILES: &[(&str, &str)] = &[
    ("intent-router-user-prompt", "intent_router_user.tera"),
    ("orchestrator-user-prompt", "orchestrator_user.tera"),
    ("formatter-description-prompt", "formatter_description.tera"),
    ("formatter-comparison-prompt", "formatter_comparison.tera"),
    ("formatter-out-of-scope-prompt", "formatter_out_of_scope.tera"),
];

pub struct TemplateManager {
    tera_templates: HashMap<String, HashMap<String, String>>, // lang -> template_id -> content
}
impl Default for TemplateManager {
    fn default() -> Self {
        Self::new()
    }
}
impl TemplateManager {

    fn load_templates(&mut self, lang: &str) {
        let mut lang_templates = HashMap::new();

        for (template_id, filename) in TEMPLATE_FILES {
            match Self::load_template_file(lang, filename) {
                Ok(content) => {
                    lang_templates.insert(template_id.to_string(), content);
                }
                Err(e) => {
                    tracing::error!("Failed to load template {} for {}: {}", filename, lang, e);
                }
            }
        }

        self.tera_templates.insert(lang.to_string(), lang_templates);
    }


    //pub fn new(lang_manager: Arc<crate::localization::LocalizationManager>) -> Self {
    pub fn new() -> Self {
        let mut manager = Self {
            //lang_manager,
            tera_templates: HashMap::new(),
        };

        // Load templates for both languages
        manager.load_templates("en");
        manager.load_templates("de");

        manager
    }

    fn load_template_file(lang: &str, filename: &str) -> Result<String> {
        let content = match (lang, filename) {
            ("en", "intent_router_user.tera") => include_str!("../locales/en/prompts/intent_router_user.tera"),
            ("en", "orchestrator_user.tera") => include_str!("../locales/en/prompts/orchestrator_user.tera"),
            ("en", "formatter_description.tera") => include_str!("../locales/en/prompts/formatter_description.tera"),
            ("en", "formatter_comparison.tera") => include_str!("../locales/en/prompts/formatter_comparison.tera"),
            ("en", "formatter_out_of_scope.tera") => include_str!("../locales/en/prompts/formatter_out_of_scope.tera"),

            ("de", "intent_router_user.tera") => include_str!("../locales/de/prompts/intent_router_user.tera"),
            ("de", "orchestrator_user.tera") => include_str!("../locales/de/prompts/orchestrator_user.tera"),
            ("de", "formatter_description.tera") => include_str!("../locales/de/prompts/formatter_description.tera"),
            ("de", "formatter_comparison.tera") => include_str!("../locales/de/prompts/formatter_comparison.tera"),
            ("de", "formatter_out_of_scope.tera") => include_str!("../locales/de/prompts/formatter_out_of_scope.tera"),

            _ => return Err(anyhow::anyhow!("Unknown template: {} for language {}", filename, lang)),
        };

        Ok(content.to_string())
    }
    pub fn render(&self, lang: &str, template_id: &str, context: Context) -> Result<String> {
        let template = self.tera_templates
            .get(lang)
            .or_else(|| self.tera_templates.get("en"))
            .and_then(|lang_templates| lang_templates.get(template_id))
            .ok_or_else(|| anyhow::anyhow!("Template {} not found for language {}", template_id, lang))?;

        let text = Tera::one_off(template, &context, false)?;
        Ok(text)
    }
}