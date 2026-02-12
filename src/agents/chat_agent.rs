use std::sync::Arc;
use rig::providers::ollama;
use rig::completion::Prompt;
use rig::prelude::CompletionClient;
use tokio::sync::mpsc;
use crate::agents::StreamEvent;
use crate::{AgentContext, AppState};

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
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let agent = self
            .client
            .agent(&state.ai_config.text_model)
            .preamble(&format!(
                "You are a friendly chat assistant. Respond naturally in {} language.",
                self.context.language
            ))
            .build();
        self.context.cancellation_token.check().await?;
        let response = agent.prompt(&self.context.message).await?;
        self.context.cancellation_token.check().await?;
        // Send response in chunks for streaming effect
        let chunk_size = 20;
        for chunk in response.chars().collect::<Vec<_>>().chunks(chunk_size) {
            self.context.cancellation_token.check().await?;
            let chunk_str: String = chunk.iter().collect();
            log::info!("Sending text chunk:{} {}", &self.context.request_id, chunk_str);
            self.send_event(StreamEvent::TextChunk {
                request_id: self.context.request_id.clone(),
                chunk: chunk_str,
            })
            .await;

            // Small delay for streaming effect
            tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        }

        Ok(response)
    }
}