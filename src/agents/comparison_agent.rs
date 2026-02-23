// src/agents/comparison_agent.rs
//
// Receives two DescriptionData-shaped serde_json::Value objects
// (produced by DescriptionAgent) and compares them using an LLM.
//
// Input:  two Value objects with fields:
//           object, date, description, windows, doors, radiators, openings
// Output: a Value representing ComparisonData:
//           object_name, prev_date, next_date, description,
//           windows, doors, radiators, openings
//
// Prompt strategy:
//   - System prompt loaded from `locales/{lang}/comparison-system-prompt.txt`
//   - User prompt rendered from `comparison-user-prompt.tera` via TemplateManager
//   - Temperature 0.2 for consistent structured output

use crate::{AgentContext, AppState, StreamEvent, TaskParameters};
use crate::agents::agent_error::AgentError;
use crate::agents::{Language, LocalizationManager};
use crate::agents::agents_helper::clean_json_response;
use crate::templating::TemplateManager;

use rig::completion::Prompt;
use rig::prelude::CompletionClient;
use rig::providers::ollama;
use serde_json::{Value, json};
use std::error::Error;
use std::sync::Arc;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use tera::Context;
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Output type
// ---------------------------------------------------------------------------

/// Structured output of ComparisonAgent.
/// Mirrors the shape returned by the LLM and can be serialized to JSON for SSE.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonData {
    pub object_name: String,
    pub prev_date: String,
    pub next_date: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub windows: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doors: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radiators: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openings: Option<String>,
}

// ---------------------------------------------------------------------------
// Agent
// ---------------------------------------------------------------------------

pub struct ComparisonAgent {
    client: Arc<ollama::Client>,
    context: AgentContext,
    event_tx: mpsc::Sender<StreamEvent>,
    lang_manager: Arc<LocalizationManager>,
    template_manager: Arc<TemplateManager>,
}

impl ComparisonAgent {
    pub fn new(
        client: Arc<ollama::Client>,
        context: AgentContext,
        event_tx: mpsc::Sender<StreamEvent>,
        lang_manager: Arc<LocalizationManager>,
        template_manager: Arc<TemplateManager>,
    ) -> Self {
        Self { client, context, event_tx, lang_manager, template_manager }
    }

    // -----------------------------------------------------------------------
    // Public entry points
    // -----------------------------------------------------------------------

    /// Dev/test entry point with hard-coded sample descriptions.
    pub async fn execute(
        &self,
        state: Arc<AppState>,
        _parameters: &TaskParameters,
    ) -> Result<Value, Box<dyn Error + Send + Sync>> {
        let reports = json!([[
            {
                "description": "A spacious modern room with white walls.",
                "windows": "Two large double-glazed windows with white frames.",
                "doors": "Sliding glass door with white frame on the eastern wall.",
                "radiators": "Two white panel radiators mounted beneath the windows.",
                "openings": null
            },
            {
                "description": "A spacious modern room with white walls.",
                "windows": "Two large double-glazed windows with white frames.",
                "doors": "Sliding glass door with white frame on the eastern wall.",
                "radiators": "Two white panel radiators mounted beneath the windows.",
                "openings": null
            }
        ]])
        .as_array()
        .unwrap()
        .to_vec();

        self.execute_comparison(&state, reports).await
    }

    /// Main comparison path called from MasterAgent.
    ///
    /// # Arguments
    /// * `state`        – AppState with AI config.
    /// * `descriptions` – exactly two Value objects from DescriptionAgent,
    ///                    ordered [earlier, later] by date.
    pub async fn execute_comparison(
        &self,
        state: &Arc<AppState>,
        descriptions: Vec<Value>,
    ) -> Result<Value, Box<dyn Error + Send + Sync>> {
        // Validate we have at least two descriptions.
        let descriptions = descriptions
            .first()
            .and_then(|v| v.as_array())
            .ok_or_else(|| AgentError::InsufficientDescriptions { found: 0 })?;

        if descriptions.len() < 2 {
            let err = AgentError::InsufficientDescriptions { found: descriptions.len() };
            err.send_to_client(&self.event_tx, &self.context, &self.lang_manager).await;
            return Err(err.into());
        }

        tracing::info!("ComparisonAgent descriptions: {:#?}", descriptions);

        let lang = Language::from_short(&self.context.language);
        let lang_code = lang.to_code();

        // Extract object name and dates from the "object" field.
        let (object_name, date_0) = Self::extract_name_pair(
            descriptions[0]["object"].as_str().unwrap_or("Unknown object"),
        );
        let (_, date_1) = Self::extract_name_pair(
            descriptions[1]["object"].as_str().unwrap_or("Unknown object"),
        );

        // Parse dates for chronological ordering.
        let native_date_0 = Self::to_native_date(&date_0).map_err(|_| {
            AgentError::DateParseError { raw: date_0.clone() }
        })?;
        let native_date_1 = Self::to_native_date(&date_1).map_err(|_| {
            AgentError::DateParseError { raw: date_1.clone() }
        })?;

        let (prev, next) = if native_date_0 <= native_date_1 {
            (&descriptions[0], &descriptions[1])
        } else {
            (&descriptions[1], &descriptions[0])
        };
        let (prev_date, next_date) = if native_date_0 <= native_date_1 {
            (date_0, date_1)
        } else {
            (date_1, date_0)
        };

        // Load system prompt.
        let system_prompt = self
            .lang_manager
            .get_prompt(lang_code, "comparison-system-prompt")
            .map_err(|e| AgentError::internal(e))?;

        // Build Tera context for user prompt template.
        let mut ctx = Context::new();
        ctx.insert("object_name", &object_name);
        ctx.insert("prev_date", &prev_date);
        ctx.insert("next_date", &next_date);
        ctx.insert("user_message", &self.context.message);
        ctx.insert("prev_description", prev);
        ctx.insert("next_description", next);

        // Render user prompt.
        let user_prompt = self
            .template_manager
            .render(lang_code, "comparison-user-prompt", ctx)
            .map_err(|_| AgentError::TemplateRenderError {
                template: "comparison-user-prompt".to_string(),
            })?;

        tracing::info!("ComparisonAgent user_prompt:\n{}", user_prompt);

        // Call LLM.
        let agent = self
            .client
            .agent(&state.ai_config.text_model)
            .preamble(&system_prompt)
            .temperature(0.2)
            .build();

        let response = agent.prompt(&user_prompt).await?;
        tracing::info!("ComparisonAgent raw response:\n{}", response);

        // Parse structured output.
        let cleaned = clean_json_response(&response);
        tracing::debug!("ComparisonAgent cleaned JSON:\n{}", cleaned);

        let mut parsed: ComparisonData = serde_json::from_str(&cleaned).map_err(|e| {
            let err = AgentError::LlmJsonParseError { detail: e.to_string() };
            // Fire-and-forget: send the error event; we still propagate the Err below.
            // Using block_in_place avoids needing async here.
            let tx = self.event_tx.clone();
            let context = self.context.clone();
            let lm = self.lang_manager.clone();
            tokio::spawn(async move {
                err.send_to_client(&tx, &context, &lm).await;
            });
            AgentError::LlmJsonParseError { detail: e.to_string() }
        })?;

        // Override dates with the values we computed (LLM may have got them wrong).
        parsed.prev_date = prev_date.to_string();
        parsed.next_date = next_date.to_string();

        Ok(json!(parsed))
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Splits `"Root/Building/01.02.2024 12:00:00"` into
    /// `("Building", "01.02.2024 12:00:00")`.
    fn extract_name_pair(full_name: &str) -> (String, String) {
        let full_name = full_name.replace("Root/", "");
        let parts: Vec<&str> = full_name.split('/').collect();
        let report_name = parts.last().unwrap_or(&"").to_string();
        let object_name = parts[..parts.len().saturating_sub(1)].join(" - ");
        (object_name, report_name)
    }

    fn to_native_date(date: &str) -> Result<NaiveDateTime, chrono::ParseError> {
        NaiveDateTime::parse_from_str(date, "%d.%m.%Y %H:%M:%S")
    }
}
