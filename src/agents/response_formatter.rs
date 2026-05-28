use super::types::*;
use crate::agents::agent_error::AgentError;
use crate::agents::agents_helper::extract_text_from_choice;
use crate::localization::LocalizationManager;
use crate::templating::TemplateManager;
use rig::client::CompletionClient;
use rig::completion::CompletionModel;
use rig::providers::ollama;
use std::sync::Arc;
use tera::Context;

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

    /// Format out of scope rejection message.
    /// Returns the formatted text. Token usage is logged but not returned —
    /// callers that need stats can be extended later.
    pub async fn format_out_of_scope(
        &self,
        language: &Language,
        original_query: &str,
    ) -> Result<String, AgentError> {
        let lang = language.to_code();

        // Get system prompt
        let system_prompt = self.lang_manager
            .get_prompt(lang, "formatter-system-prompt")?;

        // Build user prompt using template
        let mut ctx = Context::new();
        ctx.insert("original_query", original_query);
        ctx.insert("language", language.as_str());

        let user_prompt = self.template_manager
            .render(lang, "formatter-out-of-scope-prompt", ctx)?;

        tracing::debug!("Out of scope format prompt: {}", user_prompt);

        let model = self.client.completion_model(&self.model);
        let request = model
            .completion_request(&user_prompt)
            .preamble(system_prompt)
            .temperature(0.5)
            .build();

        let response = model.completion(request).await.map_err(|e| AgentError::Internal {
            detail: format!("format_out_of_scope completion failed: {}", e),
        })?;

        let tokens = response.raw_response.prompt_eval_count.unwrap_or(0)
            + response.raw_response.eval_count.unwrap_or(0);

        let text = extract_text_from_choice(response.choice);

        tracing::info!(tokens, "Formatter out of scope response:\n{}", text);

        Ok(text)
    }
       
}