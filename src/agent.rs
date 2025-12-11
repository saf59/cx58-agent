// backend/src/agent.rs - Updated for aws-sdk-s3

use ai_agent_shared::*;
use axum::{
    extract::{Path, State},
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures::stream::{Stream, StreamExt};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

pub use crate::storage::{StorageService, ImageProcessor, ImageUrlResolver};

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
    ) -> impl Stream<Item = std::result::Result<StreamEvent, String>> {
        let state_clone = state.clone();
        let req_clone = request.clone();
        
        async_stream::stream! {
            // Load tree context if provided
            if let Some(tree_refs) = &req_clone.tree_context {
                yield Ok(StreamEvent::tool("load_tree_context", "started"));

                match load_tree_nodes(&state_clone.db, tree_refs, &req_clone.user_id).await {
                    Ok(nodes) => {
                        yield Ok(StreamEvent::TreeUpdate { nodes });
                    }
                    Err(e) => {
                        yield Ok(StreamEvent::error(format!("Failed to load tree: {}", e)));
                        return;
                    }
                }
            }

            // Detect if we need vision
            let needs_vision = detect_image_operations(&req_clone.message);
            
            if needs_vision {
                yield Ok(StreamEvent::tool("vision_analysis", "started"));

                match self.process_vision_request(&req_clone, &state_clone).await {
                    Ok(mut stream) => {
                        while let Some(event) = stream.next().await {
                            yield event;
                        }
                    }
                    Err(e) => {
                        yield Ok(StreamEvent::error(format!("Vision failed: {}", e)));
                    }
                }
            } else {
                match self.process_text_request(&req_clone, &state_clone).await {
                    Ok(mut stream) => {
                        while let Some(event) = stream.next().await {
                            yield event;
                        }
                    }
                    Err(e) => {
                        yield Ok(StreamEvent::error(format!("Text failed: {}", e)));
                    }
                }
            }

            let message_id = Uuid::new_v4();
            yield Ok(StreamEvent::complete(message_id));
        }
    }

    async fn process_vision_request(
        &self,
        request: &AgentRequest,
        state: &Arc<AppState>,
    ) -> std::result::Result<impl Stream<Item = std::result::Result<StreamEvent, String>>, String> {
        let tree_refs = request.tree_context.clone().unwrap_or_default();
        let language = request.language.clone();
        let message = request.message.clone();
        
        let image_nodes = get_image_nodes(&state.db, &tree_refs, &request.user_id)
            .await
            .map_err(|e| e.to_string())?;

        let ollama_url = self.ollama_url.clone();
        let storage = state.storage.clone();

        Ok(async_stream::stream! {
            for img_node in image_nodes {
                if let NodeData::Image { url, storage_path, .. } = &img_node.data {
                    yield Ok(StreamEvent::tool("describe_image", format!("Processing {}", img_node.id)));

                    // Download from S3 using storage_path
                    let img_bytes = match storage_path {
                        Some(path) => {
                            match storage.download_image(path).await {
                                Ok(bytes) => bytes.to_vec(),
                                Err(e) => {
                                    yield Ok(StreamEvent::error(format!("S3 download failed: {}", e)));
                                    continue;
                                }
                            }
                        }
                        None => {
                            // Fallback to URL download for legacy data
                            match download_image(url).await {
                                Ok(bytes) => bytes,
                                Err(e) => {
                                    yield Ok(StreamEvent::error(format!("Download failed: {}", e)));
                                    continue;
                                }
                            }
                        }
                    };

                    // Call Ollama vision model
                    let vision_prompt = format!(
                        "Language: {}. Question: {}. Describe this image.",
                        language, message
                    );

                    match stream_ollama_vision(&ollama_url, &vision_prompt, img_bytes).await {
                        Ok(mut stream) => {
                            yield Ok(StreamEvent::text(format!("\n**Image: {}**\n", url)));

                            while let Some(chunk) = stream.next().await {
                                match chunk {
                                    Ok(text) => {
                                        yield Ok(StreamEvent::text(text));
                                    }
                                    Err(e) => {
                                        yield Ok(StreamEvent::error(format!("Vision error: {}", e)));
                                    }
                                }
                            }

                            yield Ok(StreamEvent::text("\n"));
                        }
                        Err(e) => {
                            yield Ok(StreamEvent::error(format!("Vision model error: {}", e)));
                        }
                    }
                }
            }
        })
    }

    async fn process_text_request(
        &self,
        request: &AgentRequest,
        _state: &Arc<AppState>,
    ) -> std::result::Result<impl Stream<Item = std::result::Result<StreamEvent, String>>, String> {
        let ollama_url = self.ollama_url.clone();
        let prompt = build_prompt(request);

        Ok(async_stream::stream! {
            match stream_ollama_text(&ollama_url, &prompt).await {
                Ok(mut stream) => {
                    while let Some(chunk) = stream.next().await {
                        match chunk {
                            Ok(text) => {
                                yield Ok(StreamEvent::text(text));
                            }
                            Err(e) => {
                                yield Ok(StreamEvent::error(format!("Stream error: {}", e)));
                            }
                        }
                    }
                }
                Err(e) => {
                    yield Ok(StreamEvent::error(format!("Model error: {}", e)));
                }
            }
        })
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn detect_image_operations(message: &str) -> bool {
    let keywords = ["describe", "compare", "image", "picture", "photo", "показать", "описать", "сравнить"];
    keywords.iter().any(|k| message.to_lowercase().contains(k))
}

fn build_prompt(request: &AgentRequest) -> String {
    format!(
        "Language: {}. User: {}",
        request.language, request.message
    )
}

async fn load_tree_nodes(
    db: &sqlx::PgPool,
    node_ids: &[Uuid],
    user_id: &Uuid,
) -> Result<Vec<TreeNode>> {
    let nodes = sqlx::query!(
        r#"
        SELECT id, parent_id, node_type as "node_type!: NodeType", 
               data, created_at
        FROM tree_nodes
        WHERE id = ANY($1) AND user_id = $2
        "#,
        node_ids,
        user_id
    )
    .fetch_all(db)
    .await?;

    Ok(nodes
        .into_iter()
        .map(|row| TreeNode {
            id: row.id,
            parent_id: row.parent_id,
            node_type: row.node_type,
            data: serde_json::from_value(row.data).unwrap(),
            children: vec![],
            created_at: row.created_at.to_rfc3339(),
        })
        .collect())
}

async fn get_image_nodes(
    db: &sqlx::PgPool,
    node_ids: &[Uuid],
    user_id: &Uuid,
) -> Result<Vec<TreeNode>> {
    let nodes = sqlx::query!(
        r#"
        SELECT id, parent_id, node_type as "node_type!: NodeType",
               data, created_at
        FROM tree_nodes
        WHERE id = ANY($1) AND user_id = $2 AND node_type = 'ImageLeaf'
        "#,
        node_ids,
        user_id
    )
    .fetch_all(db)
    .await?;

    Ok(nodes
        .into_iter()
        .map(|row| TreeNode {
            id: row.id,
            parent_id: row.parent_id,
            node_type: row.node_type,
            data: serde_json::from_value(row.data).unwrap(),
            children: vec![],
            created_at: row.created_at.to_rfc3339(),
        })
        .collect())
}

async fn download_image(url: &str) -> std::result::Result<Vec<u8>, reqwest::Error> {
    reqwest::get(url).await?.bytes().await.map(|b| b.to_vec())
}

async fn stream_ollama_vision(
    ollama_url: &str,
    prompt: &str,
    image_bytes: Vec<u8>,
) -> std::result::Result<impl Stream<Item = std::result::Result<String, String>>, String> {
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
) -> std::result::Result<impl Stream<Item = std::result::Result<String, String>>, String> {
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

fn parse_ollama_stream(response: reqwest::Response) -> impl Stream<Item = std::result::Result<String, String>> {
    response.bytes_stream().map(|chunk| {
        chunk
            .map_err(|e| e.to_string())
            .and_then(|bytes| {
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

pub async fn chat_stream_handler_with_images(
    State(state): State<Arc<AppState>>,
    Json(request): Json<AgentRequest>,
) -> Sse<impl Stream<Item = std::result::Result<Event, String>>> {
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
) -> Result<Json<TreeNode>> {
    let tree = load_full_tree(&state.db, &user_id, &root_id).await?;
    Ok(Json(tree))
}

async fn load_full_tree(
    db: &sqlx::PgPool,
    user_id: &Uuid,
    root_id: &Uuid,
) -> Result<TreeNode> {
    let node = sqlx::query!(
        r#"
        WITH RECURSIVE tree AS (
            SELECT * FROM tree_nodes WHERE id = $1 AND user_id = $2
            UNION ALL
            SELECT tn.* FROM tree_nodes tn
            INNER JOIN tree t ON tn.parent_id = t.id
            WHERE tn.user_id = $2
        )
        SELECT id, parent_id, node_type as "node_type!: NodeType",
               data, created_at
        FROM tree
        WHERE id = $1
        "#,
        root_id,
        user_id
    )
    .fetch_one(db)
    .await?;

    Ok(TreeNode {
        id: node.id,
        parent_id: node.parent_id,
        node_type: node.node_type,
        data: serde_json::from_value(node.data).unwrap(),
        children: vec![],
        created_at: node.created_at.to_rfc3339(),
    })
}

// ============================================================================
// AppState
// ============================================================================

pub struct AppState {
    pub db: sqlx::PgPool,
    pub redis: redis::aio::ConnectionManager,
    pub storage: Arc<StorageService>,
    pub image_resolver: Arc<ImageUrlResolver>,
    pub image_processor: Arc<ImageProcessor>,
    pub qdrant: qdrant_client::client::QdrantClient,
    pub ollama_url: String,
    pub agent: Arc<RwLock<AgentExecutor>>,
    pub orchestrator: Arc<crate::rig_integration::AgentOrchestrator>,
}

impl AppState {
    pub fn extract_user_id(&self) -> Result<Uuid> {
        // Populated by middleware
        Ok(Uuid::new_v4())
    }
}