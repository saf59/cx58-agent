use std::collections::HashMap;
use crate::agents::master_agent::MasterAgent;
use crate::db::NodeType;
use crate::error::*;
use crate::init::S3Config;
use crate::models::*;
use axum::{
    extract::{Multipart, Path, State},
    http::StatusCode,
    Json,
};
use bytes::Bytes;
use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::region::Region;
use s3::BucketConfiguration;
use sha2::{Digest, Sha256};
use std::str::FromStr;
use std::sync::Arc;
use axum::http::HeaderMap;
use reqwest::header::HeaderName;
use s3::bucket_ops::CannedBucketAcl;
use uuid::Uuid;

// ============================================================================
// AppState && AiConfig
// ============================================================================
#[derive(Clone)]
pub struct AiConfig {
    pub url: String,
    pub text_model: String,
    pub vision_model: String,
    pub chat_model: String,
}
impl AiConfig {
    pub fn from_env() -> std::result::Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            url: std::env::var("DATABASE_URL").unwrap_or_else(|_| "http://127.0.0.1:11434".to_string()),
            text_model: std::env::var("TEXT_MODEL").unwrap_or_else(|_| "llava".to_string()),
            vision_model: std::env::var("VISION_MODEL").unwrap_or_else(|_| "llama3.2-vision".to_string()),
            chat_model: std::env::var("CHAT_MODEL").unwrap_or_else(|_| "llava".to_string()),
        })
    }
}
#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::PgPool,
    pub storage: Arc<StorageService>,
    pub image_resolver: Arc<ImageUrlResolver>,
    pub image_processor: Arc<ImageProcessor>,
    pub master_agent: Arc<MasterAgent>,
    pub ai_config: AiConfig,
}
//pub redis: redis::aio::ConnectionManager,
//pub agent: Arc<RwLock<AgentExecutor>>,
//pub orchestrator: Arc<crate::rig_integration::AgentOrchestrator>,

// ============================================================================
// Storage Service with rust-s3
// ============================================================================

#[derive(Clone)]
pub struct StorageService {
    bucket: Bucket,
    public_url_base: String,
}

impl StorageService {
    /// Create from explicit configuration
    pub fn new(
        bucket_name: String,
        region: String,
        access_key: String,
        secret_key: String,
        public_url_base: String,
        endpoint: Option<String>,
    ) -> Result<Self> {
        let region = if let Some(ep) = endpoint {
            Region::Custom {
                region: region.clone(),
                endpoint: ep,
            }
        } else {
            Region::from_str(&region)
                .map_err(|e| AppError::internal(format!("Invalid region: {}", e)))?
        };

        let credentials = Credentials::new(Some(&access_key), Some(&secret_key), None, None, None)
            .map_err(|e| AppError::internal(format!("Credentials error: {}", e)))?;

        let mut bucket = Bucket::new(&bucket_name, region, credentials)
            .map_err(|e| AppError::internal(format!("Bucket creation failed: {}", e)))?;

        // Use path-style for compatibility with MinIO/LocalStack
        bucket = bucket.with_path_style();

        Ok(Self {
            bucket: *bucket,
            public_url_base,
        })
    }

    pub async fn upload_thumbnail(
        &self,
        node_id: &Uuid,
        image_data: Bytes,
        filename: &str,
    ) -> Result<StorageResult> {
        self.upload_image_type(node_id,image_data,filename,true).await
    }

    pub async fn upload_image(
        &self,
        node_id: &Uuid,
        image_data: Bytes,
        filename: &str,
    ) -> Result<StorageResult> {
        self.upload_image_type(node_id,image_data,filename,false).await
    }

    /// Upload image to S3
    pub async fn upload_image_type(
        &self,
        node_id: &Uuid,
        image_data: Bytes,
        filename: &str,
        is_thumbnail: bool
    ) -> Result<StorageResult> {
        let hash = self.compute_hash(&image_data);
        let extension = std::path::Path::new(filename)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("jpg");
        let image_type = if is_thumbnail {"thumbnails"} else {"images"};

        let storage_path = format!("{}/{}/{}.{}", image_type, node_id, hash, extension);

        let mime_type = mime_guess::from_path(filename)
            .first_or_octet_stream()
            .to_string();

        // Upload with rust-s3
        if is_thumbnail {
            let mut headers = HeaderMap::new();
            // set Public accss
            headers.insert(HeaderName::from_static("x-amz-acl"),
                           "public-read".parse().unwrap());

            let _responce_data = self.bucket
                .put_object_with_content_type_and_headers(
                    &storage_path,
                    &image_data,
                    &mime_type,
                    Some(headers)
                )
                .await
                .map_err(|e| {
                    AppError::new(ErrorCode::StorageError, format!("S3 upload failed: {}", e))
                })?;
        }
        else {
            self.bucket
                .put_object(&storage_path, &image_data)
                .await
                .map_err(|e| {
                    AppError::new(ErrorCode::StorageError, format!("S3 upload failed: {}", e))
                })?;
        }

        let public_url = format!(
            "{}/{}",
            self.public_url_base.trim_end_matches('/'),
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

    /// Download image from S3
    pub async fn download_image(&self, storage_path: &str) -> Result<Bytes> {
        let response = self.bucket.get_object(storage_path).await.map_err(|e| {
            AppError::new(
                ErrorCode::StorageError,
                format!("S3 download failed: {}", e),
            )
        })?;

        Ok(Bytes::from(response.bytes().to_vec()))
    }

    /// Delete image from S3
    pub async fn delete_image(&self, storage_path: &str) -> Result<()> {
        self.bucket.delete_object(storage_path).await.map_err(|e| {
            AppError::new(ErrorCode::StorageError, format!("S3 delete failed: {}", e))
        })?;

        Ok(())
    }

    /// Check if object exists
    pub async fn exists(&self, storage_path: &str) -> Result<bool> {
        match self.bucket.head_object(storage_path).await {
            Ok(_) => Ok(true),
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("404") || err_str.contains("NotFound") {
                    Ok(false)
                } else {
                    Err(AppError::new(
                        ErrorCode::StorageError,
                        format!("S3 head failed: {}", e),
                    ))
                }
            }
        }
    }

    /// Get object metadata
    pub async fn get_metadata(&self, storage_path: &str) -> Result<ImageMetadata> {
        let response = self.bucket.head_object(storage_path).await.map_err(|e| {
            AppError::new(ErrorCode::StorageError, format!("S3 head failed: {}", e))
        })?;

        Ok(ImageMetadata {
            size: response.0.content_length.unwrap_or(0) as u64,
            content_type: response.0.content_type,
            last_modified: response.0.last_modified,
        })
    }

    /// List all user images
    pub async fn list_user_images(&self, user_id: &str) -> Result<Vec<String>> {
        let prefix = format!("images/{}/", user_id);

        let results = self.bucket.list(prefix, None).await.map_err(|e| {
            AppError::new(ErrorCode::StorageError, format!("S3 list failed: {}", e))
        })?;

        let mut paths = Vec::new();
        for result in results {
            for object in result.contents {
                paths.push(object.key);
            }
        }

        Ok(paths)
    }

    /// Generate presigned URL (for downloads)
    pub async fn generate_presigned_url(
        &self,
        storage_path: &str,
        expires_in_secs: u32,
    ) -> Result<String> {
        let url = self
            .bucket
            .presign_get(storage_path, expires_in_secs, None)
            .await
            .map_err(|e| AppError::internal(format!("Presigned URL failed: {}", e)))?;

        Ok(url)
    }

    /// Copy object within bucket
    pub async fn copy_image(&self, source_path: &str, dest_path: &str) -> Result<()> {
        // Download then upload (rust-s3 doesn't have native copy)
        let data = self.download_image(source_path).await?;

        self.bucket
            .put_object(dest_path, &data)
            .await
            .map_err(|e| {
                AppError::new(ErrorCode::StorageError, format!("S3 copy failed: {}", e))
            })?;

        Ok(())
    }

    /// Batch delete
    pub async fn delete_batch(&self, paths: Vec<String>) -> Result<Vec<String>> {
        let mut deleted = Vec::new();

        for path in paths {
            match self.bucket.delete_object(&path).await {
                Ok(_) => deleted.push(path),
                Err(e) => {
                    log::warn!("Failed to delete {}: {}", path, e);
                }
            }
        }

        Ok(deleted)
    }
    pub async fn object_exists(&self, path: &str) -> Result<bool> {
        match self.bucket.head_object(path).await {
            Ok(_) => Ok(true),
            Err(e) => {
                // Проверяем, это ошибка "не найден" или что-то другое
                let err_str = e.to_string();
                if err_str.contains("404") || err_str.contains("NoSuchKey") {
                    Ok(false)
                } else {
                    Err(AppError::new(
                        ErrorCode::StorageError,
                        format!("S3 head_object failed: {}", e),
                    ))
                }
            }
        }
    }
    /// Compute SHA256 hash
    fn compute_hash(&self, data: &Bytes) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        hex::encode(hasher.finalize())
    }
}

// ============================================================================
// Image Processor
// ============================================================================

pub struct ImageProcessor {
    pub storage: Arc<StorageService>,
}

impl ImageProcessor {
    pub fn new(storage: Arc<StorageService>) -> Self {
        Self { storage }
    }

    /// Import external image
    pub async fn import_external_image(
        &self,
        node_id: &Uuid,
        external_url: &str,
    ) -> Result<StorageResult> {
        let response = reqwest::get(external_url)
            .await
            .map_err(|e| AppError::internal(format!("Download failed: {}", e)))?;

        let bytes = response
            .bytes()
            .await
            .map_err(|e| AppError::internal(format!("Read failed: {}", e)))?;

        let filename = external_url.split('/').next_back().unwrap_or("image.jpg");

        self.storage
            .upload_image(node_id, bytes, filename)
            .await
    }

    /// Create thumbnail - with original
    pub async fn create_thumbnail(
        &self,
        node_id: &Uuid,
        original_path: &str,
        max_width: u32,
        max_height: u32,
    ) -> Result<StorageResult> {
        let data = self.storage.download_image(original_path).await?;
        let img = image::load_from_memory(&data)
            .map_err(|e| AppError::internal(format!("Invalid image: {}", e)))?;

        let thumbnail = img.thumbnail(max_width, max_height);

        let mut buffer = Vec::new();
        thumbnail
            .write_to(
                &mut std::io::Cursor::new(&mut buffer),
                image::ImageFormat::Jpeg,
            )
            .map_err(|e| AppError::internal(format!("Encode failed: {}", e)))?;

        // Извлекаем имя файла из original_path
        // Формат: images/{node_id}/{hash}.{ext}
        let filename = original_path
            .split('/')
            .last()
            .ok_or_else(|| AppError::internal("Invalid original path"))?;

        self.storage.upload_image_type(
            node_id,
            Bytes::from(buffer),
            filename,
            true  // is_thumbnail = true
        )
            .await
    }

    fn get_thumbnail_path(&self, original_path: &str) -> Option<String> {
        // Преобразуем images/{node_id}/{hash}.{ext} -> thumbnails/{node_id}/{hash}.{ext}
        if original_path.starts_with("images/") {
            Some(original_path.replacen("images/", "thumbnails/", 1))
        } else {
            None
        }
    }

    async fn object_exists(&self, path: &str) -> Result<bool> {
        self.storage.object_exists(path).await
    }

    pub async fn get_or_create_thumbnail(
        &self,
        node_id: &Uuid,
        original_path: &str,
        max_width: u32,
        max_height: u32,
    ) -> Result<StorageResult> {
        // Получаем путь к thumbnail
        let thumbnail_path = self.get_thumbnail_path(original_path)
            .ok_or_else(|| AppError::internal("Invalid original path format"))?;

        // Проверяем, существует ли thumbnail
        if self.object_exists(&thumbnail_path).await? {
            // Thumbnail существует - возвращаем его метаданные
            let data = self.storage.download_image(&thumbnail_path).await?;
            let hash = self.storage.compute_hash(&data);

            let extension = std::path::Path::new(&thumbnail_path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("jpg");

            let mime_type = mime_guess::from_path(&thumbnail_path)
                .first_or_octet_stream()
                .to_string();

            let public_url = format!(
                "{}/{}",
                self.storage.public_url_base.trim_end_matches('/'),
                thumbnail_path
            );

            Ok(StorageResult {
                storage_path: thumbnail_path,
                public_url,
                size: data.len() as u64,
                mime_type,
                hash,
            })
        } else {
            // Thumbnail не существует - создаем его
            self.create_thumbnail(node_id, original_path, max_width, max_height).await
        }
    }

    /// Validate image
    pub fn validate_image(&self, data: &Bytes, max_size_mb: u64) -> Result<()> {
        if data.len() as u64 > max_size_mb * 1024 * 1024 {
            return Err(AppError::bad_request(format!(
                "Image too large (max {}MB)",
                max_size_mb
            )));
        }

        image::load_from_memory(data)
            .map_err(|e| AppError::validation(format!("Invalid image: {}", e)))?;

        Ok(())
    }
}

// ============================================================================
// URL Resolver
// ============================================================================

pub struct ImageUrlResolver {
    pub storage: Arc<StorageService>,
    pub db: sqlx::PgPool,
}

impl ImageUrlResolver {
    pub async fn resolve_node_url(&self, node_id: &Uuid) -> Result<String> {
        let node = sqlx::query!(
            r#"SELECT data FROM tree_nodes WHERE id = $1 AND node_type = 'ImageLeaf'"#,
            node_id
        )
            .fetch_one(&self.db)
            .await?;

        let data: serde_json::Value = node.data;
        data.get("url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| AppError::not_found("URL"))
    }

    pub async fn resolve_batch_urls(&self, node_ids: &[Uuid]) -> Result<Vec<(Uuid, String)>> {
        let nodes = sqlx::query!(
            r#"SELECT id, data FROM tree_nodes WHERE id = ANY($1) AND node_type = 'ImageLeaf'"#,
            node_ids
        )
            .fetch_all(&self.db)
            .await?;

        Ok(nodes
            .into_iter()
            .filter_map(|node| {
                let url = node.data.get("url")?.as_str()?.to_string();
                Some((node.id, url))
            })
            .collect())
    }
}

// ============================================================================
// HTTP Handlers
// ============================================================================

pub async fn upload_image_handler(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>> {
    let node_id = Uuid::now_v7();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad_request(format!("Multipart: {}", e)))?
    {
        if field.name() == Some("image") {
            let filename = field
                .file_name()
                .ok_or_else(|| AppError::bad_request("Missing filename"))?
                .to_string();

            let data = field
                .bytes()
                .await
                .map_err(|e| AppError::bad_request(format!("Read: {}", e)))?;

            state.image_processor.validate_image(&data, 10)?;

            let result = state
                .storage
                .upload_image(&node_id, data, &filename)
                .await?;

            sqlx::query!(
                r#"
                INSERT INTO tree_nodes (id, parent_id, node_type, data)
                VALUES ($1, $2, $3, $4)
                "#,
                node_id,      // TODO id is auto!!
                None::<Uuid>, // TODO it must be!
                NodeType::ImageLeaf as NodeType , //"node_type_enum: ImageLeaf", // NodeType::ImageLeaf
                serde_json::json!({
                    "url": result.public_url,
                    "storage_path": result.storage_path,
                    "size": result.size,
                    "mime_type": result.mime_type,
                    "hash": result.hash,
                })
            )
                .execute(&state.db)
                .await?;

            return Ok(Json(UploadResponse {
                node_id,
                url: result.public_url,
                storage_path: result.storage_path,
                size: result.size,
            }));
        }
    }

    Err(AppError::bad_request("No image field"))
}

pub async fn get_image_handler(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>> {
    let node = sqlx::query!(
        r#"SELECT data FROM tree_nodes WHERE id = $1 AND node_type = 'ImageLeaf'"#,
        node_id
    )
        .fetch_one(&state.db)
        .await?;

    Ok(Json(node.data))
}

pub async fn delete_image_handler(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<Uuid>,
) -> Result<StatusCode> {

    let node = sqlx::query!(
        r#"SELECT data FROM tree_nodes WHERE id = $1 AND node_type = 'ImageLeaf'"#,
        node_id,
    )
        .fetch_one(&state.db)
        .await?;

    let storage_path = node
        .data
        .get("storage_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::bad_request("No storage path"))?;

    state.storage.delete_image(storage_path).await?;

    sqlx::query!("DELETE FROM tree_nodes WHERE id = $1", node_id)
        .execute(&state.db)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn batch_upload_handler(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<Vec<UploadResponse>>> {
    let mut responses = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad_request(format!("Multipart: {}", e)))?
    {
        if field.name() == Some("images") {
            let filename = field.file_name().unwrap_or("image.jpg").to_string();
            let data = field
                .bytes()
                .await
                .map_err(|e| AppError::bad_request(format!("Read: {}", e)))?;

            let node_id = Uuid::now_v7();

            if let Ok(result) = state
                .storage
                .upload_image(&node_id, data, &filename)
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
/// Создание bucket с публичным доступом для чтения
pub async fn create_public_bucket(config: &S3Config,bucket_name: &str) -> Result<Box<Bucket>> {
    let region = Region::Custom {
        region: config.region.clone(),
        endpoint: config.endpoint.clone().unwrap(),
    };

    let credentials = Credentials::new(
        Some(&config.access_key),
        Some(&config.secret_key),
        None,
        None,
        None,
    ).map_err( |_| {AppError::unauthorized("Bad S3 credentials".to_string()) })?;

    let create_bucket_config = BucketConfiguration::public();

    let response = Bucket::create_with_path_style(
        bucket_name,
        region.clone(),
        credentials.clone(),
        create_bucket_config,
    ).await.unwrap();

    println!("✓ Public bucket '{}' created with code {}\n{}", bucket_name, &response.response_code, &response.response_text );

    Ok(response.bucket)
}

#[cfg(test)]
mod tests {
    use crate::init::S3Config;
    use crate::storage::create_public_bucket;
    use bytes::Bytes;
    use std::fs;
    use uuid::Uuid;

    #[tokio::test]
    async fn example_usage() {
        dotenv::from_path(".env.test").ok();

        let config = S3Config::from_env().unwrap();
        let storage = crate::init::setup_storage(&config).unwrap();

        let s3_path = "test.file";
        let test = b"I'm going to S3!";

        // Upload file
        let response_data = storage.bucket.put_object(s3_path, test).await.unwrap();
        assert_eq!(response_data.status_code(), 200);
        println!("{:#?}", response_data.headers());

        // Get file via SDK
        let response_data = storage.bucket.get_object(s3_path).await.unwrap();
        assert_eq!(response_data.status_code(), 200);
        assert_eq!(test, response_data.as_slice());

        // Get file range
        let response_data = storage.bucket.get_object_range(s3_path, 1, Some(10)).await.unwrap();
        assert_eq!(response_data.status_code(), 206);

        // Head request
        let (head_object_result, code) = storage.bucket.head_object(s3_path).await.unwrap();
        assert_eq!(code, 200);
        assert_eq!(
            head_object_result.content_type.unwrap_or_default(),
            "application/octet-stream".to_owned()
        );

        // Generate presigned URL
        let presigned_url = storage.bucket.presign_get(s3_path, 300, None).await.unwrap();
        println!("Presigned URL: {}", presigned_url);

        // Verify presigned URL via HTTP request
        let client = reqwest::Client::new();
        let presigned_response = client.get(&presigned_url).send().await.unwrap();
        assert_eq!(presigned_response.status(), 200);
        let presigned_body = presigned_response.bytes().await.unwrap();
        assert_eq!(test, presigned_body.as_ref());
        println!("✓ Presigned URL works correctly");

        // Generate public URL (if bucket is public)
        let public_url = format!(
            "{}/{}/{}",
            storage.bucket.url(),
            storage.bucket.name(),
            s3_path
        );
        println!("Public URL: {}", public_url);

        // Try to access public URL
        // Note: this only works if bucket is configured as public
        let public_response = client.get(&public_url).send().await;
        match public_response {
            Ok(resp) => {
                if resp.status() == 200 {
                    let public_body = resp.bytes().await.unwrap();
                    assert_eq!(test, public_body.as_ref());
                    println!("✓ Public URL works correctly");
                } else {
                    println!("⚠ Public URL returned status: {} (bucket might not be public)", resp.status());
                }
            }
            Err(e) => {
                println!("⚠ Public URL access failed: {} (bucket might not be public)", e);
            }
        }

        // Test presigned URL expiration (optional)
        // Generate URL with short lifetime
        let short_presigned_url = storage.bucket.presign_get(s3_path, 1, None).await.unwrap();
        println!("Short-lived presigned URL: {}", short_presigned_url);

        // Wait for expiration
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        let expired_response = client.get(&short_presigned_url).send().await.unwrap();
        assert_ne!(expired_response.status(), 200, "Expired URL should not return 200");
        println!("✓ Presigned URL correctly expires");

        // Delete file
        let response_data = storage.bucket.delete_object(s3_path).await.unwrap();
        assert_eq!(response_data.status_code(), 204);

        // Verify that URLs no longer work after deletion
        let deleted_response = client.get(&presigned_url).send().await.unwrap();
        assert_eq!(deleted_response.status(), 404);
        println!("✓ URLs return 404 after object deletion");
    }

    #[tokio::test]
    async fn test_presigned_put_url() {
        dotenv::from_path(".env.test").ok();

        let config = S3Config::from_env().unwrap();
        let storage = crate::init::setup_storage(&config).unwrap();

        let s3_path = "test_presigned_upload.file";
        let test_data = b"Uploaded via presigned URL!";

        // Generate presigned URL for upload
        let presigned_put_url = storage.bucket.presign_put(s3_path, 300, None, None).await.unwrap();
        println!("Presigned PUT URL: {}", presigned_put_url);

        // Upload via presigned URL
        let client = reqwest::Client::new();
        let upload_response = client
            .put(&presigned_put_url)
            .body(test_data.to_vec())
            .send()
            .await
            .unwrap();

        assert_eq!(upload_response.status(), 200);
        println!("✓ Upload via presigned PUT URL successful");

        // Verify that file was actually uploaded
        let response_data = storage.bucket.get_object(s3_path).await.unwrap();
        assert_eq!(test_data, response_data.as_slice());
        println!("✓ File uploaded correctly");

        // Cleanup
        storage.bucket.delete_object(s3_path).await.unwrap();
    }

    #[tokio::test]
    async fn test_presigned_url_with_custom_headers() {
        dotenv::from_path(".env.test").ok();

        let config = S3Config::from_env().unwrap();
        let storage = crate::init::setup_storage(&config).unwrap();

        let s3_path = "test_custom_headers.file";
        let test_data = b"Test with custom headers";

        // Upload with custom headers
        storage.bucket.put_object_with_content_type(s3_path, test_data, "text/plain").await.unwrap();

        // Generate presigned URL
        let presigned_url = storage.bucket.presign_get(s3_path, 3000, None).await.unwrap();

        // Verify via HTTP
        let client = reqwest::Client::new();
        let response = client.get(&presigned_url).send().await.unwrap();

        assert_eq!(response.status(), 200);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/plain"
        );
        println!("✓ Custom headers preserved correctly");

        // Cleanup
        storage.bucket.delete_object(s3_path).await.unwrap();
    }

    #[tokio::test]
    async fn test_presigned_url_through_proxy() {
        dotenv::from_path(".env.test").ok();
        let config = S3Config::from_env().unwrap();
        let storage = crate::init::setup_storage(&config).unwrap();

        let s3_path = "test.file";
        let test_data = b"Test data";

        let response = storage.bucket.put_object(s3_path, test_data).await.unwrap();
        println!("Put response code: {}", response.status_code());
        assert_eq!(response.status_code(), 200);

        let presigned_url = storage.bucket
            .presign_get(s3_path, 300, None)
            .await
            .unwrap();

        println!("Generated presigned URL: {}", presigned_url);

        let client = reqwest::Client::new();
        let response = client.get(&presigned_url).send().await.unwrap();

        println!("Response status: {}", response.status());
        println!("Response headers: {:?}", response.headers());

        if response.status() == 404 {
            println!("URL path in request: {:?}", response.url());
        }

        assert_eq!(response.status(), 200);

        storage.bucket.delete_object(s3_path).await.unwrap();
    }

    #[tokio::test]
    async fn test_create() {
        dotenv::from_path(".env.test").ok();
        let config = S3Config::from_env().unwrap();
        let bucket_name = &config.bucket;
        let _bucket = create_public_bucket(&config, bucket_name).await.unwrap();
    }

    #[tokio::test]
    async fn test_upload_image() {
        dotenv::from_path(".env.test").ok();
        let config = S3Config::from_env().unwrap();
        let storage = crate::init::setup_storage(&config).unwrap();

        // Read the test image file
        let image_data = fs::read("data/noise_1.jpg").expect("Failed to read test image");
        let image_bytes = Bytes::from(image_data);
        let node_id = Uuid::now_v7();
        let filename = "noise_1.jpg";

        // Upload the image (not a thumbnail)
        let result = storage
            .upload_image_type(&node_id, image_bytes, filename, false)
            .await
            .expect("Failed to upload image");

        println!("{:#?}", &result);

        // Verify the upload result
        assert_eq!(result.size, 527174); // Expected size of noise_1.jpg
        assert_eq!(result.mime_type, "image/jpeg");
        assert!(result.storage_path.starts_with(&format!("images/{}", node_id)));
        assert!(result.public_url.contains(&format!("images/{}", node_id)));
        assert!(!result.hash.is_empty());
        assert_eq!(result.hash.len(), 64); // SHA256 hash length

        // Verify the file exists
        let exists = storage.object_exists(&result.storage_path).await.unwrap();
        assert!(exists);

        // Cleanup - delete the uploaded file
        storage.bucket.delete_object(&result.storage_path).await.unwrap();
    }

    #[tokio::test]
    async fn test_upload_thumbnail() {
        dotenv::from_path(".env.test").ok();

        let config = S3Config::from_env().unwrap();
        let storage = crate::init::setup_storage(&config).unwrap();

        // Read the test image file
        let image_data = fs::read("data/noise_1.jpg").expect("Failed to read test image");
        let image_bytes = Bytes::from(image_data);
        let node_id = Uuid::now_v7();
        let filename = "noise_1.jpg";

        // Upload as thumbnail (public)
        let result = storage
            .upload_image_type(&node_id, image_bytes, filename, true)
            .await
            .expect("Failed to upload thumbnail");

        println!("{:#?}", &result);

        // Verify the upload result
        assert_eq!(result.mime_type, "image/jpeg");
        assert!(result.storage_path.starts_with(&format!("thumbnails/{}", node_id)));
        assert!(result.public_url.contains(&format!("thumbnails/{}", node_id)));
        assert!(!result.hash.is_empty());

        // Verify the file exists
        let exists = storage.object_exists(&result.storage_path).await.unwrap();
        assert!(exists);

        // Verify that thumbnail is publicly accessible
        let client = reqwest::Client::new();
        let public_response = client.get(&result.public_url).send().await;

        match public_response {
            Ok(resp) => {
                if resp.status() == 200 {
                    println!("✓ Thumbnail is publicly accessible");
                } else {
                    println!("⚠ Thumbnail URL returned status: {}", resp.status());
                }
            }
            Err(e) => {
                println!("⚠ Thumbnail public access failed: {}", e);
            }
        }

        // Cleanup
        storage.bucket.delete_object(&result.storage_path).await.unwrap();
    }

    #[tokio::test]
    async fn test_upload_image_with_different_extensions() {
        dotenv::from_path(".env.test").ok();

        let config = S3Config::from_env().unwrap();
        let storage = crate::init::setup_storage(&config).unwrap();

        // Test with PNG file (using the same data for simplicity)
        let image_data = fs::read("data/noise_1.jpg").expect("Failed to read test image");
        let image_bytes = Bytes::from(image_data);
        let node_id = Uuid::now_v7();

        // Upload as PNG
        let result = storage
            .upload_image_type(&node_id, image_bytes, "test.png", false)
            .await
            .expect("Failed to upload image as PNG");

        assert_eq!(result.mime_type, "image/png");
        assert!(result.storage_path.ends_with(".png"));

        // Cleanup
        storage.bucket.delete_object(&result.storage_path).await.unwrap();
    }

    #[tokio::test]
    async fn test_create_thumbnail() {
        dotenv::from_path(".env.test").ok();

        let config = S3Config::from_env().unwrap();
        let storage = crate::init::setup_storage(&config).unwrap();
        let image_processor = crate::storage::ImageProcessor::new(storage.clone());

        // First upload an image to create a thumbnail from
        let image_data = fs::read("data/noise_1.jpg").expect("Failed to read test image");
        let image_bytes = Bytes::from(image_data);
        let node_id = Uuid::now_v7();
        let filename = "noise_1.jpg";

        // Upload the original image (private)
        let upload_result = storage
            .upload_image_type(&node_id, image_bytes, filename, false)
            .await
            .expect("Failed to upload original image");

        println!("Original image: {:#?}", upload_result);

        // Create thumbnail from the original
        let thumbnail_result = image_processor
            .create_thumbnail(&node_id, &upload_result.storage_path, 100, 100)
            .await
            .expect("Failed to create thumbnail");

        println!("Thumbnail: {:#?}", thumbnail_result);

        // Verify thumbnail properties
        assert_eq!(thumbnail_result.mime_type, "image/jpeg");
        assert!(thumbnail_result.storage_path.starts_with("thumbnails/"));
        assert!(thumbnail_result.storage_path.contains(&node_id.to_string()));

        // Extract filename from original path and verify it's used in thumbnail
        let original_filename = upload_result.storage_path
            .split('/')
            .last()
            .unwrap();
        assert!(thumbnail_result.storage_path.ends_with(original_filename));

        assert!(thumbnail_result.size > 0);
        assert!(!thumbnail_result.hash.is_empty());
        assert_eq!(thumbnail_result.hash.len(), 64); // SHA256 hash length

        // Verify the thumbnail exists
        let exists = storage.object_exists(&thumbnail_result.storage_path).await.unwrap();
        assert!(exists);

        // Cleanup - delete both original and thumbnail
        storage.bucket.delete_object(&upload_result.storage_path).await.unwrap();
        storage.bucket.delete_object(&thumbnail_result.storage_path).await.unwrap();
    }

    #[tokio::test]
    async fn test_get_or_create_thumbnail() {
        dotenv::from_path(".env.test").ok();

        let config = S3Config::from_env().unwrap();
        let storage = crate::init::setup_storage(&config).unwrap();
        let image_processor = crate::storage::ImageProcessor::new(storage.clone());

        // Upload original image
        let image_data = fs::read("data/noise_1.jpg").expect("Failed to read test image");
        let image_bytes = Bytes::from(image_data);
        let node_id = Uuid::now_v7();
        let filename = "noise_1.jpg";

        let upload_result = storage
            .upload_image_type(&node_id, image_bytes, filename, false)
            .await
            .expect("Failed to upload original image");

        println!("Original image: {:#?}", upload_result);

        // First call - should create thumbnail
        let thumbnail_result_1 = image_processor
            .get_or_create_thumbnail(&node_id, &upload_result.storage_path, 100, 100)
            .await
            .expect("Failed to get or create thumbnail");

        println!("First call (created): {:#?}", thumbnail_result_1);

        // Verify thumbnail was created
        assert!(thumbnail_result_1.storage_path.starts_with("thumbnails/"));
        let thumbnail_path = thumbnail_result_1.storage_path.clone();

        // Second call - should return existing thumbnail without recreation
        let thumbnail_result_2 = image_processor
            .get_or_create_thumbnail(&node_id, &upload_result.storage_path, 100, 100)
            .await
            .expect("Failed to get existing thumbnail");

        println!("Second call (retrieved): {:#?}", thumbnail_result_2);

        // Verify same thumbnail is returned
        assert_eq!(thumbnail_result_1.storage_path, thumbnail_result_2.storage_path);
        assert_eq!(thumbnail_result_1.hash, thumbnail_result_2.hash);

        // Verify thumbnail exists
        let exists = storage.object_exists(&thumbnail_path).await.unwrap();
        assert!(exists);

        // Cleanup
        storage.bucket.delete_object(&upload_result.storage_path).await.unwrap();
        storage.bucket.delete_object(&thumbnail_path).await.unwrap();
    }

    #[tokio::test]
    async fn test_thumbnail_path_conversion() {
        dotenv::from_path(".env.test").ok();

        let config = S3Config::from_env().unwrap();
        let storage = crate::init::setup_storage(&config).unwrap();
        let image_processor = crate::storage::ImageProcessor::new(storage.clone());

        let node_id = Uuid::now_v7();
        let hash = "abc123def456";
        let original_path = format!("images/{}/{}.jpg", node_id, hash);

        // Test path conversion
        let thumbnail_path = image_processor
            .get_thumbnail_path(&original_path)
            .expect("Failed to convert path");

        let expected_path = format!("thumbnails/{}/{}.jpg", node_id, hash);
        assert_eq!(thumbnail_path, expected_path);

        println!("✓ Path conversion works correctly");
        println!("  Original:  {}", original_path);
        println!("  Thumbnail: {}", thumbnail_path);
    }

    #[tokio::test]
    async fn test_object_exists() {
        dotenv::from_path(".env.test").ok();

        let config = S3Config::from_env().unwrap();
        let storage = crate::init::setup_storage(&config).unwrap();

        let s3_path = "test_exists.file";
        let test_data = b"Test data for existence check";

        // File should not exist initially
        let exists_before = storage.object_exists(s3_path).await.unwrap();
        assert!(!exists_before);

        // Upload file
        storage.bucket.put_object(s3_path, test_data).await.unwrap();

        // File should exist now
        let exists_after = storage.object_exists(s3_path).await.unwrap();
        assert!(exists_after);

        // Cleanup
        storage.bucket.delete_object(s3_path).await.unwrap();

        // File should not exist after deletion
        let exists_deleted = storage.object_exists(s3_path).await.unwrap();
        assert!(!exists_deleted);

        println!("✓ Object existence check works correctly");
    }

    #[tokio::test]
    async fn test_download_image() {
        dotenv::from_path(".env.test").ok();

        let config = S3Config::from_env().unwrap();
        let storage = crate::init::setup_storage(&config).unwrap();

        // Upload test image
        let image_data = fs::read("data/noise_1.jpg").expect("Failed to read test image");
        let original_bytes = Bytes::from(image_data.clone());
        let node_id = Uuid::now_v7();
        let filename = "noise_1.jpg";

        let upload_result = storage
            .upload_image_type(&node_id, original_bytes, filename, false)
            .await
            .expect("Failed to upload image");

        // Download the image
        let downloaded_bytes = storage
            .download_image(&upload_result.storage_path)
            .await
            .expect("Failed to download image");

        // Verify downloaded data matches original
        assert_eq!(downloaded_bytes.len(), image_data.len());
        assert_eq!(downloaded_bytes.as_ref(), image_data.as_slice());

        println!("✓ Image download works correctly");

        // Cleanup
        storage.bucket.delete_object(&upload_result.storage_path).await.unwrap();
    }
}