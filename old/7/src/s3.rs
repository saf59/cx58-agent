// Cargo.toml additional dependencies:
// opendal = { version = "0.45", features = ["services-s3"] }
// bytes = "1"
// mime_guess = "2"
// sha2 = "0.10"
// hex = "0.4"

use bytes::Bytes;
use opendal::{Operator, services::S3};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

// ============================================================================
// S3 Storage Configuration
// ============================================================================

#[derive(Debug, Clone)]
pub struct S3Config {
    pub bucket: String,
    pub region: String,
    pub endpoint: Option<String>,
    pub access_key: String,
    pub secret_key: String,
    pub public_url_base: String, // e.g., "https://cdn.example.com"
}

// ============================================================================
// Storage Service using OpenDAL (RustFS)
// ============================================================================

pub struct StorageService {
    pub operator: Operator,
    pub config: S3Config,
}

impl StorageService {
    pub fn new(config: S3Config) -> Result<Self, String> {
        // Initialize OpenDAL S3 backend
        let mut builder = S3::default();

        builder
            .bucket(&config.bucket)
            .region(&config.region)
            .access_key_id(&config.access_key)
            .secret_access_key(&config.secret_key);

        if let Some(endpoint) = &config.endpoint {
            builder.endpoint(endpoint);
        }

        let operator = Operator::new(builder)
            .map_err(|e| format!("Failed to create S3 operator: {}", e))?
            .finish();

        Ok(Self { operator, config })
    }

    /// Upload image and return public URL
    pub async fn upload_image(
        &self,
        user_id: &Uuid,
        node_id: &Uuid,
        image_data: Bytes,
        filename: &str,
    ) -> Result<StorageResult, String> {
        // Generate storage path: user_id/node_id/hash.ext
        let hash = self.compute_hash(&image_data);
        let extension = std::path::Path::new(filename)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("jpg");

        let storage_path = format!("images/{}/{}/{}.{}", user_id, node_id, hash, extension);

        // Detect MIME type
        let mime_type = mime_guess::from_path(filename)
            .first_or_octet_stream()
            .to_string();

        // Upload to S3 via OpenDAL
        self.operator
            .write(&storage_path, image_data.clone())
            .await
            .map_err(|e| format!("S3 upload failed: {}", e))?;

        // Set metadata (content-type, cache-control)
        let metadata = self
            .operator
            .metadata(&storage_path, opendal::Metakey::ContentType)
            .await
            .map_err(|e| format!("Failed to set metadata: {}", e))?;

        // Construct public URL
        let public_url = format!(
            "{}/{}",
            self.config.public_url_base.trim_end_matches('/'),
            storage_path
        );

        Ok(StorageResult {
            storage_path,
            public_url,
            size: image_data.len() as u64,
            mime_type,
            hash,
        })
    }

    /// Download image by path
    pub async fn download_image(&self, storage_path: &str) -> Result<Bytes, String> {
        self.operator
            .read(storage_path)
            .await
            .map_err(|e| format!("S3 download failed: {}", e))
    }

    /// Delete image
    pub async fn delete_image(&self, storage_path: &str) -> Result<(), String> {
        self.operator
            .delete(storage_path)
            .await
            .map_err(|e| format!("S3 delete failed: {}", e))
    }

    /// Check if image exists
    pub async fn exists(&self, storage_path: &str) -> Result<bool, String> {
        self.operator
            .is_exist(storage_path)
            .await
            .map_err(|e| format!("S3 exists check failed: {}", e))
    }

    /// Get image metadata
    pub async fn get_metadata(&self, storage_path: &str) -> Result<ImageMetadata, String> {
        let metadata = self
            .operator
            .stat(storage_path)
            .await
            .map_err(|e| format!("Failed to get metadata: {}", e))?;

        Ok(ImageMetadata {
            size: metadata.content_length(),
            content_type: metadata.content_type().map(|s| s.to_string()),
            last_modified: metadata.last_modified(),
        })
    }

    /// List all images for a user
    pub async fn list_user_images(&self, user_id: &Uuid) -> Result<Vec<String>, String> {
        let prefix = format!("images/{}/", user_id);

        let lister = self
            .operator
            .lister(&prefix)
            .await
            .map_err(|e| format!("Failed to list: {}", e))?;

        let mut paths = Vec::new();
        let mut entries = lister;

        while let Some(entry) = entries
            .try_next()
            .await
            .map_err(|e| format!("Failed to iterate: {}", e))?
        {
            paths.push(entry.path().to_string());
        }

        Ok(paths)
    }

    /// Generate presigned URL (for direct browser upload)
    pub async fn generate_presigned_url(
        &self,
        storage_path: &str,
        expires_in_secs: u64,
    ) -> Result<String, String> {
        // OpenDAL doesn't directly support presigned URLs yet,
        // so we use AWS SDK for this specific case
        use aws_sdk_s3::presigning::PresigningConfig;
        use std::time::Duration;

        let aws_config = aws_config::load_from_env().await;
        let s3_client = aws_sdk_s3::Client::new(&aws_config);

        let presigning_config = PresigningConfig::expires_in(Duration::from_secs(expires_in_secs))
            .map_err(|e| format!("Presigning config error: {}", e))?;

        let presigned = s3_client
            .get_object()
            .bucket(&self.config.bucket)
            .key(storage_path)
            .presigned(presigning_config)
            .await
            .map_err(|e| format!("Failed to generate presigned URL: {}", e))?;

        Ok(presigned.uri().to_string())
    }

    /// Compute SHA256 hash for deduplication
    fn compute_hash(&self, data: &Bytes) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        hex::encode(hasher.finalize())
    }

    /// Copy image (for duplication/backup)
    pub async fn copy_image(&self, source_path: &str, dest_path: &str) -> Result<(), String> {
        let data = self.download_image(source_path).await?;
        self.operator
            .write(dest_path, data)
            .await
            .map_err(|e| format!("Copy failed: {}", e))
    }
}

// ============================================================================
// Storage Result Structures
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageResult {
    pub storage_path: String,
    pub public_url: String,
    pub size: u64,
    pub mime_type: String,
    pub hash: String,
}

#[derive(Debug, Clone)]
pub struct ImageMetadata {
    pub size: u64,
    pub content_type: Option<String>,
    pub last_modified: Option<chrono::DateTime<chrono::Utc>>,
}

// ============================================================================
// HTTP Upload Handler
// ============================================================================

use axum::{
    Json,
    extract::{Multipart, State},
    http::StatusCode,
};

#[derive(Debug, Serialize)]
pub struct UploadResponse {
    pub node_id: Uuid,
    pub url: String,
    pub storage_path: String,
    pub size: u64,
}

pub async fn upload_image_handler(
    State(state): State<Arc<crate::AppState>>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, (StatusCode, String)> {
    // Extract from auth context
    let user_id = expect_context::<Uuid>(); // From middleware
    let node_id = Uuid::new_v4();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();

        if name == "image" {
            let filename = field.file_name().unwrap_or("image.jpg").to_string();

            let data = field
                .bytes()
                .await
                .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

            // Upload to S3
            let storage_service = StorageService::new(state.s3_config.clone())
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

            let result = storage_service
                .upload_image(&user_id, &node_id, data, &filename)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

            // Save to database
            sqlx::query!(
                r#"
                INSERT INTO tree_nodes (id, user_id, parent_id, node_type, data)
                VALUES ($1, $2, $3, $4, $5)
                "#,
                node_id,
                user_id,
                None::<Uuid>, // Set parent_id later
                crate::NodeType::ImageLeaf as _,
                serde_json::json!({
                    "url": result.public_url,
                    "storage_path": result.storage_path,
                    "size": result.size,
                    "mime_type": result.mime_type,
                    "hash": result.hash,
                })
            )
            .execute(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            return Ok(Json(UploadResponse {
                node_id,
                url: result.public_url,
                storage_path: result.storage_path,
                size: result.size,
            }));
        }
    }

    Err((StatusCode::BAD_REQUEST, "No image field found".to_string()))
}

// ============================================================================
// Image Processing Service
// ============================================================================

pub struct ImageProcessor {
    pub storage: Arc<StorageService>,
}

impl ImageProcessor {
    pub fn new(storage: Arc<StorageService>) -> Self {
        Self { storage }
    }

    /// Download image from external URL and upload to S3
    pub async fn import_external_image(
        &self,
        user_id: &Uuid,
        node_id: &Uuid,
        external_url: &str,
    ) -> Result<StorageResult, String> {
        // Download from external URL
        let response = reqwest::get(external_url)
            .await
            .map_err(|e| format!("Failed to download: {}", e))?;

        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read bytes: {}", e))?;

        // Extract filename from URL
        let filename = external_url.split('/').last().unwrap_or("image.jpg");

        // Upload to S3
        self.storage
            .upload_image(user_id, node_id, bytes, filename)
            .await
    }

    /// Create thumbnail (requires image crate)
    pub async fn create_thumbnail(
        &self,
        user_id: &Uuid,
        node_id: &Uuid,
        original_path: &str,
        max_width: u32,
        max_height: u32,
    ) -> Result<StorageResult, String> {
        use image::GenericImageView;

        // Download original
        let data = self.storage.download_image(original_path).await?;

        // Load and resize
        let img = image::load_from_memory(&data).map_err(|e| format!("Invalid image: {}", e))?;

        let thumbnail = img.thumbnail(max_width, max_height);

        // Encode as JPEG
        let mut buffer = Vec::new();
        thumbnail
            .write_to(
                &mut std::io::Cursor::new(&mut buffer),
                image::ImageOutputFormat::Jpeg(85),
            )
            .map_err(|e| format!("Failed to encode: {}", e))?;

        // Upload thumbnail
        let thumb_node_id = Uuid::new_v4();
        self.storage
            .upload_image(
                user_id,
                &thumb_node_id,
                Bytes::from(buffer),
                "thumbnail.jpg",
            )
            .await
    }

    /// Validate image before upload
    pub fn validate_image(&self, data: &Bytes, max_size_mb: u64) -> Result<(), String> {
        // Check size
        if data.len() as u64 > max_size_mb * 1024 * 1024 {
            return Err(format!("Image too large (max {}MB)", max_size_mb));
        }

        // Verify it's a valid image
        image::load_from_memory(data).map_err(|e| format!("Invalid image format: {}", e))?;

        Ok(())
    }

    /// Batch upload images
    pub async fn batch_upload(
        &self,
        user_id: &Uuid,
        parent_node_id: &Uuid,
        images: Vec<(String, Bytes)>, // (filename, data)
    ) -> Result<Vec<StorageResult>, String> {
        let mut results = Vec::new();

        for (filename, data) in images {
            let node_id = Uuid::new_v4();

            match self
                .storage
                .upload_image(user_id, &node_id, data, &filename)
                .await
            {
                Ok(result) => results.push(result),
                Err(e) => {
                    log::error!("Failed to upload {}: {}", filename, e);
                    // Continue with other images
                }
            }
        }

        Ok(results)
    }
}

// ============================================================================
// Image URL Resolution Service
// ============================================================================

pub struct ImageUrlResolver {
    pub storage: Arc<StorageService>,
    pub db: sqlx::PgPool,
}

impl ImageUrlResolver {
    /// Get public URL from node_id
    pub async fn resolve_node_url(&self, node_id: &Uuid) -> Result<String, String> {
        let node = sqlx::query!(
            r#"
            SELECT data
            FROM tree_nodes
            WHERE id = $1 AND node_type = 'ImageLeaf'
            "#,
            node_id
        )
        .fetch_one(&self.db)
        .await
        .map_err(|e| format!("Node not found: {}", e))?;

        let data: serde_json::Value = node.data;
        data.get("url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| "URL not found in node data".to_string())
    }

    /// Get multiple URLs efficiently
    pub async fn resolve_batch_urls(
        &self,
        node_ids: &[Uuid],
    ) -> Result<Vec<(Uuid, String)>, String> {
        let nodes = sqlx::query!(
            r#"
            SELECT id, data
            FROM tree_nodes
            WHERE id = ANY($1) AND node_type = 'ImageLeaf'
            "#,
            node_ids
        )
        .fetch_all(&self.db)
        .await
        .map_err(|e| format!("Failed to fetch nodes: {}", e))?;

        Ok(nodes
            .into_iter()
            .filter_map(|node| {
                let url = node
                    .data
                    .get("url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())?;
                Some((node.id, url))
            })
            .collect())
    }

    /// Update node URL after S3 migration
    pub async fn update_node_url(
        &self,
        node_id: &Uuid,
        new_url: &str,
        storage_path: &str,
    ) -> Result<(), String> {
        sqlx::query!(
            r#"
            UPDATE tree_nodes
            SET data = jsonb_set(jsonb_set(data, '{url}', $2), '{storage_path}', $3)
            WHERE id = $1
            "#,
            node_id,
            serde_json::json!(new_url),
            serde_json::json!(storage_path)
        )
        .execute(&self.db)
        .await
        .map_err(|e| format!("Failed to update URL: {}", e))?;

        Ok(())
    }
}

// ============================================================================
// Router Integration
// ============================================================================

use axum::routing::{delete, get, post};

pub fn storage_routes() -> axum::Router<Arc<crate::AppState>> {
    axum::Router::new()
        .route("/api/images/upload", post(upload_image_handler))
        .route("/api/images/:node_id", get(get_image_handler))
        .route("/api/images/:node_id", delete(delete_image_handler))
        .route("/api/images/batch", post(batch_upload_handler))
}

async fn get_image_handler(
    State(state): State<Arc<crate::AppState>>,
    axum::extract::Path(node_id): axum::extract::Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let user_id = expect_context::<Uuid>();

    let node = sqlx::query!(
        r#"
        SELECT data
        FROM tree_nodes
        WHERE id = $1 AND user_id = $2 AND node_type = 'ImageLeaf'
        "#,
        node_id,
        user_id
    )
    .fetch_one(&state.db)
    .await
    .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;

    Ok(Json(node.data))
}

async fn delete_image_handler(
    State(state): State<Arc<crate::AppState>>,
    axum::extract::Path(node_id): axum::extract::Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    let user_id = expect_context::<Uuid>();

    // Get storage path
    let node = sqlx::query!(
        r#"
        SELECT data
        FROM tree_nodes
        WHERE id = $1 AND user_id = $2 AND node_type = 'ImageLeaf'
        "#,
        node_id,
        user_id
    )
    .fetch_one(&state.db)
    .await
    .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;

    let storage_path = node
        .data
        .get("storage_path")
        .and_then(|v| v.as_str())
        .ok_or((StatusCode::BAD_REQUEST, "No storage path".to_string()))?;

    // Delete from S3
    let storage_service = StorageService::new(state.s3_config.clone())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    storage_service
        .delete_image(storage_path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    // Delete from database
    sqlx::query!(
        "DELETE FROM tree_nodes WHERE id = $1 AND user_id = $2",
        node_id,
        user_id
    )
    .execute(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

async fn batch_upload_handler(
    State(state): State<Arc<crate::AppState>>,
    mut multipart: Multipart,
) -> Result<Json<Vec<UploadResponse>>, (StatusCode, String)> {
    let user_id = expect_context::<Uuid>();
    let mut responses = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        if field.name() == Some("images") {
            let filename = field.file_name().unwrap_or("image.jpg").to_string();
            let data = field
                .bytes()
                .await
                .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

            let node_id = Uuid::new_v4();
            let storage_service = StorageService::new(state.s3_config.clone())
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

            if let Ok(result) = storage_service
                .upload_image(&user_id, &node_id, data, &filename)
                .await
            {
                responses.push(UploadResponse {
                    node_id,
                    url: result.public_url.clone(),
                    storage_path: result.storage_path.clone(),
                    size: result.size,
                });
            }
        }
    }

    Ok(Json(responses))
}

// Helper to get user_id from context (implement based on your auth)
fn expect_context<T: Clone + 'static>() -> T {
    // This should extract from axum extension or request context
    unimplemented!("Implement based on your auth middleware")
}
