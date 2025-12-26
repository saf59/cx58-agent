use std::sync::Arc;
use rig::providers::ollama;
use rig::completion::Prompt;
use rig::prelude::CompletionClient;
use tokio::sync::mpsc;
use serde_json::json;
use crate::{AgentContext, AppState, StreamEvent, TaskParameters};

pub struct DocumentAgent {
    client: ollama::Client,
    request_id: String,
    event_tx: mpsc::Sender<StreamEvent>,
}

impl DocumentAgent {
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
        context: &AgentContext,
        parameters: &TaskParameters,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Send initial text chunk
        self.send_event(StreamEvent::TextChunk {
            request_id: self.request_id.clone(),
            chunk: "Retrieving documents...\n".to_string(),
        })
        .await;

        // Build agent prompt
        let agent_prompt = format!(
            "You are a document retrieval assistant. User request: {}\nParameters: last={}, all={}, period={:?}, amount={:?}",
            prompt, parameters.last, parameters.all, parameters.period, parameters.amount
        );

        let agent = self
            .client
            .agent("ministral-3:14b")
            .preamble("You are a document management system. Return structured document data in JSON format.")
            .build();

        let response = agent.prompt(&agent_prompt).await?;

        // Send text description
        self.send_event(StreamEvent::TextChunk {
            request_id: self.request_id.clone(),
            chunk: format!("Document results:\n{}\n", response),
        })
        .await;

        // Send structured data
        let document_data = json!({
            "documents": [
                {
                    "id": "doc_001",
                    "title": "Q4 Report",
                    "type": "report",
                    "size": "2.4 MB",
                    "created": "2024-12-15T09:00:00Z",
                    "author": "John Doe",
                    "status": "published"
                },
                {
                    "id": "doc_002",
                    "title": "Annual Review",
                    "type": "document",
                    "size": "1.8 MB",
                    "created": "2024-12-18T11:30:00Z",
                    "author": "Jane Smith",
                    "status": "draft"
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

        self.send_event(StreamEvent::DocumentChunk {
            request_id: self.request_id.clone(),
            data: document_data,
        })
        .await;

        Ok(response)
    }
}