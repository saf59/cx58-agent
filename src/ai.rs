// Cargo.toml dependencies:
// axum = "0.7"
// tokio = { version = "1", features = ["full"] }
// rig-core = "0.2"
// serde = { version = "1", features = ["derive"] }
// serde_json = "1"
// sqlx = { version = "0.7", features = ["postgres", "runtime-tokio-rustls", "uuid", "chrono"] }
// redis = { version = "0.24", features = ["tokio-comp", "connection-manager"] }
// aws-sdk-s3 = "1"
// qdrant-client = "1.7"
// uuid = { version = "1", features = ["serde", "v4"] }
// chrono = { version = "0.4", features = ["serde"] }
// futures = "0.3"
// tokio-stream = "0.1"
// tower-http = { version = "0.5", features = ["cors"] }
// reqwest = { version = "0.11", features = ["json", "stream"] }

use axum::{
    Json, Router,
    extract::{Path, State},
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
};
use futures::stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

// ============================================================================
// Domain Models
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeNode {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub node_type: NodeType,
    pub data: NodeData,
    pub children: Vec<TreeNode>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NodeType {
    Root,
    Branch,
    ImageLeaf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NodeData {
    Root {
        title: String,
    },
    Branch {
        label: String,
        description: Option<String>,
    },
    Image {
        url: String,
        description: Option<String>,
        embeddings: Option<Vec<f32>>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: Uuid,
    pub chat_id: Uuid,
    pub user_id: Uuid,
    pub role: MessageRole,
    pub content: String,
    pub tree_refs: Vec<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

// ============================================================================
// Agent Request/Response
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct AgentRequest {
    pub message: String,
    pub chat_id: Uuid,
    pub user_id: Uuid,
    pub session_id: String,
    pub language: String,
    pub tree_context: Option<Vec<Uuid>>, // Referenced nodes
}

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type")]
pub enum StreamEvent {
    TextChunk { content: String },
    TreeUpdate { nodes: Vec<TreeNode> },
    ToolCall { tool: String, status: String },
    Complete { message_id: Uuid },
    Error { error: String },
}

// ============================================================================
// Application State
// ============================================================================

pub struct AppState {
    pub db: PgPool,
    pub redis: redis::aio::ConnectionManager,
    pub s3_client: aws_sdk_s3::Client,
    pub qdrant: qdrant_client::client::QdrantClient,
    pub ollama_url: String,
    pub agent: Arc<RwLock<AgentExecutor>>,
}

// ============================================================================
// Agent Executor
// ============================================================================

pub struct AgentExecutor {
    pub ollama_url: String,
}

impl AgentExecutor {
    pub fn new(ollama_url: String) -> Self {
        Self { ollama_url }
    }

    pub async fn execute(
        &self,
        request: AgentRequest,
        state: Arc<AppState>,
    ) -> impl Stream<Item = Result<StreamEvent, String>> {
        let state_clone = state.clone();
        let req_clone = request.clone();

        async_stream::stream! {
            // Step 1: Load tree context if provided
            if let Some(tree_refs) = &req_clone.tree_context {
                yield Ok(StreamEvent::ToolCall {
                    tool: "load_tree_context".to_string(),
                    status: "started".to_string(),
                });

                match load_tree_nodes(&state_clone.db, tree_refs, &req_clone.user_id).await {
                    Ok(nodes) => {
                        yield Ok(StreamEvent::TreeUpdate { nodes });
                    }
                    Err(e) => {
                        yield Ok(StreamEvent::Error {
                            error: format!("Failed to load tree context: {}", e),
                        });
                        return;
                    }
                }
            }

            // Step 2: Analyze message for image operations
            let needs_vision = detect_image_operations(&req_clone.message);

            if needs_vision {
                yield Ok(StreamEvent::ToolCall {
                    tool: "vision_analysis".to_string(),
                    status: "started".to_string(),
                });

                // Process images with multimodal model
                match self.process_vision_request(&req_clone, &state_clone).await {
                    Ok(mut stream) => {
                        while let Some(event) = stream.next().await {
                            yield event;
                        }
                    }
                    Err(e) => {
                        yield Ok(StreamEvent::Error {
                            error: format!("Vision processing failed: {}", e),
                        });
                    }
                }
            } else {
                // Step 3: Regular text processing
                match self.process_text_request(&req_clone, &state_clone).await {
                    Ok(mut stream) => {
                        while let Some(event) = stream.next().await {
                            yield event;
                        }
                    }
                    Err(e) => {
                        yield Ok(StreamEvent::Error {
                            error: format!("Text processing failed: {}", e),
                        });
                    }
                }
            }

            // Step 4: Save message to DB
            let message_id = Uuid::new_v4();
            yield Ok(StreamEvent::Complete { message_id });
        }
    }

    async fn process_vision_request(
        &self,
        request: &AgentRequest,
        state: &Arc<AppState>,
    ) -> Result<impl Stream<Item = Result<StreamEvent, String>>, String> {
        // Get image URLs from tree context
        let image_nodes = if let Some(tree_refs) = &request.tree_context {
            get_image_nodes(&state.db, tree_refs, &request.user_id)
                .await
                .map_err(|e| e.to_string())?
        } else {
            vec![]
        };

        let ollama_url = self.ollama_url.clone();
        let message = request.message.clone();
        let lang = request.language.clone();

        Ok(async_stream::stream! {
            for img_node in image_nodes {
                if let NodeData::Image { url, .. } = &img_node.data {
                    // Download image
                    let img_bytes = match download_image(url).await {
                        Ok(b) => b,
                        Err(e) => {
                            yield Ok(StreamEvent::Error {
                                error: format!("Failed to download image: {}", e),
                            });
                            continue;
                        }
                    };

                    // Call Ollama vision model
                    let vision_prompt = format!(
                        "Language: {}. User question: {}. Describe this image in detail.",
                        lang, message
                    );

                    match stream_ollama_vision(&ollama_url, &vision_prompt, img_bytes).await {
                        Ok(mut stream) => {
                            while let Some(chunk) = stream.next().await {
                                match chunk {
                                    Ok(text) => {
                                        yield Ok(StreamEvent::TextChunk { content: text });
                                    }
                                    Err(e) => {
                                        yield Ok(StreamEvent::Error {
                                            error: format!("Vision stream error: {}", e),
                                        });
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            yield Ok(StreamEvent::Error {
                                error: format!("Vision model error: {}", e),
                            });
                        }
                    }
                }
            }
        })
    }

    async fn process_text_request(
        &self,
        request: &AgentRequest,
        state: &Arc<AppState>,
    ) -> Result<impl Stream<Item = Result<StreamEvent, String>>, String> {
        let ollama_url = self.ollama_url.clone();
        let prompt = build_prompt(request);

        Ok(async_stream::stream! {
            match stream_ollama_text(&ollama_url, &prompt).await {
                Ok(mut stream) => {
                    while let Some(chunk) = stream.next().await {
                        match chunk {
                            Ok(text) => {
                                yield Ok(StreamEvent::TextChunk { content: text });
                            }
                            Err(e) => {
                                yield Ok(StreamEvent::Error {
                                    error: format!("Text stream error: {}", e),
                                });
                            }
                        }
                    }
                }
                Err(e) => {
                    yield Ok(StreamEvent::Error {
                        error: format!("Text model error: {}", e),
                    });
                }
            }
        })
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn detect_image_operations(message: &str) -> bool {
    let keywords = [
        "describe",
        "compare",
        "image",
        "picture",
        "photo",
        "показать",
        "описать",
        "сравнить",
    ];
    keywords.iter().any(|k| message.to_lowercase().contains(k))
}

fn build_prompt(request: &AgentRequest) -> String {
    format!(
        "Language: {}. User: {}. Provide a helpful response in the specified language.",
        request.language, request.message
    )
}

async fn load_tree_nodes(
    db: &PgPool,
    node_ids: &[Uuid],
    user_id: &Uuid,
) -> Result<Vec<TreeNode>, sqlx::Error> {
    sqlx::query_as!(
        TreeNode,
        r#"
        SELECT id, parent_id, node_type as "node_type: _",
               data as "data: _", children as "children: _", created_at
        FROM tree_nodes
        WHERE id = ANY($1) AND user_id = $2
        "#,
        node_ids,
        user_id
    )
    .fetch_all(db)
    .await
}

async fn get_image_nodes(
    db: &PgPool,
    node_ids: &[Uuid],
    user_id: &Uuid,
) -> Result<Vec<TreeNode>, sqlx::Error> {
    sqlx::query_as!(
        TreeNode,
        r#"
        SELECT id, parent_id, node_type as "node_type: _",
               data as "data: _", children as "children: _", created_at
        FROM tree_nodes
        WHERE id = ANY($1) AND user_id = $2 AND node_type = 'ImageLeaf'
        "#,
        node_ids,
        user_id
    )
    .fetch_all(db)
    .await
}

async fn download_image(url: &str) -> Result<Vec<u8>, reqwest::Error> {
    reqwest::get(url).await?.bytes().await.map(|b| b.to_vec())
}

async fn stream_ollama_vision(
    ollama_url: &str,
    prompt: &str,
    image_bytes: Vec<u8>,
) -> Result<impl Stream<Item = Result<String, String>>, String> {
    let client = reqwest::Client::new();
    let base64_image = base64::encode(image_bytes);

    let response = client
        .post(format!("{}/api/generate", ollama_url))
        .json(&serde_json::json!({
            "model": "llava",
            "prompt": prompt,
            "images": [base64_image],
            "stream": true
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    Ok(parse_ollama_stream(response))
}

async fn stream_ollama_text(
    ollama_url: &str,
    prompt: &str,
) -> Result<impl Stream<Item = Result<String, String>>, String> {
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/api/generate", ollama_url))
        .json(&serde_json::json!({
            "model": "llama3.2",
            "prompt": prompt,
            "stream": true
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    Ok(parse_ollama_stream(response))
}

fn parse_ollama_stream(response: reqwest::Response) -> impl Stream<Item = Result<String, String>> {
    response.bytes_stream().map(|chunk| {
        chunk.map_err(|e| e.to_string()).and_then(|bytes| {
            String::from_utf8(bytes.to_vec())
                .map_err(|e| e.to_string())
                .and_then(|text| {
                    serde_json::from_str::<serde_json::Value>(&text)
                        .map_err(|e| e.to_string())
                        .and_then(|json| {
                            json.get("response")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                                .ok_or_else(|| "No response field".to_string())
                        })
                })
        })
    })
}

// ============================================================================
// HTTP Handlers
// ============================================================================

pub async fn chat_stream_handler(
    State(state): State<Arc<AppState>>,
    Json(request): Json<AgentRequest>,
) -> Sse<impl Stream<Item = Result<Event, String>>> {
    let agent = state.agent.read().await;
    let stream = agent.execute(request, state.clone()).await;

    let event_stream = stream.map(|result| {
        result.and_then(|event| {
            serde_json::to_string(&event)
                .map_err(|e| e.to_string())
                .map(|json| Event::default().data(json))
        })
    });

    Sse::new(event_stream).keep_alive(KeepAlive::default())
}

pub async fn get_tree_handler(
    State(state): State<Arc<AppState>>,
    Path((user_id, root_id)): Path<(Uuid, Uuid)>,
) -> Json<TreeNode> {
    // Load full tree with recursion
    let tree = load_full_tree(&state.db, &user_id, &root_id).await.unwrap();
    Json(tree)
}

async fn load_full_tree(
    db: &PgPool,
    user_id: &Uuid,
    root_id: &Uuid,
) -> Result<TreeNode, sqlx::Error> {
    // Recursive CTE to load entire tree
    sqlx::query_as!(
        TreeNode,
        r#"
        WITH RECURSIVE tree AS (
            SELECT * FROM tree_nodes WHERE id = $1 AND user_id = $2
            UNION ALL
            SELECT tn.* FROM tree_nodes tn
            INNER JOIN tree t ON tn.parent_id = t.id
            WHERE tn.user_id = $2
        )
        SELECT * FROM tree
        "#,
        root_id,
        user_id
    )
    .fetch_one(db)
    .await
}

// ============================================================================
// Router Setup
// ============================================================================

pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/agent/chat", post(chat_stream_handler))
        .route("/api/agent/tree/:user_id/:root_id", get(get_tree_handler))
        .with_state(state)
}
