use crate::agents::agent_error::AgentError;
use crate::agents::agents_helper::extract_text_from_choice;
use crate::agents::{Language, LocalizationManager, StreamEvent};
use crate::{AgentContext, AppState};
use rig::completion::CompletionModel;
use rig::prelude::CompletionClient;
use rig::providers::ollama;
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct ChatAgent {
    client: Arc<ollama::Client>,
    context: AgentContext,
    lang_manager: Arc<LocalizationManager>,
    event_tx: mpsc::Sender<StreamEvent>,
}

/// Interacts directly with the ollama text model.
// Must have  context that say "I'm only targeting the cx58".
impl ChatAgent {
    pub fn new(
        client: Arc<ollama::Client>,
        context: AgentContext,
        lang_manager: Arc<LocalizationManager>,
        event_tx: mpsc::Sender<StreamEvent>,
    ) -> Self {
        Self {
            client,
            context,
            lang_manager,
            event_tx,
        }
    }

    async fn send_event(&self, event: StreamEvent) {
        let _ = self.event_tx.send(event).await;
    }

    pub async fn execute(
        &self,
        state: Arc<AppState>,
        message: &str,
    ) -> Result<(String, Option<u64>), AgentError> {
        self.context.cancellation_token.check().await?;

        let lang = Language::from_short(&self.context.language);
        let lang_code = lang.to_code();
        // Load system prompt.
        let system_prompt = self
            .lang_manager
            .get_prompt(lang_code, "chat-system-prompt")
            .map_err(|e| AgentError::internal(e))?;

        let model = self.client.completion_model(&state.ai_config.text_model);
        let request = model
            .completion_request(message)
            .preamble(system_prompt)
            .temperature(0.5)
            .build();
        let response = model.completion(request).await?;

        let choice = response.choice.clone();
        let text = extract_text_from_choice(choice);

        if text.is_empty() {
            let err_msg = format!(
                "ChatAgent LLM returned no text content. Response: {:?}",
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

        // Send response in chunks for streaming effect
        let chunk_size = 20;
        for chunk in text.chars().collect::<Vec<_>>().chunks(chunk_size) {
            self.context.cancellation_token.check().await?;
            let chunk_str: String = chunk.iter().collect();
            tracing::info!(
                "Sending text chunk:{} {}",
                &self.context.request_id,
                chunk_str
            );
            self.send_event(StreamEvent::TextChunk {
                request_id: self.context.request_id.clone(),
                chunk: chunk_str,
            })
            .await;

            // Small delay for streaming effect
            tokio::time::sleep(tokio::time::Duration::from_millis(4)).await;
        }

        Ok((text, tokens))
    }
}
