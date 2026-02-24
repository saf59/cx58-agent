use crate::agents::ReportPair;
use crate::agents::agent_error::AgentError;
use crate::agents::description::description_build::generate_description_from_image;
use crate::agents::description::description_build::{
    extract_description_content_robust, resize_image_to_bytes,
};
use crate::agents::description::description_helper::{
    resolve_node_full_name, resolve_node_storage_path,
};
use crate::agents::description::description_json::DescriptionData;
use crate::db_description::{
    CreateImageDescription, ImageDescription, get_descriptions_by_node, upsert_description,
};
use crate::localization::LocalizationManager;
use crate::templating::TemplateManager;
use crate::{AgentContext, AppState, StreamEvent, TaskParameters};
use rig::providers::ollama;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

pub struct DescriptionAgent {
    client: Arc<ollama::Client>,
    context: AgentContext,
    event_tx: mpsc::Sender<StreamEvent>,
    lang_manager: Arc<LocalizationManager>,
    _template_manager: Arc<TemplateManager>,
}

impl DescriptionAgent {
    pub fn new(
        client: Arc<ollama::Client>,
        context: AgentContext,
        event_tx: mpsc::Sender<StreamEvent>,
        lang_manager: Arc<LocalizationManager>,
        template_manager: Arc<TemplateManager>,
    ) -> Self {
        Self {
            client,
            context,
            event_tx,
            lang_manager,
            _template_manager: template_manager,
        }
    }

    async fn send_event(&self, event: StreamEvent) {
        let _ = self.event_tx.send(event).await;
    }

    pub async fn execute(
        &self,
        state: Arc<AppState>,
        _parameters: &TaskParameters,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let report_id = self.context.prev_leaf.clone().unwrap_or_default();
        let report_pair = ReportPair {
            prev: report_id.clone(),
            next: self.context.next_leaf.clone(),
        };

        let (result, _tokens, _calls) = self.execute_by_id(&state, &report_pair).await?;
        Ok(result.to_string())
    }

    /// Main execution method - processes prev and next report IDs
    /// Returns a JSON Value with array of description objects
    pub async fn execute_by_id(
        &self,
        state: &Arc<AppState>,
        reports: &ReportPair,
    ) -> Result<(Value, Option<u64>, u32), AgentError> {
        let mut descriptions: Vec<Value> = Vec::new();
        let mut total_tokens: u64 = 0;
        let mut calls: u32 = 0;

        // Process prev report
        if let Some((desc, tokens)) = self
            .process_single_report(state, &reports.prev, "prev")
            .await?
        {
            descriptions.push(desc);
            if let Some(n) = tokens {
                if n > 0 {
                    calls += 1;
                    total_tokens += n;
                };
            }
        }

        // Process next report if provided
        if let Some(ref next_id) = reports.next {
            if let Some((desc, tokens)) = self.process_single_report(state, next_id, "next").await?
            {
                descriptions.push(desc);
                if let Some(n) = tokens {
                    if n > 0 {
                        calls += 1;
                        total_tokens += n;
                    };
                };
            }
        }
        if descriptions.is_empty() {
            tracing::warn!(
                "DescriptionAgent: no descriptions generated for reports {:?}",
                reports
            );
            let err = AgentError::InsufficientDescriptions { found: 0 };
            err.send_to_client(&self.event_tx, &self.context, &self.lang_manager)
                .await;
            tracing::error!("DescriptionAgent: insufficient descriptions for reports {:?}, found 0", reports);
            return Err(err);
        }

        let tokens_result = Some(total_tokens);

        Ok((json!(descriptions), tokens_result, calls))
    }

    /// Process a single report ID and return its description as JSON
    /// Returns None if the report cannot be processed
    async fn process_single_report(
        &self,
        state: &Arc<AppState>,
        report_id: &str,
        report_type: &str,
    ) -> Result<Option<(Value, Option<u64>)>, AgentError> {
        // Parse the report_id as UUID
        let node_id = match Uuid::parse_str(report_id) {
            Ok(id) => id,
            Err(e) => {
                tracing::error!(report_id = %report_id, error = %e, "Failed to parse report_id as UUID");
                let err = AgentError::InvalidUuid{    
                    raw: report_id.to_string(),
                };       
                return Err(err);
            }
        };

        // Get the language code from context
        let lang = self.context.language.clone();
        let lang_code = if lang.to_lowercase() == "de" {
            "de"
        } else {
            "en"
        };

        // First, try to get existing description from database
        let descriptions = match get_descriptions_by_node(&state.db, &node_id, lang_code).await {
            Ok(descriptions) => descriptions,
            Err(e) => {
                let err_msg = format!("Database query failed for node {}: {}", node_id, e);
                tracing::error!("{}", err_msg);
                let err = AgentError::internal(err_msg.clone());
                return Err(err);
            }
        };

        let object_name = match resolve_node_full_name(&state.db, &node_id).await {
            Ok(object_name) => object_name,
            Err(e) => {
                let err_msg = format!("Failed to resolve full name for node {}: {}", node_id, e);
                tracing::error!("{}",err_msg);
                let err = AgentError::internal(err_msg.clone());
                return Err(err);
            }
        };
        let report_type = object_name.split('/').last().unwrap_or(report_type);

        // Check if we have a matching description
        let existing_desc = descriptions.first();
        //.iter().find(|d| d.model_name == model_name);

        if let Some(image_desc) = existing_desc {
            tracing::info!(
                "Found existing description for '{}', node_id: {}",
                report_type,
                node_id
            );

            // Convert to JSON format
            let desc_json = self
                .build_description_json_response(image_desc, &object_name, &node_id, lang_code)
                .await
                .map_err(|e| AgentError::internal(e))?;
            return Ok(Some((desc_json, Some(0))));
        }

        // No existing description found - need to generate one
        tracing::info!(
            "Generating new description for {}, node_id {}",
            report_type,
            node_id
        );

        // Get the image URL from database
        let storage_path = match resolve_node_storage_path(&state.db, &node_id).await {
            Ok(path) => path,
            Err(e) => {
                let err_msg = format!("Failed to resolve storage path for node {}: {}", node_id, e);
                tracing::error!("{}",err_msg);
                let err = AgentError::internal(err_msg.clone());
                return Err(err);
            }
        };

        // Download and resize image
        self.send_event(StreamEvent::Progress {
            request_id: self.context.request_id.clone(),
            status: "downloading_image".to_string(),
            percent: 30,
            message: format!("Downloading '{}' image...", report_type),
        })
        .await;

        let image_url = match state
            .storage
            .generate_presigned_url(&storage_path, 120)
            .await
        {
            Ok(url) => url,
            Err(e) => {
                let msg = format!(
                    "Failed to generate presigned URL for {}: {}",
                    &storage_path, e
                );
                tracing::error!(msg);
                let err = AgentError::internal(msg.clone());
                return Err(err);
            }
        };
        // Download image from storage
        // The URL is the public presigned URL, we need to extract the storage path
        // or use the image processor to download from the internal path
        let image_bytes = match self.download_image_from_url(&image_url).await {
            Ok(bytes) => bytes,
            Err(e) => {
                let err_msg = format!("Failed to download image from URL {}: {}", image_url, e);
                tracing::error!("{}",err_msg);
                let err = AgentError::internal(err_msg.clone());
                return Err(err);
            }
        };

        // Validate and resize image
        self.send_event(StreamEvent::Progress {
            request_id: self.context.request_id.clone(),
            status: "processing_image".to_string(),
            percent: 50,
            message: format!("Processing '{}' image...", report_type),
        })
        .await;

        // Resize to 1200x1200
        let resized_bytes = match resize_image_to_bytes(&image_bytes, 1200, 1200) {
            Ok(bytes) => bytes,
            Err(e) => {
                let err_msg = format!("Failed to resize image for node {}: {}", node_id, e);
                tracing::error!("{}",err_msg);
                let err = AgentError::internal(err_msg.clone());
                return Err(err);
            }
        };

        // Get system prompt from localization
        self.send_event(StreamEvent::Progress {
            request_id: self.context.request_id.clone(),
            status: "generating_description".to_string(),
            percent: 70,
            message: format!("Generating '{}' description...", report_type),
        })
        .await;

        let system_prompt = self
            .lang_manager
            .get_prompt(&lang, "description-system-prompt")
            .map_err(|e| format!("Failed to get system prompt: {}", e))
            .map_err(|e| AgentError::internal(e))?;

        // Create the prompt for description generation
        // The prompt should be in the requested language
        let description_prompt = format!("Describe the construction image: {}", object_name);

        // Generate description using LLM
        let (description_text, tokens) = match generate_description_from_image(
            &self.client,
            &state.ai_config.vision_model,
            &resized_bytes,
            &description_prompt,
            &system_prompt,
        )
        .await
        {
            Ok(text) => text,
            Err(e) => {
                let err_msg = format!("Failed to generate description for node {}: {}", node_id, e);
                tracing::error!("{}",err_msg);
                let err = AgentError::internal(err_msg.clone());
                return Err(err);
            }
        };

        // Parse the description content
        let content = match extract_description_content_robust(&description_text) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Failed to parse description content: {}", e);
                // Even if parsing fails, we can still save the raw text
                let content_str = serde_json::to_string(&description_text)
                    .map_err(|e| AgentError::internal(e))?;
                let create_data = CreateImageDescription {
                    node_id,
                    model_name: state.ai_config.vision_model.clone(),
                    prompt: description_prompt.clone(),
                    description: content_str,
                    confidence: None,
                };
                if let Err(e) = upsert_description(&state.db, &create_data, lang_code).await {
                    tracing::error!("Failed to save description: {}", e);
                }
                return Ok(None);
            }
        };

        // Convert back to JSON string for storage
        let description_json =
            serde_json::to_string(&content).map_err(|e| AgentError::internal(e))?;

        // Save to database
        let create_data = CreateImageDescription {
            node_id,
            model_name: state.ai_config.vision_model.clone(),
            prompt: description_prompt.clone(),
            description: description_json.clone(),
            confidence: None,
        };

        let saved_data = match upsert_description(&state.db, &create_data, lang_code).await {
            Ok(saved) => {
                tracing::info!(
                    "Saved description for node {} with model {} on {}",
                    node_id,
                    saved.model_name,
                    lang_code
                );
                saved
            }
            Err(e) => {
                tracing::error!("Failed to save description: {}", e);
                // Continue anyway - we have the description in memory
                create_data.into()
            }
        };

        // Build and return the JSON response
        let desc_json = self
            .build_description_json_response(&saved_data, &object_name, &node_id, lang_code)
            .await
            .map_err(|e| AgentError::internal(e))?;
        Ok(Some((desc_json, tokens)))
    }

    /// Build JSON response from ImageDescription
    async fn build_description_json_response(
        &self,
        image_desc: &ImageDescription,
        object_name: &str,
        node_id: &Uuid,
        _lang_code: &str,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        // Parse the description content
        let content: crate::agents::description::description_json::DescriptionContent =
            serde_json::from_str(&image_desc.description)?;

        // Build DescriptionData
        let object_str = object_name.to_string();
        let date_str = image_desc.created_at.format("%Y-%m-%d %H:%M:%S").to_string();

        let desc_data = DescriptionData {
            object: object_str,
            object_id: *node_id,
            date: date_str,
            date_id: *node_id,
            description: content.description,
            windows: content.windows,
            doors: content.doors,
            radiators: content.radiators,
            openings: content.openings,
            model_name: image_desc.model_name.clone(),
            confidence: image_desc.confidence,
            created_at: image_desc.created_at,
        };

        // Convert to JSON
        let json_data = serde_json::to_value(desc_data)?;
        Ok(json_data)
    }

    /// Download image from presigned URL
    async fn download_image_from_url(
        &self,
        url: &str,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        // The URL is a presigned S3 URL
        // We can use reqwest to download from it
        let response = reqwest::get(url)
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()).into());
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read response: {}", e))?;

        Ok(bytes.to_vec())
    }
}
