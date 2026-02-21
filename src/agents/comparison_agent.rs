use crate::{AgentContext, AppState, StreamEvent, TaskParameters};
use rig::completion::Prompt;
use rig::prelude::CompletionClient;
use rig::providers::ollama;
use serde_json::{Value, json};
use std::error::Error;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tera::Context;
use tokio::sync::mpsc;
use crate::agents::agents_helper::clean_json_response;
use crate::agents::{Language, LocalizationManager};
use crate::templating::TemplateManager;
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


/// # ComparisonAgent
///
/// Receives two `DescriptionData`-shaped `serde_json::Value` objects
/// (produced by `DescriptionAgent`) and compares them using an LLM.
///
/// ## Input
/// Two `Value` objects with fields:
///   object, date, description, windows, doors, radiators, openings
///
/// ## Output
/// A `Value` representing `ComparisonData`:
///   object_name, prev_date, next_date, description, windows, doors, radiators, openings
///
/// ## Prompt strategy
/// - System prompt loaded from `comparison_system.txt` via `LocalizationManager`
/// - User prompt rendered from `comparison_user_prompt.tera` via `TemplateManager`
/// - Temperature 0.2 for consistent structured output
pub struct ComparisonAgent {
    client: Arc<ollama::Client>,
    context: AgentContext,
    event_tx: mpsc::Sender<StreamEvent>,
    lang_manager: Arc<LocalizationManager>,
    template_manager: Arc<TemplateManager>,
}
/// Loads two media file descriptions
/// Compares them
impl ComparisonAgent {
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

    pub async fn execute(
        &self,
        state: Arc<AppState>,
        _parameters: &TaskParameters,
    ) -> Result<Value, Box<dyn Error + Send + Sync>> {
        // Send structured data
        let reports = json!([
{
  "description": "A spacious modern room with white walls, wooden flooring, and a textured concrete ceiling. Natural light streams through multiple windows and a sliding door, creating a bright and airy atmosphere.",
  "windows": "Two large double-glazed windows with white frames on the left wall, and a smaller fixed window with white frames on the middle wall",
  "doors": "Sliding glass door with white frame on the eastern wall, leading to a balcony, with a sheer curtain partially drawn",
  "radiators": "Two white panel radiators mounted beneath the windows",
  "openings": null
},
{
  "description": "A spacious modern room with white walls, wooden flooring, and a textured concrete ceiling. Natural light streams through multiple windows and a sliding door, creating a bright and airy atmosphere.",
  "windows": "Two large double-glazed windows with white frames on the left wall, and a smaller fixed window with white frames on the middle wall",
  "doors": "Sliding glass door with white frame on the eastern wall, leading to a balcony, with a sheer curtain partially drawn",
  "radiators": "Two white panel radiators mounted beneath the windows",
  "openings": null
}
,
        ])
        .as_array()
        .unwrap()
        .to_vec();
        self.execute_comparison(&state, reports).await
    }

    /// Execute comparison from two DescriptionAgent results.
    ///
    /// # Arguments
    /// * `state`        - AppState with AI config
    /// * `descriptions` - Exactly two `Value` objects from DescriptionAgent,
    ///                    ordered [earlier, later] by date.
    ///
    /// # Returns
    /// A `Value` containing `ComparisonData` fields ready for SSE streaming.
    pub async fn execute_comparison(
        &self,
        state: &Arc<AppState>,
        descriptions: Vec<Value>,
       //lang: &str
    ) -> Result<Value, Box<dyn Error + Send + Sync>> {
        if descriptions.len() < 2 {
            return Err(format!(
                "ComparisonAgent requires exactly 2 DescribeReport results, got {}",
                descriptions.len()
            ).into());
        }
        let lang = Language::from_short(&self.context.language);
        let lang = lang.to_code();

        let (object_name, date_0) = Self::extract_name_pair(
            descriptions[0]["object"]
                .as_str()
                .unwrap_or("Unknown object")
        );
        let (_, date_1) = Self::extract_name_pair(
            descriptions[1]["object"]
                .as_str()
                .unwrap_or("Unknown object")
        );

        let (prev,next) = if date_0 <= date_1 {
            (&descriptions[0], &descriptions[1])
        } else {
            (&descriptions[1], &descriptions[0])
        };
        let (prev_date, next_date) = if date_0 <= date_1 {
            (date_0, date_1)
        } else {
            (date_1, date_0)
        };

        self.send_event(StreamEvent::TextChunk {
            request_id: self.context.request_id.clone(),
            chunk: format!(
                "Comparing inspections: {} vs {}...\n",
                prev_date, next_date
            ),
        }).await;

        // Load system prompt from localization (comparison_system.txt)
        let system_prompt = self.lang_manager
            .get_prompt(lang, "comparison-system-prompt")?;

        // Build Tera context for user prompt template
        let mut ctx = Context::new();
        ctx.insert("object_name", &object_name);
        ctx.insert("prev_date", &prev_date);
        ctx.insert("next_date", &next_date);
        ctx.insert("user_message", &self.context.message);
        ctx.insert("prev_description", prev);
        ctx.insert("next_description", next);

        // Render user prompt from comparison_user_prompt.tera
        let user_prompt = self.template_manager
            .render(lang, "comparison-user-prompt", ctx)?;

        tracing::info!("ComparisonAgent - user_prompt:\n{}", user_prompt);

        let agent = self.client
            .agent(&state.ai_config.text_model)
            .preamble(&system_prompt)
            .temperature(0.2)
            .build();

        let response = agent.prompt(&user_prompt).await?;

        tracing::info!("ComparisonAgent raw response:\n{}", response);

        let cleaned = clean_json_response(&response);

        tracing::debug!("ComparisonAgent cleaned JSON:\n{}", cleaned);

        let parsed: ComparisonData = serde_json::from_str(&cleaned)
            .map_err(|e| format!(
                "ComparisonAgent: failed to parse LLM response: {}\nCleaned: {}",
                e, cleaned
            ))?;

        Ok(json!(parsed))
    }
    fn extract_name_pair(full_name: &str) -> (String, String) {
        let full_name = full_name.replace("Root/","");
        let parts: Vec<&str> = full_name.split('/').collect();

        let report_name = parts.last().unwrap_or(&"").to_string();
        let object_name = parts[..parts.len().saturating_sub(1)].join(" - ");

        (object_name, report_name)
    }
}
