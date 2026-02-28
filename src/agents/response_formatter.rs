use super::types::*;
use crate::localization::LocalizationManager;
use crate::templating::TemplateManager;
use rig::client::CompletionClient;
use rig::completion::Prompt;
use rig::providers::ollama;
use std::sync::Arc;
use tera::Context;
use crate::agents::agent_error::AgentError;

pub struct ResponseFormatter {
    client: Arc<ollama::Client>,
    model: String,
    lang_manager: Arc<LocalizationManager>,
    template_manager: Arc<TemplateManager>,
}

impl ResponseFormatter {
    pub fn new(
        client: Arc<ollama::Client>,
        model: String,
        lang_manager: Arc<LocalizationManager>,
        template_manager: Arc<TemplateManager>,
    ) -> Self {

        Self {
            client,
            model,
            lang_manager,
            template_manager,
        }
    }

    /// Format out of scope rejection message
    pub async fn format_out_of_scope(
        &self,
        language: &Language,
        original_query: &str,
    ) -> Result<String,AgentError> {
        let lang = language.to_code();
        
        // Get system prompt
        let system_prompt = self.lang_manager
            .get_prompt(lang, "formatter-system-prompt")?;

        // Build user prompt using template
        let mut ctx = Context::new();
        ctx.insert("original_query", original_query);
        ctx.insert("language", language.as_str());
        
        let prompt = self.template_manager.render(lang, "formatter-out-of-scope-prompt", ctx)?;
        
        tracing::debug!("Out of scope format prompt: {}", prompt);
        
        let agent = self.client
            .agent(&self.model)
            .preamble(&system_prompt)
            .temperature(0.5)
            .build();
        
        let response = agent.prompt(&prompt).await.map_err(|_| AgentError::Internal {
            detail: "format_out_of_scope prompt".to_string(),
        })?;
        
        tracing::info!("Formatter out of scope response:\n{}", response);
        
        Ok(response)
    }
       
}