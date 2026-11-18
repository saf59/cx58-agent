use crate::agents::agent_error::AgentError;
use crate::agents::description::description_build::generate_description_from_image;
use crate::agents::description::description_build::{
    extract_description_content_robust, resize_image_to_bytes,
};
use crate::agents::description::description_helper::{
    resolve_image_node, resolve_node_full_name, resolve_node_storage_path,
};
use crate::agents::description::description_json::DescriptionData;
use crate::agents::runtime_policy::{
    IMAGE_PROCESS_SLOTS, IMAGE_PROCESS_TIMEOUT, STORAGE_TIMEOUT, VISION_LLM_SLOTS,
    VISION_LLM_TIMEOUT, acquire_slot, run_cancellable,
};
use crate::agents::singleflight::acquire_singleflight;
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

const MAX_DESCRIPTION_IMAGE_BYTES: usize = 20 * 1024 * 1024;

/// Formats a stored UTC report timestamp in the report's Berlin wall-clock time.
fn format_report_date(updated_at: chrono::NaiveDateTime) -> String {
    use chrono::{TimeZone, Utc};
    use chrono_tz::Europe::Berlin;

    let utc = Utc.from_utc_datetime(&updated_at);
    utc.with_timezone(&Berlin)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

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
        if let Some((desc, prev_tokens, report_calls)) = self
            .process_single_report(state, &reports.prev, "prev")
            .await?
        {
            descriptions.push(desc);
            calls += report_calls;
            total_tokens += prev_tokens.unwrap_or(0);
        }

        // Process next report if provided
        if let Some(ref next_id) = reports.next
            && let Some((desc, next_tokens, report_calls)) =
                self.process_single_report(state, next_id, "next").await?
        {
            descriptions.push(desc);
            calls += report_calls;
            total_tokens += next_tokens.unwrap_or(0);
        }
        if descriptions.is_empty() {
            tracing::error!(
                "DescriptionAgent: no descriptions generated for reports {:?}",
                reports
            );
            return Err(AgentError::InsufficientDescriptions { found: 0 });
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
    ) -> Result<Option<(Value, Option<u64>, u32)>, AgentError> {
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
        let report_type = object_name.split('/').next_back().unwrap_or(report_type);

        // Check if we have a matching description
        let existing_desc = current_description(&descriptions);

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
                .map_err(AgentError::internal)?;
            return Ok(Some((desc_json, None, 0)));
        }

        let flight_key = format!("description:{node_id}:{model}:{lang_code}");
        let _flight = run_cancellable(
            &self.context.cancellation_token,
            VISION_LLM_TIMEOUT,
            "description_singleflight",
            async { Ok(acquire_singleflight(&flight_key).await) },
        )
        .await?;
        let refreshed = get_descriptions_by_node(&state.db, &node_id, &model, lang_code)
            .await
            .map_err(AgentError::internal)?;
        if let Some(image_desc) = current_description(&refreshed) {
            let desc_json = self
                .build_description_json_response(
                    state,
                    image_desc,
                    &object_name,
                    &node_id,
                    lang_code,
                )
                .await
                .map_err(AgentError::internal)?;
            return Ok(Some((desc_json, None, 0)));
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
            report_type,
        );
        // Download and resize image
        self.send_event(StreamEvent::Progress {
            request_id: self.context.request_id.clone(),
            status: "downloading_image".to_string(),
            percent: 30,
            message: executing_msg,
        })
        .await;

        let download_result = run_cancellable(
            &self.context.cancellation_token,
            STORAGE_TIMEOUT,
            "image_download",
            async {
                state
                    .storage
                    .download_image(&storage_path)
                    .await
                    .map_err(AgentError::internal)
            },
        )
        .await;
        let image_bytes = match download_result {
            Ok(bytes) => bytes,
            Err(e) => {
                let err_msg = format!("Failed to download image for node {}: {}", node_id, e);
                tracing::error!(node_id = %node_id, error = %e, "Failed to download image");
                let err = AgentError::internal(err_msg.clone());
                return Err(err);
            }
        };
        if image_bytes.len() > MAX_DESCRIPTION_IMAGE_BYTES {
            return Err(AgentError::internal(format!(
                "Image for node {node_id} exceeds the processing byte limit"
            )));
        }

        let executing_msg = self.lang_manager.get_msg_with_arg(
            lang_code,
            "progress-processing-image",
            "report_type",
            report_type,
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
        let _image_slot = acquire_slot(
            &self.context.cancellation_token,
            &IMAGE_PROCESS_SLOTS,
            IMAGE_PROCESS_TIMEOUT,
            "image_processing_queue",
        )
        .await?;
        let resized_bytes = run_cancellable(
            &self.context.cancellation_token,
            IMAGE_PROCESS_TIMEOUT,
            "image_processing",
            async move {
                match tokio::task::spawn_blocking(move || {
                    resize_image_to_bytes(&image_bytes, 1200, 1200)
                })
                .await
                {
                    Ok(Ok(bytes)) => Ok(bytes),
                    Ok(Err(error)) => Err(AgentError::internal(format!(
                        "Failed to resize image for node {node_id}: {error}"
                    ))),
                    Err(error) => Err(AgentError::internal(format!(
                        "Image resize task panicked for node {node_id}: {error}"
                    ))),
                }
            },
        )
        .await?;

        let executing_msg = self.lang_manager.get_msg_with_arg(
            lang_code,
            "progress-generating-description",
            "report_type",
            report_type,
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
        let generation_future = generate_description_from_image(
            &self.client,
            &state.ai_config.vision_model,
            &resized_bytes,
            &description_prompt,
            &system_prompt,
        );
        let _vision_slot = acquire_slot(
            &self.context.cancellation_token,
            &VISION_LLM_SLOTS,
            VISION_LLM_TIMEOUT,
            "vision_queue",
        )
        .await?;
        let generation_result = run_cancellable(
            &self.context.cancellation_token,
            VISION_LLM_TIMEOUT,
            "vision",
            generation_future,
        )
        .await;
        let (description_text, tokens, calls) = match generation_result {
            Ok(text) => text,
            Err(e) => {
                tracing::error!(%report_type, error = %e, "Photo description generation failed");
                return Err(e);
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
                    report_type,
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
        content.normalize_language(lang_code);
        content.mark_current_prompt();

        // Convert back to JSON string for storage
        let description_json = serde_json::to_string(&content).map_err(AgentError::internal)?;

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
            .map_err(AgentError::internal)?;
        Ok(Some((desc_json, tokens, calls)))
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
        let resolved_node = resolve_image_node(&state.db, node_id).await?;
        let date_str = format_report_date(resolved_node.updated_at);
        let mut node_data = resolved_node.data;
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
            date_id: image_desc.node_id,
            thumbnail_url,
            full_url,
            power_outlets: content.power_outlet_count(),
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
}

/// Finds a cached description generated with the current schema and prompt contract.
fn current_description(descriptions: &[ImageDescription]) -> Option<&ImageDescription> {
    descriptions.iter().find(|description| {
        serde_json::from_str::<crate::agents::description::description_json::DescriptionContent>(
            &description.description,
        )
        .is_ok_and(|content| content.uses_current_prompt())
    })
}

#[cfg(test)]
mod tests {
    use super::format_report_date;
    use chrono::NaiveDate;

    /// Builds a UTC timestamp for report-date formatting tests.
    fn utc_datetime(year: i32, month: u32, day: u32, hour: u32) -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_hms_opt(hour, 0, 0)
            .unwrap()
    }

    /// Verifies conversion from UTC to Berlin summer wall-clock time.
    #[test]
    fn report_date_uses_berlin_summer_time() {
        assert_eq!(
            format_report_date(utc_datetime(2026, 7, 7, 16)),
            "2026-07-07 18:00:00"
        );
    }

    /// Verifies conversion from UTC to Berlin winter wall-clock time.
    #[test]
    fn report_date_uses_berlin_winter_time() {
        assert_eq!(
            format_report_date(utc_datetime(2026, 1, 7, 17)),
            "2026-01-07 18:00:00"
        );
    }
}
