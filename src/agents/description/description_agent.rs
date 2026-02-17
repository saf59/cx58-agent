use crate::{AgentContext, AppState, StreamEvent, TaskParameters};
use rig::providers::ollama;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::agents::description::description_build::generate_description_from_image;
use crate::agents::description::description_build::{
    extract_description_content_robust, resize_image_to_bytes,
};
use crate::agents::description::description_helper::{resolve_node_data, resolve_node_storage_path};
use crate::agents::description::description_json::DescriptionData;
use crate::agents::ReportPair;
use crate::db_description::{
    get_descriptions_by_node, upsert_description, CreateImageDescription,
    ImageDescription,
};
use crate::localization::LocalizationManager;
use crate::templating::TemplateManager;

pub struct DescriptionAgent {
    client: Arc<ollama::Client>,
    context: AgentContext,
    event_tx: mpsc::Sender<StreamEvent>,
    //image_resolver: Arc<ImageUrlResolver>,
    //image_processor: Arc<ImageProcessor>,
    lang_manager: Arc<LocalizationManager>,
    template_manager: Arc<TemplateManager>,
}

impl DescriptionAgent {
    pub fn new(
        client: Arc<ollama::Client>,
        context: AgentContext,
        event_tx: mpsc::Sender<StreamEvent>,
        //image_resolver: Arc<ImageUrlResolver>,
        //image_processor: Arc<ImageProcessor>,
        lang_manager: Arc<LocalizationManager>,
        template_manager: Arc<TemplateManager>,
    ) -> Self {
        Self {
            client,
            context,
            event_tx,
            //image_resolver,
            //image_processor,
            lang_manager,
            template_manager,
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

        let result = self.execute_by_id(&state, &report_pair).await?;
        Ok(result.to_string())
    }

    /// Main execution method - processes prev and next report IDs
    /// Returns a JSON Value with array of description objects
    pub async fn execute_by_id(
        &self,
        state: &Arc<AppState>,
        reports: &ReportPair,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut descriptions: Vec<Value> = Vec::new();

        // Process prev report
        if let Some(desc) = self
            .process_single_report(state, &reports.prev, "prev")
            .await?
        {
            descriptions.push(desc);
        }

        // Process next report if provided
        if let Some(ref next_id) = reports.next {
            if let Some(desc) = self.process_single_report(state, next_id, "next").await? {
                descriptions.push(desc);
            }
        }

        // Return as array
        Ok(json!(descriptions))
    }

    /// Process a single report ID and return its description as JSON
    /// Returns None if the report cannot be processed
    async fn process_single_report(
        &self,
        state: &Arc<AppState>,
        report_id: &str,
        report_type: &str,
    ) -> Result<Option<Value>, Box<dyn std::error::Error + Send + Sync>> {
        // Parse the report_id as UUID
        let node_id = match Uuid::parse_str(report_id) {
            Ok(id) => id,
            Err(e) => {
                tracing::error!("Failed to parse report_id {} as UUID: {}", report_id, e);
                self.send_event(StreamEvent::TextChunk {
                    request_id: self.context.request_id.clone(),
                    chunk: format!("Invalid report ID: {}", report_id),
                })
                .await;
                return Ok(None);
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
        let descriptions = match get_descriptions_by_node(&state.db, &node_id).await {
            Ok(descs) => descs,
            Err(e) => {
                tracing::error!("Failed to query descriptions for node {}: {}", node_id, e);
                self.send_event(StreamEvent::TextChunk {
                    request_id: self.context.request_id.clone(),
                    chunk: format!("Database error: {}", e),
                })
                .await;
                return Ok(None);
            }
        };

        let model_name = state.ai_config.vision_model.clone();

        // Check if we have a matching description
        let existing_desc = descriptions.first();
            //.iter().find(|d| d.model_name == model_name);

        if let Some(image_desc) = existing_desc {
            tracing::info!("Found existing description for node {}", node_id);

            // Get node name/path for context
            let object_name = match resolve_node_data(&state.db, &node_id).await {
                Ok(data) => data
                    .get("src")
                    .and_then(|s| s.as_str())
                    .unwrap_or("image")
                    .to_string(),
                Err(_) => "image".to_string(),
            };

            // Convert to JSON format
            let desc_json = self
                .build_description_json_response(image_desc, &object_name, &node_id, lang_code)
                .await?;
            return Ok(Some(desc_json));
        }

        // No existing description found - need to generate one
        tracing::info!("Generating new description for node {}", node_id);

        // Get the image URL from database
        let storage_path = match resolve_node_storage_path(&state.db, &node_id).await {
            Ok(path) => path,
            Err(e) => {
                tracing::error!("Failed to get image URL for node {}: {}", node_id, e);
                self.send_event(StreamEvent::TextChunk {
                    request_id: self.context.request_id.clone(),
                    chunk: format!("Failed to get image URL: {}", e),
                })
                .await;
                return Ok(None);
            }
        };

        // Download and resize image
        self.send_event(StreamEvent::Progress {
            request_id: self.context.request_id.clone(),
            status: "downloading_image".to_string(),
            percent: 30,
            message: format!("Downloading {} image...", report_type),
        })
        .await;

        let image_url = match state
            .storage
            .generate_presigned_url(&storage_path, 120)
            .await
        {
            Ok(url) => url,
            Err(e) => {
                let msg = format!("Failed to generate presigned URL for {}: {}", &storage_path, e);
                tracing::error!(msg);
                self.send_event(StreamEvent::TextChunk {
                    request_id: self.context.request_id.clone(),
                    chunk: msg,
                }).await;
                return Ok(None);
            }
        };
        // Download image from storage
        // The URL is the public presigned URL, we need to extract the storage path
        // or use the image processor to download from the internal path
        let image_bytes = match self.download_image_from_url(&image_url).await {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::error!("Failed to download image {}: {}", image_url, e);
                self.send_event(StreamEvent::TextChunk {
                    request_id: self.context.request_id.clone(),
                    chunk: format!("Failed to download image: {}", e),
                })
                .await;
                return Ok(None);
            }
        };

        // Validate and resize image
        self.send_event(StreamEvent::Progress {
            request_id: self.context.request_id.clone(),
            status: "processing_image".to_string(),
            percent: 50,
            message: format!("Processing {} image...", report_type),
        })
        .await;

        // Resize to 1200x1200
        let resized_bytes = match resize_image_to_bytes(&image_bytes, 1200, 1200) {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::error!("Failed to resize image: {}", e);
                self.send_event(StreamEvent::TextChunk {
                    request_id: self.context.request_id.clone(),
                    chunk: format!("Failed to resize image: {}", e),
                })
                .await;
                return Ok(None);
            }
        };

        // Get system prompt from localization
        self.send_event(StreamEvent::Progress {
            request_id: self.context.request_id.clone(),
            status: "generating_description".to_string(),
            percent: 70,
            message: format!("Generating {} description...", report_type),
        })
        .await;

        let system_prompt = self
            .lang_manager
            .get_prompt(&lang, "description-system-prompt")
            .map_err(|e| format!("Failed to get system prompt: {}", e))?;

        // Get object name/path for the prompt
        let object_name = match resolve_node_data(&state.db, &node_id).await {
            Ok(data) => data
                .get("src")
                .and_then(|s| s.as_str())
                .unwrap_or("image")
                .to_string(),
            Err(_) => "image".to_string(),
        };

        // Create the prompt for description generation
        // The prompt should be in the requested language
        let description_prompt = format!("Describe the construction image: {}", object_name);

        // Generate description using LLM
        let description_text = match generate_description_from_image(
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
                tracing::error!("Failed to generate description: {}", e);
                self.send_event(StreamEvent::TextChunk {
                    request_id: self.context.request_id.clone(),
                    chunk: format!("Failed to generate description: {}", e),
                })
                .await;
                return Ok(None);
            }
        };

        // Parse the description content
        let content = match extract_description_content_robust(&description_text) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Failed to parse description content: {}", e);
                // Even if parsing fails, we can still save the raw text
                let content_str = serde_json::to_string(&description_text)?;
                let create_data = CreateImageDescription {
                    node_id,
                    model_name: state.ai_config.vision_model.clone(),
                    prompt: description_prompt.clone(),
                    description: content_str,
                    confidence: None,
                };
                if let Err(e) = upsert_description(&state.db, &create_data).await {
                    tracing::error!("Failed to save description: {}", e);
                }
                return Ok(None);
            }
        };

        // Convert back to JSON string for storage
        let description_json = serde_json::to_string(&content)?;

        // Save to database
        let create_data = CreateImageDescription {
            node_id,
            model_name: state.ai_config.vision_model.clone(),
            prompt: description_prompt.clone(),
            description: description_json.clone(),
            confidence: None,
        };

        let saved_data = match upsert_description(&state.db, &create_data).await {
            Ok(saved) => {
                tracing::info!(
                    "Saved description for node {} with model {}",
                    node_id,
                    saved.model_name
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
            .await?;
        Ok(Some(desc_json))
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
        let date_str = chrono::Utc::now().format("%Y-%m-%d").to_string();

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
            created_at: chrono::Utc::now(),
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
