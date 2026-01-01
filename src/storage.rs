use crate::error::*;
use crate::models::*;
use axum::{
    Json,
    extract::{Multipart, Path, State},
    http::StatusCode,
};
use bytes::Bytes;
use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::region::Region;
use sha2::{Digest, Sha256};
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;
use crate::agents::master_agent::MasterAgent;
use crate::db::NodeType;

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

    /// Upload image to S3
    pub async fn upload_image(
        &self,
        node_id: &Uuid,
        image_data: Bytes,
        filename: &str,
    ) -> Result<StorageResult> {
        let hash = self.compute_hash(&image_data);
        let extension = std::path::Path::new(filename)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("jpg");

        let storage_path = format!("images/{}/{}.{}", node_id, hash, extension);

        let mime_type = mime_guess::from_path(filename)
            .first_or_octet_stream()
            .to_string();

        // Upload with rust-s3
        self.bucket
            .put_object(&storage_path, &image_data)
            .await
            .map_err(|e| {
                AppError::new(ErrorCode::StorageError, format!("S3 upload failed: {}", e))
            })?;

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

    /// Create thumbnail
    pub async fn create_thumbnail(
        &self,
        _node_id: &Uuid,
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
                image::ImageFormat::Jpeg, //   ImageOutputFormat::Jpeg(85),
            )
            .map_err(|e| AppError::internal(format!("Encode failed: {}", e)))?;

        let thumb_node_id = Uuid::now_v7();
        self.storage
            .upload_image(
                &thumb_node_id,
                Bytes::from(buffer),
                "thumbnail.jpg",
            )
            .await
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

#[cfg(test)]
mod tests {
    use crate::init::S3Config;

    #[tokio::test]
    async fn example_usage() {
        dotenv::from_path(".env.test").ok();

        let config = S3Config::from_env().unwrap();
        let storage = crate::init::setup_storage(&config).unwrap();

        let s3_path = "test.file";
        let test = b"I'm going to S3!";

        // Загрузка файла
        let response_data = storage.bucket.put_object(s3_path, test).await.unwrap();
        assert_eq!(response_data.status_code(), 200);
        println!("{:#?}", response_data.headers());

        // Получение файла через SDK
        let response_data = storage.bucket.get_object(s3_path).await.unwrap();
        assert_eq!(response_data.status_code(), 200);
        assert_eq!(test, response_data.as_slice());

        // Получение части файла
        let response_data = storage.bucket.get_object_range(s3_path, 1, Some(10)).await.unwrap();
        assert_eq!(response_data.status_code(), 206);

        // Head запрос
        let (head_object_result, code) = storage.bucket.head_object(s3_path).await.unwrap();
        assert_eq!(code, 200);
        assert_eq!(
            head_object_result.content_type.unwrap_or_default(),
            "application/octet-stream".to_owned()
        );

        // Генерация временной URL (presigned URL)
        let presigned_url = storage.bucket.presign_get(s3_path, 300, None).await.unwrap();
        println!("Presigned URL: {}", presigned_url);

        // Проверка доступности временной URL через HTTP-запрос
        let client = reqwest::Client::new();
        let presigned_response = client.get(&presigned_url).send().await.unwrap();
        assert_eq!(presigned_response.status(), 200);
        let presigned_body = presigned_response.bytes().await.unwrap();
        assert_eq!(test, presigned_body.as_ref());
        println!("✓ Presigned URL works correctly");

        // Генерация публичной URL (если бакет публичный)
        let public_url = format!(
            "{}/{}/{}",
            storage.bucket.url(),
            storage.bucket.name(),
            s3_path
        );
        println!("Public URL: {}", public_url);

        // Попытка доступа к публичной URL
        // Примечание: это работает только если бакет настроен как публичный
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

        // Тест на истечение временной URL (опционально)
        // Генерируем URL с коротким временем жизни
        let short_presigned_url = storage.bucket.presign_get(s3_path, 1, None).await.unwrap();
        println!("Short-lived presigned URL: {}", short_presigned_url);

        // Ждем истечения
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        let expired_response = client.get(&short_presigned_url).send().await.unwrap();
        assert_ne!(expired_response.status(), 200, "Expired URL should not return 200");
        println!("✓ Presigned URL correctly expires");

        // Удаление файла
        let response_data = storage.bucket.delete_object(s3_path).await.unwrap();
        assert_eq!(response_data.status_code(), 204);

        // Проверка, что после удаления URL больше не работают
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

        // Генерация presigned URL для загрузки
        let presigned_put_url = storage.bucket.presign_put(s3_path, 300, None, None).await.unwrap();
        println!("Presigned PUT URL: {}", presigned_put_url);

        // Загрузка через presigned URL
        let client = reqwest::Client::new();
        let upload_response = client
            .put(&presigned_put_url)
            .body(test_data.to_vec())
            .send()
            .await
            .unwrap();

        assert_eq!(upload_response.status(), 200);
        println!("✓ Upload via presigned PUT URL successful");

        // Проверка, что файл действительно загрузился
        let response_data = storage.bucket.get_object(s3_path).await.unwrap();
        assert_eq!(test_data, response_data.as_slice());
        println!("✓ File uploaded correctly");

        // Очистка
        storage.bucket.delete_object(s3_path).await.unwrap();
    }

    #[tokio::test]
    async fn test_presigned_url_with_custom_headers() {
        dotenv::from_path(".env.test").ok();

        let config = S3Config::from_env().unwrap();
        let storage = crate::init::setup_storage(&config).unwrap();

        let s3_path = "test_custom_headers.file";
        let test_data = b"Test with custom headers";

        // Загрузка с custom headers
        let mut custom_headers = std::collections::HashMap::new();
        custom_headers.insert("Content-Type".to_string(), "text/plain".to_string());

        storage.bucket.put_object_with_content_type(s3_path, test_data, "text/plain").await.unwrap();

        // Генерация presigned URL
        let presigned_url = storage.bucket.presign_get(s3_path, 300, None).await.unwrap();

        // Проверка через HTTP
        let client = reqwest::Client::new();
        let response = client.get(&presigned_url).send().await.unwrap();

        assert_eq!(response.status(), 200);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/plain"
        );
        println!("✓ Custom headers preserved correctly");

        // Очистка
        storage.bucket.delete_object(s3_path).await.unwrap();
    }
}