use std::error::Error;
use std::sync::Arc;
use rig::providers::ollama;
use rig::completion::Prompt;
use rig::prelude::CompletionClient;
use tokio::sync::mpsc;
use serde_json::json;
use crate::{AgentContext, AppState, StreamEvent, TaskParameters};

pub struct DescriptionAgent {
    client: Arc<ollama::Client>,
    context: AgentContext,
    event_tx: mpsc::Sender<StreamEvent>,
}
/// Retrieves a description of a media file.
/// If there is no description, retrieves the media file
/// and calls the modal model to create the description.
/// Stores the description in a separate table.
impl DescriptionAgent {
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
        state: Arc<AppState>,
        parameters: &TaskParameters,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let report_id = self.context.prev_leaf.clone().unwrap_or_default();

        self.execute_by_id(&state, &report_id).await?
    }

    pub async fn execute_by_id(&self, state: &Arc<AppState>, report_id: &str) -> Result<Result<String, Box<dyn Error + Send + Sync>>, Box<dyn Error + Send + Sync>> {

        // Send initial text chunk
        self.send_event(StreamEvent::TextChunk {
            request_id: self.context.request_id.clone(),
            chunk: "Generating description...\n".to_string(),
        })
            .await;

        // Build agent prompt
        let agent_prompt = format!("You are a description generator. Provide detailed description for {:?}", report_id);

        let agent = self
            .client
            .agent(&state.ai_config.vision_model)
            .preamble("You are a detailed description assistant. Provide comprehensive explanations in a structured format.")
            .build();

        let response = agent.prompt(&agent_prompt).await?;

        // Send text description
        self.send_event(StreamEvent::TextChunk {
            request_id: self.context.request_id.clone(),
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
        });

        self.send_event(StreamEvent::Description {
            request_id: self.context.request_id.clone(),
            data: description_data,
        })
            .await;

        Ok(Ok(response))
    }
}