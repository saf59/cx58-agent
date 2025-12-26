use std::sync::Arc;
use rig::providers::ollama;
use rig::completion::Prompt;
use tokio::sync::mpsc;
use serde_json::json;
use rig::prelude::CompletionClient;
use crate::{AgentContext, AppState, StreamEvent, TaskParameters};
pub struct ObjectAgent {
    client: ollama::Client,
    request_id: String,
    event_tx: mpsc::Sender<StreamEvent>,
}

impl ObjectAgent {
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
        state:Arc<AppState>,
        prompt: &str,
        _context: &AgentContext,
        parameters: &TaskParameters,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Send initial text chunk
        self.send_event(StreamEvent::TextChunk {
            request_id: self.request_id.clone(),
            chunk: "Processing object request...\n".to_string(),
        })
        .await;

        // Build agent prompt
        let agent_prompt = format!(
            "You are an object retrieval assistant. User request: {}\nParameters: last={}, all={}, period={:?}, amount={:?}",
            prompt, parameters.last, parameters.all, parameters.period, parameters.amount
        );

        let agent = self
            .client
            .agent(&state.ai_config.text_model)
            .preamble("You are an object management system. Return structured object data in JSON format.")
            .build();

        let response = agent.prompt(&agent_prompt).await?;

        // Send text description
        self.send_event(StreamEvent::TextChunk {
            request_id: self.request_id.clone(),
            chunk: format!("Found objects:\n{}\n", response),
        })
        .await;

        // Send structured data
        let object_data = json!({
            "objects": [
                {
                    "id": "obj_001",
                    "name": "Sample Object 1",
                    "type": "document",
                    "created": "2024-12-20T10:00:00Z",
                    "status": "active"
                },
                {
                    "id": "obj_002",
                    "name": "Sample Object 2",
                    "type": "record",
                    "created": "2024-12-22T14:30:00Z",
                    "status": "pending"
                }
            ],
            "total": 2,
            "parameters": {
                "last": parameters.last,
                "all": parameters.all,
                "period": format!("{:?}", parameters.period),
                "amount": parameters.amount
            }
        });

        self.send_event(StreamEvent::ObjectChunk {
            request_id: self.request_id.clone(),
            data: object_data,
        })
        .await;

        Ok(response)
    }
}