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

use crate::agents::agent_error::AgentError;
use crate::agents::agents_helper::{clean_json_response, extract_text_from_choice};
use crate::agents::{Language, LocalizationManager};
use crate::templating::TemplateManager;
use crate::{AgentContext, AppState, StreamEvent};

use chrono::NaiveDateTime;
use rig::completion::CompletionModel;
use rig::prelude::CompletionClient;
use rig::providers::ollama;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::sync::Arc;
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
        Self {
            client,
            context,
            event_tx,
            lang_manager,
            template_manager,
        }
    }

    // -----------------------------------------------------------------------
    // Public entry points
    // -----------------------------------------------------------------------

    /// Main comparison path called from MasterAgent.
    ///
    /// # Arguments
    /// * `state`        – AppState with AI config.
    /// * `descriptions` – expects a Vec<Value> where the first element is an array of two objects
    ///                    ordered [earlier, later] by date.
    pub async fn execute_comparison(
        &self,
        state: &Arc<AppState>,
        descriptions: Vec<Value>,
    ) -> Result<(Value, Option<u64>), AgentError> {
        // first row in the descriptions is expected to be an array of two objects
        // Validate we have at least two descriptions.
        let descriptions = descriptions
            .first()
            .and_then(|v| v.as_array())
            .ok_or_else(|| AgentError::InsufficientDescriptions { found: 0 })?;

        if descriptions.len() < 2 {
            let err = AgentError::InsufficientDescriptions {
                found: descriptions.len(),
            };
            tracing::error!(
                found = descriptions.len(),
                request_id = %self.context.request_id,
                "ComparisonAgent: insufficient descriptions, need 2"
            );
            err.send_to_client(&self.event_tx, &self.context, &self.lang_manager)
                .await;
            return Err(err.into());
        }

        tracing::debug!("ComparisonAgent descriptions: {:#?}", descriptions);

        let lang = Language::from_short(&self.context.language);
        let lang_code = lang.to_code();

        // Extract object name and dates from the "object" field.
        let (object_name, date_0) = Self::extract_name_pair(
            descriptions[0]["object"]
                .as_str()
                .unwrap_or("Unknown object"),
        );
        let (_, date_1) = Self::extract_name_pair(
            descriptions[1]["object"]
                .as_str()
                .unwrap_or("Unknown object"),
        );

        // Parse dates for chronological ordering.
        let native_date_0 =
            Self::str_to_native_date(&date_0).map_err(|_| AgentError::DateParseError {
                raw: date_0.clone(),
            })?;
        let native_date_1 =
            Self::str_to_native_date(&date_1).map_err(|_| AgentError::DateParseError {
                raw: date_1.clone(),
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

        let model_name = &state.ai_config.text_model;
        let prev_id = prev["date_id"].as_str().unwrap_or("");
        let next_id = next["date_id"].as_str().unwrap_or("");
        // --- Cache lookup ---
        if let Some(cached) = self.get_cached_comparison(
            &state.db, prev_id, next_id, lang_code, model_name
        ).await? {
            tracing::info!(prev_id, next_id, "Comparison cache HIT");
            return Ok((cached, None));
        }

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
        let model = self.client.completion_model(&model_name.clone());
        let request = model
            .completion_request(&user_prompt)
            .preamble(system_prompt)
            .temperature(0.2)
            .build();
        let response = model.completion(request).await?;

        let choice = response.choice.clone();
        let text = extract_text_from_choice(choice);

        if text.is_empty() {
            let err_msg = format!(
                "ComparisonAgent LLM returned no text content. Response: {:?}",
                response.choice
            );
            tracing::error!("{}", err_msg);
            let err = AgentError::internal(err_msg.clone());
            return Err(err);
        }

        let tokens = Some(
            response.raw_response.prompt_eval_count.unwrap_or(0)
                + response.raw_response.eval_count.unwrap_or(0),
        );

        tracing::info!("ComparisonAgent raw response:\n{}", &text);

        // Parse structured output.
        let cleaned = clean_json_response(&text);
        tracing::debug!("ComparisonAgent cleaned JSON:\n{}", cleaned);

        let mut parsed: ComparisonData = serde_json::from_str(&cleaned).map_err(|e| {
            tracing::error!(
                "ComparisonAgent: failed to parse LLM response: {}\nCleaned: {}",
                e,
                cleaned
            );
            AgentError::LlmJsonParseError {
                detail: e.to_string(),
            }
        })?;

        // Override dates with the values we computed (LLM may have got them wrong).
        parsed.prev_date = prev_date.to_string();
        parsed.next_date = next_date.to_string();
        let result_json = json!(parsed);
        // --- Cache store ---
        self.save_comparison(
            &state.db, prev_id, next_id, lang_code, model_name, &result_json
        ).await?;

        Ok((result_json, tokens))
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

    fn str_to_native_date(date: &str) -> Result<NaiveDateTime, chrono::ParseError> {
        NaiveDateTime::parse_from_str(date, "%d.%m.%Y %H:%M:%S")
    }
    async fn get_cached_comparison(
        &self,
        pool: &PgPool,
        prev_id: &str,
        next_id: &str,
        lang: &str,
        model: &str,
    ) -> Result<Option<Value>, AgentError> {
        let row = sqlx::query!(
            r#"
            SELECT data
            FROM comparison
            WHERE prev_id   = $1
              AND next_id   = $2
              AND lang      = $3
              AND model     = $4
            "#,
            prev_id, next_id, lang, model
        )
            .fetch_optional(pool)
            .await?;

        Ok(row.map(|r| r.data))
    }

    async fn save_comparison(
        &self,
        pool: &PgPool,
        prev_id: &str,
        next_id: &str,
        lang: &str,
        model: &str,
        data: &Value,
    ) -> Result<(), AgentError> {
/*        let expires_at = chrono::Utc::now()
            + chrono::Duration::days(COMPARISON_CACHE_TTL_DAYS);
*/
        sqlx::query!(
            r#"
            INSERT INTO comparison
                (prev_id, next_id, lang, model, data)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (prev_id, next_id, lang, model)
                DO UPDATE SET data = EXCLUDED.data
            "#,
            prev_id, next_id, lang, model, data
        )
            .execute(pool)
            .await?;

        Ok(())
    }

}
