use crate::agents::cancellation::RequestManager;
use crate::agents::{ChatAgent, ComparisonAgent, ContextParser, DescriptionAgent, DocumentAgent, ObjectAgent, Task, TaskDetector};
use crate::{AgentContext, AgentRequest, AiConfig, AppState, StreamEvent};
use rig::providers::ollama;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

// ============================================================================
// MASTER AGENT
// ============================================================================

pub struct MasterAgent {
    client: Arc<ollama::Client>,
    config:AiConfig,
    request_manager: Arc<RequestManager>,
}
/// Parameter parsing calls other models.
/// Butler only.
/// Parameters are parsed rigidly.
/// Further replacement with a smart model with tool selection.
impl MasterAgent {
    pub fn new(client:Arc<ollama::Client>,config:AiConfig) -> Self {
        Self { client, config, request_manager: Arc::new(RequestManager::new()) }
    }

    pub async fn handle_request_stream(
        &self,
        state:Arc<AppState>,
        request: AgentRequest
    ) -> mpsc::Receiver<StreamEvent> {
        let (tx, rx) = mpsc::channel(100);

        let client = self.client.clone();
        let request_manager = self.request_manager.clone();

        tokio::spawn(async move {
            let request_id  = Uuid::now_v7().to_string();
            let cancellation_token = request_manager.register(request_id.clone()).await;
            let context = AgentContext::from_request(request_id.clone(), request.clone(), cancellation_token.clone());

            // Send start event
            let _ = tx
                .send(StreamEvent::Started {
                    request_id: request_id.clone(),
                    timestamp: chrono::Utc::now().timestamp(),
                })
                .await;

            // Process request
            let result = Self::process_request(state.clone(), client, context, tx.clone()).await;

            // Send final event
            match result {
                Ok(final_result) => {
                    let _ = tx
                        .send(StreamEvent::Completed {
                            request_id: request_id.clone(),
                            final_result,
                            timestamp: chrono::Utc::now().timestamp(),
                        })
                        .await;
                }
                Err(e) => {
                    let is_cancelled = e.to_string().contains("cancelled");

                    if is_cancelled {
                        let _ = tx
                            .send(StreamEvent::Cancelled {
                                request_id: request_id.clone(),
                                reason: "User cancelled".to_string(),
                            })
                            .await;
                    } else {
                        let _ = tx
                            .send(StreamEvent::Error {
                                request_id: request_id.clone(),
                                error: e.to_string(),
                                recoverable: false,
                            })
                            .await;
                    }
                }
            }

            request_manager.unregister(&request_id).await;
        });

        rx
    }

    async fn process_request(
        state:Arc<AppState>,
        client: Arc<ollama::Client>,
        context: AgentContext,
        event_tx: mpsc::Sender<StreamEvent>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Send coordinator thinking event
        let _ = event_tx
            .send(StreamEvent::CoordinatorThinking {
                request_id: context.request_id.clone(),
                message: "Analyzing request and determining task type...".to_string(),
            })
            .await;

        context.cancellation_token.check().await?;

        // Parse the prompt
        let mut parser = ContextParser::new();
        let prompt_context = parser.parse(&context.language, &context.message)?;

        // Detect task
        let detector = TaskDetector::new();
        let task = detector.detect_task(&prompt_context, &context.message)?;

        context.cancellation_token.check().await?;

        let _ = event_tx
            .send(StreamEvent::CoordinatorThinking {
                request_id: context.request_id.clone(),
                message: format!("Processing '{:?}' in progress...",task),
            })
            .await;

        context.cancellation_token.check().await?;

        // Execute appropriate task
        let result = match task {
            Task::Object { parameters } => {
                let agent = ObjectAgent::new(
                    client,
                    context.clone(),
                    event_tx.clone(),
                );
                agent.execute(state, &parameters).await?
            }
            Task::Document { parameters } => {
                let agent = DocumentAgent::new(
                    client,
                    context.clone(),
                    event_tx.clone(),
                );
                agent.execute(state, &parameters).await?
            }
            Task::Description { parameters } => {
                let agent = DescriptionAgent::new(
                    client,
                    context,
                    event_tx.clone(),
                );
                agent.execute(state, &parameters).await?
            }
            Task::Comparison { parameters } => {
                let agent = ComparisonAgent::new(
                    client,
                    context.clone(),
                    event_tx.clone(),
                );
                agent.execute(state, &parameters).await?
            }
            Task::Chat => {
                let agent = ChatAgent::new(
                    client,
                    context.clone(),
                    event_tx.clone(),
                );
                agent.execute(state).await?
            }
        };

        Ok(result)
    }

    pub async fn cancel_request(&self, request_id: &str) -> bool {
        self.request_manager.cancel(request_id).await
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use rig::client::Nothing;
    use super::*;
    use crate::init::app_init;
    const URL:&str = "http://localhost:3050";

    fn test_default() -> MasterAgent {
        let client = Arc::new(ollama::Client::builder()
            .api_key(Nothing)
            .base_url(URL)
            .build()
            .unwrap());
        let config = AiConfig {
            url: URL.to_string(),
            text_model: "test-text-model".to_string(),
            vision_model: "test-vision-model".to_string(),
            chat_model: "test-chat-model".to_string(),
            agent_secret: "test-secret".to_string(),
        };
        MasterAgent::new(client, config)
    }
    #[tokio::test]
    async fn test_object_task() {
        let agent = test_default();

        let request = sample_agent_request();
        dotenv::dotenv().ok();
        let (_config, state) = app_init().await.unwrap();

        let mut rx = agent.handle_request_stream(state.clone(),request).await;

        while let Some(event) = rx.recv().await {
            match event {
                StreamEvent::Started { .. } => println!("✓ Started"),
                StreamEvent::CoordinatorThinking { message, .. } => {
                    println!("🤔 {}", message);
                }
                StreamEvent::TextChunk { chunk, .. } => {
                    print!("{}", chunk);
                }
                StreamEvent::ObjectChunk { data, .. } => {
                    println!("\n📦 Object data: {}", serde_json::to_string_pretty(&data).unwrap());
                }
                StreamEvent::Completed { .. } => {
                    println!("\n✅ Completed");
                    break;
                }
                StreamEvent::Error { error, .. } => {
                    println!("\n❌ Error: {}", error);
                    break;
                }
                _ => {}
            }
        }
    }

    fn sample_agent_request() -> AgentRequest {
        let request = AgentRequest {
            message: "show me the last 5 objects".to_string(),
            user_id: "user_123".to_string(),
            chat_id: "chat_123".to_string(),
            language: "en".to_string(),
            object_id: None,
            prev_leaf: None,
            next_leaf: None,
            metadata: None,
        };
        request
    }

    #[tokio::test]
    async fn test_chat_task() {
        let agent = test_default();

        let request = sample_agent_request();
        dotenv::dotenv().ok();
        let (_config, state) = app_init().await.unwrap();

        let mut rx = agent.handle_request_stream(state.clone(),request).await;

        while let Some(event) = rx.recv().await {
            match event {
                StreamEvent::TextChunk { chunk, .. } => {
                    print!("{}", chunk);
                }
                StreamEvent::Completed { .. } => {
                    println!("\n✅ Completed");
                    break;
                }
                _ => {}
            }
        }
    }

    #[tokio::test]
    async fn test_comparison_task() {
        let agent = test_default();
        
        let request = AgentRequest {
            message: "compare the last 2 documents".to_string(),
            user_id: "user_123".to_string(),
            chat_id: "chat_123".to_string(),
            language: "en".to_string(),
            object_id: None,
            prev_leaf: None,
            next_leaf: None,
            metadata: None,
        };
        dotenv::dotenv().ok();
        let (_config, state) = app_init().await.unwrap();

        let mut rx = agent.handle_request_stream(state.clone(),request).await;

        while let Some(event) = rx.recv().await {
            match event {
                StreamEvent::ComparisonChunk { data, .. } => {
                    println!("\n🔄 Comparison: {}", serde_json::to_string_pretty(&data).unwrap());
                }
                StreamEvent::Completed { .. } => {
                    println!("\n✅ Completed");
                    break;
                }
                _ => {}
            }
        }
    }
}
