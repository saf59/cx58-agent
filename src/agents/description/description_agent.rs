use crate::agents::agent_error::AgentError;
use crate::agents::description::description_build::generate_description_from_image;
use crate::agents::description::description_build::{
    extract_description_content_robust, resize_image_to_bytes,
};
use crate::agents::description::description_helper::{
    resolve_node_full_name, resolve_node_storage_path,
};
use crate::agents::description::description_json::DescriptionData;
use crate::agents::{Language, ReportPair};
use crate::db_description::{
    CreateImageDescription, ImageDescription, get_descriptions_by_node, upsert_description,
};
use crate::localization::LocalizationManager;
use crate::templating::TemplateManager;
use crate::{AgentContext, AppState, StreamEvent};
use rig::providers::ollama;
use serde_json::{Value, json};
use std::sync::Arc;
use tera::Context;
use tokio::sync::mpsc;
use uuid::Uuid;

pub struct DescriptionAgent {
    client: Arc<ollama::Client>,
    context: AgentContext,
    event_tx: mpsc::Sender<StreamEvent>,
    lang_manager: Arc<LocalizationManager>,
    template_manager: Arc<TemplateManager>,
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
            template_manager,
        }
    }

    async fn send_event(&self, event: StreamEvent) {
        let _ = self.event_tx.send(event).await;
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
        if let Some((desc, prev_tokens)) = self
            .process_single_report(state, &reports.prev, "prev")
            .await?
        {
            descriptions.push(desc);
            if let Some(n) = prev_tokens {
                if n > 0 {
                    calls += 1;
                    total_tokens += n;
                };
            }
        }

        // Process next report if provided
        if let Some(ref next_id) = reports.next {
            if let Some((desc, next_tokens)) =
                self.process_single_report(state, next_id, "next").await?
            {
                descriptions.push(desc);
                if let Some(n) = next_tokens {
                    if n > 0 {
                        calls += 1;
                        total_tokens += n;
                    };
                };
            }
        }
        if descriptions.is_empty() {
            tracing::error!(
                "DescriptionAgent: no descriptions generated for reports {:?}",
                reports
            );
            let err = AgentError::InsufficientDescriptions { found: 0 };
            err.send_to_client(&self.event_tx, &self.context, &self.lang_manager)
                .await;
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
                let err = AgentError::InvalidUuid {
                    raw: report_id.to_string(),
                };
                return Err(err);
            }
        };

        // Get the language code from context
        let lang = Language::from_short(&self.context.language);
        let lang_code = lang.to_code();
        tracing::info!(
            raw_language = %self.context.language,
            lang_code = %lang_code,
            node_id = %node_id,
            "DescriptionAgent language resolved"
        );
        let model = state.ai_config.vision_model.clone();
        // First, try to get existing description from database
        let descriptions =
            match get_descriptions_by_node(&state.db, &node_id, &model, lang_code).await {
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
                tracing::error!("{}", err_msg);
                let err = AgentError::internal(err_msg.clone());
                return Err(err);
            }
        };
        let report_type = object_name.split('/').last().unwrap_or(report_type);

        // Check if we have a matching description
        let existing_desc = descriptions.iter().find(|description| {
            serde_json::from_str::<
                crate::agents::description::description_json::DescriptionContent,
            >(&description.description)
            .is_ok_and(|content| content.uses_current_prompt())
        });

        if let Some(image_desc) = existing_desc {
            tracing::info!(
                "Found existing description for '{}', node_id: {}",
                report_type,
                node_id
            );

            // Convert to JSON format
            let desc_json = self
                .build_description_json_response(
                    state,
                    image_desc,
                    &object_name,
                    &node_id,
                    lang_code,
                )
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
                tracing::error!("{}", err_msg);
                let err = AgentError::internal(err_msg.clone());
                return Err(err);
            }
        };
        let executing_msg = self.lang_manager.get_msg_with_arg(
            lang_code,
            "progress-downloading-image",
            "report_type",
            &report_type,
        );
        // Download and resize image
        self.send_event(StreamEvent::Progress {
            request_id: self.context.request_id.clone(),
            status: "downloading_image".to_string(),
            percent: 30,
            message: executing_msg,
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
                tracing::error!("{}", err_msg);
                let err = AgentError::internal(err_msg.clone());
                return Err(err);
            }
        };

        let executing_msg = self.lang_manager.get_msg_with_arg(
            lang_code,
            "progress-processing-image",
            "report_type",
            &report_type,
        );

        // Validate and resize image
        self.send_event(StreamEvent::Progress {
            request_id: self.context.request_id.clone(),
            status: "processing_image".to_string(),
            percent: 50,
            message: executing_msg,
        })
        .await;

        // Resize to 1200x1200
        let resized_bytes = match tokio::task::spawn_blocking(move || {
            resize_image_to_bytes(&image_bytes, 1200, 1200)
        })
        .await
        {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(e)) => {
                let err_msg = format!("Failed to resize image for node {}: {}", node_id, e);
                tracing::error!("{}", err_msg);
                return Err(AgentError::internal(err_msg));
            }
            Err(e) => {
                // JoinError — spawn_blocking task panicked
                let err_msg = format!("Image resize task panicked for node {}: {}", node_id, e);
                tracing::error!("{}", err_msg);
                return Err(AgentError::internal(err_msg));
            }
        };

        let executing_msg = self.lang_manager.get_msg_with_arg(
            lang_code,
            "progress-generating-description",
            "report_type",
            &report_type,
        );

        // Get system prompt from localization
        self.send_event(StreamEvent::Progress {
            request_id: self.context.request_id.clone(),
            status: "generating_description".to_string(),
            percent: 70,
            message: executing_msg,
        })
        .await;

        let system_prompt = self
            .lang_manager
            .get_prompt(lang_code, "description-system-prompt")
            .map_err(|e| format!("Failed to get system prompt: {}", e))
            .map_err(|e| {
                let err = AgentError::internal(e);
                tracing::error!(
                    "Failed to get system prompt for language {}: {}",
                    lang_code,
                    err
                );
                err
            })?;
        /*        tracing::debug!(
                    "Using system prompt for language {}: {}",
                    lang_code,
                    system_prompt
                );
        */
        // Create the prompt for description generation
        let mut ctx = Context::new();
        ctx.insert("object_name", &object_name);
        let description_prompt = self
            .template_manager
            .render(lang_code, "descriptor-user-prompt", ctx)
            .map_err(|_| AgentError::TemplateRenderError {
                template: "descriptor-user-prompt".to_string(),
            })?;

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
                let err_msg = self.lang_manager.get_msg_with_arg(
                    lang_code,
                    "progress-generate-err",
                    "report_type",
                    &report_type,
                );
                tracing::error!("{} {}", err_msg, e);
                let err = AgentError::internal(err_msg.clone());
                return Err(err);
            }
        };

        // Parse the description content
        let mut content = match extract_description_content_robust(&description_text) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(
                    node_id = %node_id,
                    error = %e,
                    "DescriptionAgent: failed to parse LLM response — not caching malformed data"
                );
                let err_msg = self.lang_manager.get_msg_with_arg(
                    lang_code,
                    "progress-description-parse-warning",
                    "report_type",
                    &report_type,
                );
                self.send_event(StreamEvent::Progress {
                    request_id: self.context.request_id.clone(),
                    status: "warning".to_string(),
                    percent: 0,
                    message: err_msg,
                })
                .await;
                return Err(AgentError::internal(format!(
                    "LLM response parse failed for node {}: {}",
                    node_id, e
                )));
            }
        };
        content.mark_current_prompt();

        // Convert back to JSON string for storage
        let description_json =
            serde_json::to_string(&content).map_err(|e| AgentError::internal(e))?;

        // Save to database
        let create_data = CreateImageDescription {
            node_id,
            model_name: state.ai_config.vision_model.clone(),
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
            .build_description_json_response(state, &saved_data, &object_name, &node_id, lang_code)
            .await
            .map_err(|e| AgentError::internal(e))?;
        Ok(Some((desc_json, tokens)))
    }

    /// Build JSON response from ImageDescription
    async fn build_description_json_response(
        &self,
        state: &Arc<AppState>,
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
        let date_str = image_desc
            .created_at
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        let mut node_data =
            crate::agents::description::description_helper::resolve_node_data(&state.db, node_id)
                .await?;
        if let Some(object) = node_data.as_object_mut() {
            crate::storage::set_storage_url(state.clone(), object, node_id).await;
        }
        let thumbnail_url = node_data
            .get("thumbnail_url")
            .and_then(Value::as_str)
            .map(str::to_string);
        let full_url = node_data
            .get("url")
            .and_then(Value::as_str)
            .map(str::to_string);

        let desc_data = DescriptionData {
            object: object_str,
            object_id: *node_id,
            date: date_str,
            date_id: image_desc.node_id.clone(),
            thumbnail_url,
            full_url,
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
