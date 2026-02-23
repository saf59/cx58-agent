use std::sync::Arc;
use rig::providers::ollama;
use rig::completion::{AssistantContent, CompletionModel};
use rig::prelude::CompletionClient;
use tokio::sync::mpsc;
use crate::agents::StreamEvent;
use crate::{AgentContext, AppState};
use crate::agents::agent_error::AgentError;

pub struct ChatAgent {
    client: Arc<ollama::Client>,
    context: AgentContext,
    event_tx: mpsc::Sender<StreamEvent>,
}

/// Interacts directly with the ollama text model.
// RAG will be added later.
// Must have  context that say "I'm only targeting the cx58".
impl ChatAgent {
    pub fn new(
        client: Arc<ollama::Client>,
        context: AgentContext,
        event_tx: mpsc::Sender<StreamEvent>,
    ) -> Self {
        Self {
            client,
            context,
            event_tx,
        }
    }

    async fn send_event(&self, event: StreamEvent) {
        let _ = self.event_tx.send(event).await;
    }

    pub async fn execute(
        &self,
        state:Arc<AppState>,
        message: &str,
    ) -> Result<(String,Option<u64>), AgentError> {

        self.context.cancellation_token.check().await?;

        let system_prompt = format!(
            "You are a helpful assistant. Respond in {} language. Only answer the question based on the provided context. If you don't know the answer, say you don't know.",
            self.context.language
        );

        let model = self.client.completion_model(&state.ai_config.text_model);
        let request = model
            .completion_request(message)
            .preamble(system_prompt)
            .temperature(0.2)
            .build();
        let response = model.completion(request).await?;

        let text = response.choice
            .iter()
            .filter_map(|c| match c {
                AssistantContent::Text(t) => Some(t.text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");

        if text.is_empty() {
            return Err(AgentError::internal("Orchestrator LLM returned no text content").into());
        }

        let tokens = Some(
            response.raw_response.prompt_eval_count.unwrap_or(0)
                + response.raw_response.eval_count.unwrap_or(0)
        );

        // Send response in chunks for streaming effect
        let chunk_size = 20;
        for chunk in text.chars().collect::<Vec<_>>().chunks(chunk_size) {
            self.context.cancellation_token.check().await?;
            let chunk_str: String = chunk.iter().collect();
            tracing::info!("Sending text chunk:{} {}", &self.context.request_id, chunk_str);
            self.send_event(StreamEvent::TextChunk {
                request_id: self.context.request_id.clone(),
                chunk: chunk_str,
            })
            .await;

            // Small delay for streaming effect
            tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        }

        Ok((text,tokens))
    }
}