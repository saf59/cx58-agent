use std::sync::Arc;
use rig::providers::ollama;
use rig::completion::Prompt;
use rig::prelude::CompletionClient;
use tokio::sync::mpsc;
use serde_json::json;
use crate::{AgentContext, AppState, StreamEvent, TaskParameters};

pub struct DescriptionAgent {
    client: ollama::Client,
    request_id: String,
    event_tx: mpsc::Sender<StreamEvent>,
}

impl DescriptionAgent {
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
            chunk: "Generating description...\n".to_string(),
        })
        .await;

        // Build agent prompt
        let agent_prompt = format!(
            "You are a description generator. Provide detailed description for: {}\nParameters: last={}, all={}, period={:?}, amount={:?}",
            prompt, parameters.last, parameters.all, parameters.period, parameters.amount
        );

        let agent = self
            .client
            .agent(&state.ai_config.vision_model)
            .preamble("You are a detailed description assistant. Provide comprehensive explanations in a structured format.")
            .build();

        let response = agent.prompt(&agent_prompt).await?;

        // Send text description
        self.send_event(StreamEvent::TextChunk {
            request_id: self.request_id.clone(),
            chunk: format!("Description:\n{}\n", response),
        })
        .await;

        // Send structured data
        let description_data = json!({
            "description": {
                "subject": "Requested item",
                "overview": response.chars().take(200).collect::<String>(),
                "details": {
                    "category": "General",
                    "complexity": "Medium",
                    "estimated_time": "5 minutes"
                },
                "sections": [
                    {
                        "title": "Introduction",
                        "content": "Initial overview of the subject matter."
                    },
                    {
                        "title": "Key Points",
                        "content": "Main aspects and characteristics."
                    },
                    {
                        "title": "Conclusion",
                        "content": "Summary and final thoughts."
                    }
                ]
            },
            "parameters": {
                "last": parameters.last,
                "all": parameters.all,
                "period": format!("{:?}", parameters.period),
                "amount": parameters.amount
            }
        });

        self.send_event(StreamEvent::DescriptionChunk {
            request_id: self.request_id.clone(),
            data: description_data,
        })
        .await;

        Ok(response)
    }
}