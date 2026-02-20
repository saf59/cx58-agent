use std::error::Error;
use crate::{AgentContext, AppState, StreamEvent, TaskParameters};
use rig::completion::Prompt;
use rig::prelude::CompletionClient;
use rig::providers::ollama;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct ComparisonAgent {
    client: Arc<ollama::Client>,
    context: AgentContext,
    event_tx: mpsc::Sender<StreamEvent>,
}
/// Loads two media file descriptions
/// Compares them
/// Sends the raw result to the chat
/// Requires JSON format as input
/// On the chat side, the format is parsed into hardcoded format
/// Then it is converted to fixed HTML
impl ComparisonAgent {
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
        _parameters: &TaskParameters,
    ) -> Result<String, Box<dyn Error + Send + Sync>> {
        let report_id_1 = self.context.prev_leaf.clone().unwrap_or_default();
        let report_id_2 = self.context.next_leaf.clone().unwrap_or_default();
        self.execute_comparision(&state, &report_id_1, &report_id_2).await?
    }

    pub async fn execute_comparision(&self, state: &Arc<AppState>, report_id_1: &str, report_id_2: &str) -> Result<Result<String, Box<dyn Error + Send + Sync>>, Box<dyn Error + Send + Sync>> {
        // Send initial text chunk
        self.send_event(StreamEvent::TextChunk {
            request_id: self.context.request_id.clone(),
            chunk: "Performing comparison analysis...\n".to_string(),
        })
            .await;

        // Build agent prompt
        let agent_prompt = format!(
            "You are a comparison analyst. Compare items based on: {}\nParameters: prev={}, next={}",
            self.context.message, report_id_1, report_id_2
        );

        let agent = self
            .client
            .agent(&state.ai_config.text_model)
            .preamble("You are a comparison specialist. Provide detailed comparative analysis with pros, cons, and recommendations.")
            .build();

        let response = agent.prompt(&agent_prompt).await?;

        // Send text description
        self.send_event(StreamEvent::TextChunk {
            request_id: self.context.request_id.clone(),
            chunk: format!("Comparison results:\n{}\n", response),
        })
            .await;

        // Send structured data
        let comparison_data = json!({
            "comparison": {
                "items_compared": 2,
                "analysis": {
                    "item_a": {
                        "name": "Item A",
                        "score": 85,
                        "pros": ["Fast processing", "Easy to use", "Cost-effective"],
                        "cons": ["Limited features", "Basic interface"]
                    },
                    "item_b": {
                        "name": "Item B",
                        "score": 78,
                        "pros": ["Advanced features", "Customizable", "Good support"],
                        "cons": ["Higher cost", "Steeper learning curve"]
                    }
                },
                "differences": [
                    {
                        "category": "Performance",
                        "item_a": "95%",
                        "item_b": "88%",
                        "winner": "Item A"
                    },
                    {
                        "category": "Features",
                        "item_a": "Basic",
                        "item_b": "Advanced",
                        "winner": "Item B"
                    },
                    {
                        "category": "Cost",
                        "item_a": "$50",
                        "item_b": "$120",
                        "winner": "Item A"
                    }
                ],
                "recommendation": {
                    "best_for_budget": "Item A",
                    "best_for_features": "Item B",
                    "overall_winner": "Item A",
                    "reasoning": "Better value for most use cases"
                }
            },
        });

        self.send_event(StreamEvent::Comparison {
            request_id: self.context.request_id.clone(),
            data: comparison_data,
        })
            .await;

        Ok(Ok(response))
    }
}
