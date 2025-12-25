use rig::providers::ollama;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use rig::client::Nothing;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;
use crate::agents::{ChatAgent, ComparisonAgent, ContextParser, DescriptionAgent, DocumentAgent, ObjectAgent, StreamEvent, Task, TaskDetector};
use crate::AppState;

const IS_LOCAL: bool = false;

// ============================================================================
// CANCELLATION TOKEN
// ============================================================================

#[derive(Clone, Debug)]
pub struct CancellationToken {
    cancelled: Arc<RwLock<bool>>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(RwLock::new(false)),
        }
    }

    pub async fn cancel(&self) {
        let mut cancelled = self.cancelled.write().await;
        *cancelled = true;
    }

    pub async fn is_cancelled(&self) -> bool {
        *self.cancelled.read().await
    }

    pub async fn check(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.is_cancelled().await {
            Err("Operation cancelled".into())
        } else {
            Ok(())
        }
    }
}

// ============================================================================
// REQUEST MANAGER
// ============================================================================

pub struct RequestManager {
    active_requests: Arc<RwLock<HashMap<String, CancellationToken>>>,
}

impl RequestManager {
    pub fn new() -> Self {
        Self {
            active_requests: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register(&self, request_id: String) -> CancellationToken {
        let token = CancellationToken::new();
        let mut requests = self.active_requests.write().await;
        requests.insert(request_id, token.clone());
        token
    }

    pub async fn cancel(&self, request_id: &str) -> bool {
        let requests = self.active_requests.read().await;
        if let Some(token) = requests.get(request_id) {
            token.cancel().await;
            true
        } else {
            false
        }
    }

    pub async fn unregister(&self, request_id: &str) {
        let mut requests = self.active_requests.write().await;
        requests.remove(request_id);
    }
}

// ============================================================================
// REQUEST STRUCTURES
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentRequest {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}
impl AgentRequest {
    fn new(message:String) -> Self {
        Self {message, ..}
    }
}
#[derive(Debug, Clone)]
pub struct AgentContext {
    pub request_id: String,
    pub user_id: Option<String>,
    pub chat_id: Option<String>,
    pub object_id: Option<String>,
    pub language: String,
    pub metadata: serde_json::Value,
    pub cancellation_token: CancellationToken,
}

impl AgentContext {
    pub fn from_request(req: AgentRequest, cancellation_token: CancellationToken) -> Self {
        Self {
            request_id: Uuid::now_v7().to_string(),
            user_id: req.user_id,
            chat_id: req.chat_id,
            object_id: req.object_id,
            language: req.language.unwrap_or_else(|| "en".to_string()),
            metadata: req.metadata.unwrap_or(serde_json::json!({})),
            cancellation_token,
        }
    }
}

// ============================================================================
// MASTER AGENT
// ============================================================================

pub struct MasterAgent {
    client: ollama::Client,
    request_manager: Arc<RequestManager>,
}

impl MasterAgent {
    pub fn new(ai_url: &str) -> Self {
        let client = ollama::Client::builder()
            .api_key(Nothing)
            .base_url(ai_url)
            .build()
            .unwrap();
        Self {
            client,
            request_manager: Arc::new(RequestManager::new()),
        }
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
            let cancellation_token = request_manager.register(Uuid::now_v7().to_string()).await;
            let context = AgentContext::from_request(request.clone(), cancellation_token.clone());
            let request_id = context.request_id.clone();

            // Send start event
            let _ = tx
                .send(StreamEvent::Started {
                    request_id: request_id.clone(),
                    timestamp: chrono::Utc::now().timestamp(),
                })
                .await;

            // Process request
            let result = Self::process_request(client, request, context, tx.clone()).await;

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
        client: ollama::Client,
        request: AgentRequest,
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
        let prompt_context = parser.parse(&context.language, &request.message)?;

        // Detect task
        let detector = TaskDetector::new();
        let task = detector.detect_task(&prompt_context, &request.message)?;

        context.cancellation_token.check().await?;

        // Execute appropriate task
        let result = match task {
            Task::Object { parameters } => {
                let agent = ObjectAgent::new(
                    client,
                    context.request_id.clone(),
                    event_tx.clone(),
                );
                agent.execute(&request.message, &parameters).await?
            }
            Task::Document { parameters } => {
                let agent = DocumentAgent::new(
                    client,
                    context.request_id.clone(),
                    event_tx.clone(),
                );
                agent.execute(&request.message, &parameters).await?
            }
            Task::Description { parameters } => {
                let agent = DescriptionAgent::new(
                    client,
                    context.request_id.clone(),
                    event_tx.clone(),
                );
                agent.execute(&request.message, &parameters).await?
            }
            Task::Comparison { parameters } => {
                let agent = ComparisonAgent::new(
                    client,
                    context.request_id.clone(),
                    event_tx.clone(),
                );
                agent.execute(&request.message, &parameters).await?
            }
            Task::Chat => {
                let agent = ChatAgent::new(
                    client,
                    context.request_id.clone(),
                    event_tx.clone(),
                );
                agent.execute(&request.message, &context.language).await?
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
    use super::*;
    const URL:&str = "http://localhost:8080";
    #[tokio::test]
    async fn test_object_task() {
        let agent = MasterAgent::new(URL);
        
        let request = AgentRequest {
            message: "show me the last 5 objects".to_string(),
            user_id: Some("user_123".to_string()),
            chat_id: None,
            object_id: None,
            language: Some("en".to_string()),
            session_id: None,
            metadata: None,
        };

        let mut rx = agent.handle_request_stream(request).await;

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

    #[tokio::test]
    async fn test_chat_task() {
        let agent = MasterAgent::new(URL);
        
        let request = AgentRequest {
            message: "hello, how are you?".to_string(),
            user_id: Some("user_456".to_string()),
            chat_id: Some("chat_789".to_string()),
            object_id: None,
            language: Some("en".to_string()),
            session_id: None,
            metadata: None,
        };

        let mut rx = agent.handle_request_stream(request).await;

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
        let agent = MasterAgent::new(URL);
        
        let request = AgentRequest {
            message: "compare the last 2 documents".to_string(),
            user_id: Some("user_789".to_string()),
            chat_id: None,
            object_id: None,
            language: Some("en".to_string()),
            session_id: None,
            metadata: None,
        };

        let mut rx = agent.handle_request_stream(request).await;

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

fn main() {
    println!("Master Agent SSE Application");
    println!("Use tests to run examples: cargo test -- --nocapture");
}