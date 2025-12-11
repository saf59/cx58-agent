// Updated AgentExecutor to work with S3-stored images

use crate::storage::{ImageProcessor, ImageUrlResolver, StorageService};

// ============================================================================
// Updated AppState with S3 config
// ============================================================================

pub struct AppState {
    pub db: PgPool,
    pub redis: redis::aio::ConnectionManager,
    pub storage: Arc<StorageService>,
    pub image_resolver: Arc<ImageUrlResolver>,
    pub image_processor: Arc<ImageProcessor>,
    pub qdrant: qdrant_client::client::QdrantClient,
    pub ollama_url: String,
    pub agent: Arc<RwLock<AgentExecutor>>,
}

impl AppState {
    pub async fn new(
        db: PgPool,
        redis: redis::aio::ConnectionManager,
        s3_config: S3Config,
        qdrant: qdrant_client::client::QdrantClient,
        ollama_url: String,
    ) -> Result<Self, String> {
        let storage = Arc::new(StorageService::new(s3_config)?);
        let image_resolver = Arc::new(ImageUrlResolver {
            storage: storage.clone(),
            db: db.clone(),
        });
        let image_processor = Arc::new(ImageProcessor::new(storage.clone()));
        let agent = Arc::new(RwLock::new(AgentExecutor::new(ollama_url.clone())));

        Ok(Self {
            db,
            redis,
            storage,
            image_resolver,
            image_processor,
            qdrant,
            ollama_url,
            agent,
        })
    }
}

// ============================================================================
// Updated Vision Processing with S3
// ============================================================================

impl AgentExecutor {
    pub async fn process_vision_request_with_s3(
        &self,
        request: &AgentRequest,
        state: &Arc<AppState>,
    ) -> Result<impl Stream<Item = Result<StreamEvent, String>>, String> {
        let tree_refs = request.tree_context.clone().unwrap_or_default();
        let language = request.language.clone();
        let message = request.message.clone();

        // Get image nodes with S3 URLs
        let image_nodes = get_image_nodes(&state.db, &tree_refs, &request.user_id)
            .await
            .map_err(|e| e.to_string())?;

        let ollama_url = self.ollama_url.clone();
        let storage = state.storage.clone();
        let resolver = state.image_resolver.clone();

        Ok(async_stream::stream! {
            for img_node in image_nodes {
                if let NodeData::Image { url, storage_path, .. } = &img_node.data {
                    yield Ok(StreamEvent::ToolCall {
                        tool: "vision_analysis".to_string(),
                        status: format!("Processing image {}", img_node.id),
                    });

                    // Download from S3 using storage_path
                    let img_bytes = match storage_path {
                        Some(path) => {
                            match storage.download_image(path).await {
                                Ok(bytes) => bytes.to_vec(),
                                Err(e) => {
                                    yield Ok(StreamEvent::Error {
                                        error: format!("S3 download failed: {}", e),
                                    });
                                    continue;
                                }
                            }
                        }
                        None => {
                            // Fallback to URL download (for legacy data)
                            match download_image(url).await {
                                Ok(bytes) => bytes,
                                Err(e) => {
                                    yield Ok(StreamEvent::Error {
                                        error: format!("Download failed: {}", e),
                                    });
                                    continue;
                                }
                            }
                        }
                    };

                    // Call Ollama vision model
                    let vision_prompt = format!(
                        "Language: {}. User question: {}. Describe this image in detail.",
                        language, message
                    );

                    match stream_ollama_vision(&ollama_url, &vision_prompt, img_bytes).await {
                        Ok(mut stream) => {
                            // Prepend image reference
                            yield Ok(StreamEvent::TextChunk {
                                content: format!("\n\n**Image: {}**\n", url),
                            });

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

                            yield Ok(StreamEvent::TextChunk { content: "\n".to_string() });
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
}

// ============================================================================
// Updated NodeData to include storage_path
// ============================================================================

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
        storage_path: Option<String>, // S3 path
        size: Option<u64>,
        mime_type: Option<String>,
        hash: Option<String>,
        description: Option<String>,
        embeddings: Option<Vec<f32>>,
    },
}

// ============================================================================
// Image Import Tool for External URLs
// ============================================================================

pub struct ImageImportTool {
    pub processor: Arc<ImageProcessor>,
    pub db: PgPool,
}

impl ImageImportTool {
    /// Import external image into user's tree
    pub async fn import_from_url(
        &self,
        user_id: &Uuid,
        parent_node_id: &Uuid,
        external_url: &str,
        description: Option<String>,
    ) -> Result<TreeNode, String> {
        let node_id = Uuid::new_v4();

        // Download and upload to S3
        let result = self
            .processor
            .import_external_image(user_id, &node_id, external_url)
            .await?;

        // Create tree node
        let data = NodeData::Image {
            url: result.public_url.clone(),
            storage_path: Some(result.storage_path.clone()),
            size: Some(result.size),
            mime_type: Some(result.mime_type.clone()),
            hash: Some(result.hash.clone()),
            description,
            embeddings: None,
        };

        sqlx::query!(
            r#"
            INSERT INTO tree_nodes (id, user_id, parent_id, node_type, data)
            VALUES ($1, $2, $3, $4, $5)
            "#,
            node_id,
            user_id,
            Some(parent_node_id),
            NodeType::ImageLeaf as _,
            serde_json::to_value(&data).unwrap()
        )
        .execute(&self.db)
        .await
        .map_err(|e| e.to_string())?;

        Ok(TreeNode {
            id: node_id,
            parent_id: Some(*parent_node_id),
            node_type: NodeType::ImageLeaf,
            data,
            children: vec![],
            created_at: chrono::Utc::now(),
        })
    }

    /// Import multiple images in batch
    pub async fn batch_import(
        &self,
        user_id: &Uuid,
        parent_node_id: &Uuid,
        urls: Vec<String>,
    ) -> Result<Vec<TreeNode>, String> {
        let mut nodes = Vec::new();

        for url in urls {
            match self
                .import_from_url(user_id, parent_node_id, &url, None)
                .await
            {
                Ok(node) => nodes.push(node),
                Err(e) => {
                    log::error!("Failed to import {}: {}", url, e);
                    // Continue with other images
                }
            }
        }

        Ok(nodes)
    }
}

// ============================================================================
// Agent Tool: Image Operations
// ============================================================================

pub struct ImageOperationsTool {
    pub processor: Arc<ImageProcessor>,
    pub storage: Arc<StorageService>,
    pub db: PgPool,
}

impl ImageOperationsTool {
    /// Create thumbnail for existing image
    pub async fn create_thumbnail(
        &self,
        user_id: &Uuid,
        node_id: &Uuid,
    ) -> Result<TreeNode, String> {
        // Get original image
        let node = sqlx::query!(
            r#"
            SELECT data
            FROM tree_nodes
            WHERE id = $1 AND user_id = $2 AND node_type = 'ImageLeaf'
            "#,
            node_id,
            user_id
        )
        .fetch_one(&self.db)
        .await
        .map_err(|e| e.to_string())?;

        let data: NodeData = serde_json::from_value(node.data).map_err(|e| e.to_string())?;

        if let NodeData::Image { storage_path, .. } = data {
            let path = storage_path.ok_or("No storage path")?;

            // Create thumbnail
            let thumb_result = self
                .processor
                .create_thumbnail(user_id, node_id, &path, 300, 300)
                .await?;

            // Create thumbnail node
            let thumb_node_id = Uuid::new_v4();
            let thumb_data = NodeData::Image {
                url: thumb_result.public_url.clone(),
                storage_path: Some(thumb_result.storage_path),
                size: Some(thumb_result.size),
                mime_type: Some(thumb_result.mime_type),
                hash: Some(thumb_result.hash),
                description: Some("Thumbnail".to_string()),
                embeddings: None,
            };

            sqlx::query!(
                r#"
                INSERT INTO tree_nodes (id, user_id, parent_id, node_type, data)
                VALUES ($1, $2, $3, $4, $5)
                "#,
                thumb_node_id,
                user_id,
                Some(node_id),
                NodeType::ImageLeaf as _,
                serde_json::to_value(&thumb_data).unwrap()
            )
            .execute(&self.db)
            .await
            .map_err(|e| e.to_string())?;

            Ok(TreeNode {
                id: thumb_node_id,
                parent_id: Some(*node_id),
                node_type: NodeType::ImageLeaf,
                data: thumb_data,
                children: vec![],
                created_at: chrono::Utc::now(),
            })
        } else {
            Err("Not an image node".to_string())
        }
    }

    /// Get presigned URL for direct download
    pub async fn get_download_url(
        &self,
        user_id: &Uuid,
        node_id: &Uuid,
        expires_in_secs: u64,
    ) -> Result<String, String> {
        // Verify ownership
        let node = sqlx::query!(
            r#"
            SELECT data
            FROM tree_nodes
            WHERE id = $1 AND user_id = $2 AND node_type = 'ImageLeaf'
            "#,
            node_id,
            user_id
        )
        .fetch_one(&self.db)
        .await
        .map_err(|e| e.to_string())?;

        let data: NodeData = serde_json::from_value(node.data).map_err(|e| e.to_string())?;

        if let NodeData::Image { storage_path, .. } = data {
            let path = storage_path.ok_or("No storage path")?;
            self.storage
                .generate_presigned_url(&path, expires_in_secs)
                .await
        } else {
            Err("Not an image node".to_string())
        }
    }
}

// ============================================================================
// Updated Chat Handler with Image Tools
// ============================================================================

pub async fn chat_stream_handler_with_images(
    State(state): State<Arc<AppState>>,
    Json(request): Json<AgentRequest>,
) -> Sse<impl Stream<Item = Result<Event, String>>> {
    let agent = state.agent.read().await;

    // Check if request involves image operations
    let has_images = request
        .tree_context
        .as_ref()
        .map(|refs| !refs.is_empty())
        .unwrap_or(false);

    let stream = if has_images && detect_image_operations(&request.message) {
        // Use vision processing
        agent
            .process_vision_request_with_s3(&request, &state)
            .await
            .unwrap_or_else(|e| {
                futures::stream::once(async move { Ok(StreamEvent::Error { error: e }) }).boxed()
            })
            .boxed()
    } else {
        // Regular text processing
        agent.execute(request, state.clone()).await.boxed()
    };

    let event_stream = stream.map(|result| {
        result.and_then(|event| {
            serde_json::to_string(&event)
                .map_err(|e| e.to_string())
                .map(|json| Event::default().data(json))
        })
    });

    Sse::new(event_stream).keep_alive(KeepAlive::default())
}

// ============================================================================
// Image Migration Tool (for existing data)
// ============================================================================

pub struct ImageMigrationTool {
    pub processor: Arc<ImageProcessor>,
    pub resolver: Arc<ImageUrlResolver>,
    pub db: PgPool,
}

impl ImageMigrationTool {
    /// Migrate images from external URLs to S3
    pub async fn migrate_user_images(&self, user_id: &Uuid) -> Result<usize, String> {
        // Find all image nodes without storage_path
        let nodes = sqlx::query!(
            r#"
            SELECT id, data
            FROM tree_nodes
            WHERE user_id = $1
            AND node_type = 'ImageLeaf'
            AND data->>'storage_path' IS NULL
            "#,
            user_id
        )
        .fetch_all(&self.db)
        .await
        .map_err(|e| e.to_string())?;

        let mut migrated = 0;

        for node in nodes {
            let data: NodeData = serde_json::from_value(node.data).map_err(|e| e.to_string())?;

            if let NodeData::Image { url, .. } = data {
                // Import to S3
                match self
                    .processor
                    .import_external_image(user_id, &node.id, &url)
                    .await
                {
                    Ok(result) => {
                        // Update node
                        let _ = self
                            .resolver
                            .update_node_url(&node.id, &result.public_url, &result.storage_path)
                            .await;

                        migrated += 1;
                    }
                    Err(e) => {
                        log::error!("Migration failed for {}: {}", node.id, e);
                    }
                }
            }
        }

        Ok(migrated)
    }
}

// ============================================================================
// Complete Router with All Endpoints
// ============================================================================

pub fn create_full_router(state: Arc<AppState>) -> Router {
    Router::new()
        // Agent endpoints
        .route("/api/agent/chat", post(chat_stream_handler_with_images))
        .route("/api/agent/tree/:user_id/:root_id", get(get_tree_handler))
        // Image management
        .route("/api/images/upload", post(upload_image_handler))
        .route("/api/images/:node_id", get(get_image_handler))
        .route("/api/images/:node_id", delete(delete_image_handler))
        .route("/api/images/batch", post(batch_upload_handler))
        .route(
            "/api/images/:node_id/thumbnail",
            post(create_thumbnail_handler),
        )
        .route(
            "/api/images/:node_id/download",
            get(get_download_url_handler),
        )
        // Image import
        .route("/api/images/import", post(import_image_handler))
        .route("/api/images/import/batch", post(batch_import_handler))
        // Migration (admin only)
        .route(
            "/api/admin/migrate/:user_id",
            post(migrate_user_images_handler),
        )
        .with_state(state)
}

// Additional handlers implementation...
async fn create_thumbnail_handler(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<Uuid>,
) -> Result<Json<TreeNode>, (StatusCode, String)> {
    let user_id = expect_context::<Uuid>();

    let tool = ImageOperationsTool {
        processor: state.image_processor.clone(),
        storage: state.storage.clone(),
        db: state.db.clone(),
    };

    tool.create_thumbnail(&user_id, &node_id)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn get_download_url_handler(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let user_id = expect_context::<Uuid>();

    let tool = ImageOperationsTool {
        processor: state.image_processor.clone(),
        storage: state.storage.clone(),
        db: state.db.clone(),
    };

    let url = tool
        .get_download_url(&user_id, &node_id, 3600)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(Json(serde_json::json!({ "url": url })))
}

async fn import_image_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<TreeNode>, (StatusCode, String)> {
    let user_id = expect_context::<Uuid>();
    let url = payload["url"]
        .as_str()
        .ok_or((StatusCode::BAD_REQUEST, "Missing url".to_string()))?;
    let parent_id = payload["parent_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or((StatusCode::BAD_REQUEST, "Invalid parent_id".to_string()))?;

    let tool = ImageImportTool {
        processor: state.image_processor.clone(),
        db: state.db.clone(),
    };

    tool.import_from_url(&user_id, &parent_id, url, None)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn batch_import_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<Vec<TreeNode>>, (StatusCode, String)> {
    let user_id = expect_context::<Uuid>();
    let urls: Vec<String> = serde_json::from_value(payload["urls"].clone())
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let parent_id = payload["parent_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or((StatusCode::BAD_REQUEST, "Invalid parent_id".to_string()))?;

    let tool = ImageImportTool {
        processor: state.image_processor.clone(),
        db: state.db.clone(),
    };

    tool.batch_import(&user_id, &parent_id, urls)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn migrate_user_images_handler(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // Add admin check here

    let tool = ImageMigrationTool {
        processor: state.image_processor.clone(),
        resolver: state.image_resolver.clone(),
        db: state.db.clone(),
    };

    let count = tool
        .migrate_user_images(&user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(Json(serde_json::json!({ "migrated": count })))
}
