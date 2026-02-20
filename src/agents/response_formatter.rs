use super::types::*;
use crate::localization::LocalizationManager;
use crate::templating::TemplateManager;
use anyhow::Result;
use rig::client::CompletionClient;
use rig::completion::Prompt;
use rig::providers::ollama;
use std::sync::Arc;
use tera::Context;
use crate::agents::agents_helper::clean_json_response;

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
    
    /// Format vision analysis into natural description
    pub async fn format_description(
        &self,
        worker_data: &serde_json::Value,
        language: &Language,
        report_id: &str,
    ) -> Result<String> {
        let lang = language.to_code();
        
        // Get system prompt
        let system_prompt = self.lang_manager
            .get_prompt(lang, "formatter-system-prompt")?;
        
        // Build user prompt using template
        let mut ctx = Context::new();
        ctx.insert("raw_analysis", &serde_json::to_string_pretty(worker_data)?);
        ctx.insert("report_id", report_id);
        ctx.insert("report_date", "2024-01-30"); // Should come from metadata
        ctx.insert("object_name", "Construction Site"); // Should come from metadata
        ctx.insert("photo_count", &5); // Should come from metadata
        ctx.insert("language", language.as_str());
        
        let prompt = self.template_manager.render(lang, "formatter-description-prompt", ctx)?;
        
        tracing::debug!("Description format prompt: {}", prompt);
        
        let agent = self.client
            .agent(&self.model)
            .preamble(&system_prompt)
            .temperature(0.4)
            .build();
        
        let response = agent.prompt(&prompt).await?;
        
        tracing::info!("Formatter description response:\n{}", response);
        
        Ok(response)
    }
    
    /// Format comparison between two reports
    pub async fn format_comparison(
        &self,
        report1_desc: &str,
        report2_desc: &str,
        language: &Language,
        report_id_1: &str,
        report_id_2: &str,
    ) -> Result<serde_json::Value> {
        let lang = language.to_code();
        
        // Get system prompt
        let system_prompt = self.lang_manager
            .get_prompt(lang, "formatter-system-prompt")?;
        
        // Build user prompt using template
        let mut ctx = Context::new();
        ctx.insert("report_id_1", report_id_1);
        ctx.insert("report_date_1", "2024-01-23"); // Should come from metadata
        ctx.insert("report_1_description", report1_desc);
        ctx.insert("report_id_2", report_id_2);
        ctx.insert("report_date_2", "2024-01-30"); // Should come from metadata
        ctx.insert("report_2_description", report2_desc);
        ctx.insert("time_difference", "7 days"); // Should be calculated
        ctx.insert("language", language.as_str());
        
        let prompt = self.template_manager.render(lang, "formatter-comparison-prompt", ctx)?;
        
        tracing::debug!("Comparison format prompt: {}", prompt);
        
        let agent = self.client
            .agent(&self.model)
            .preamble(&system_prompt)
            .temperature(0.3)
            .build();
        
        let response = agent.prompt(&prompt).await?;
        
        tracing::info!("Formatter comparison response:\n{}", response);
        
        let cleaned = clean_json_response(&response);

        let error_msg = self.lang_manager.get_msg(lang, "error-comparison-parse");
        let comparison: serde_json::Value = serde_json::from_str(&cleaned)
            .map_err(|e| anyhow::anyhow!(
                "{}\nCleaned: {}\nOriginal: {}\nError: {}",
                error_msg, cleaned, response, e
            ))?;

        Ok(comparison)
    }
    
    /// Format out of scope rejection message
    pub async fn format_out_of_scope(
        &self,
        language: &Language,
        original_query: &str,
    ) -> Result<String> {
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
        
        let response = agent.prompt(&prompt).await?;
        
        tracing::info!("Formatter out of scope response:\n{}", response);
        
        Ok(response)
    }
       
}