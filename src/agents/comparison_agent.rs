use rig::providers::ollama;
use rig::completion::Prompt;
use rig::prelude::CompletionClient;
use tokio::sync::mpsc;
use serde_json::json;
use crate::agents::{StreamEvent, TaskParameters};

pub struct ComparisonAgent {
    client: ollama::Client,
    request_id: String,
    event_tx: mpsc::Sender<StreamEvent>,
}

impl ComparisonAgent {
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
        parameters: &TaskParameters,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Send initial text chunk
        self.send_event(StreamEvent::TextChunk {
            request_id: self.request_id.clone(),
            chunk: "Performing comparison analysis...\n".to_string(),
        })
        .await;

        // Build agent prompt
        let agent_prompt = format!(
            "You are a comparison analyst. Compare items based on: {}\nParameters: last={}, all={}, period={:?}, amount={:?}",
            prompt, parameters.last, parameters.all, parameters.period, parameters.amount
        );

        let agent = self
            .client
            .agent("ministral-3:14b")
            .preamble("You are a comparison specialist. Provide detailed comparative analysis with pros, cons, and recommendations.")
            .build();

        let response = agent.prompt(&agent_prompt).await?;

        // Send text description
        self.send_event(StreamEvent::TextChunk {
            request_id: self.request_id.clone(),
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
            "parameters": {
                "last": parameters.last,
                "all": parameters.all,
                "period": format!("{:?}", parameters.period),
                "amount": parameters.amount
            }
        });

        self.send_event(StreamEvent::ComparisonChunk {
            request_id: self.request_id.clone(),
            data: comparison_data,
        })
        .await;

        Ok(response)
    }
}