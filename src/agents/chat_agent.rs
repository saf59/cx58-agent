use crate::events::StreamEvent;
use rig::providers::ollama;
use rig::completion::Prompt;
use tokio::sync::mpsc;

pub struct ChatAgent {
    client: ollama::Client,
    request_id: String,
    event_tx: mpsc::Sender<StreamEvent>,
}

impl ChatAgent {
    pub fn new(
        client: ollama::Client,
        request_id: String,
        event_tx: mpsc::Sender<StreamEvent>,
    ) -> Self {
        Self {
            client,
            request_id,
            event_tx,
        }
    }

    async fn send_event(&self, event: StreamEvent) {
        let _ = self.event_tx.send(event).await;
    }

    pub async fn execute(
        &self,
        prompt: &str,
        language: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let agent = self
            .client
            .agent("ministral-3:14b")
            .preamble(&format!(
                "You are a friendly chat assistant. Respond naturally in {} language.",
                language
            ))
            .build();

        let response = agent.prompt(prompt).await?;

        // Send response in chunks for streaming effect
        let chunk_size = 20;
        for chunk in response.chars().collect::<Vec<_>>().chunks(chunk_size) {
            let chunk_str: String = chunk.iter().collect();
            
            self.send_event(StreamEvent::TextChunk {
                request_id: self.request_id.clone(),
                chunk: chunk_str,
            })
            .await;

            // Small delay for streaming effect
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }

        Ok(response)
    }
}