// backend/src/storage.rs - Complete S3 implementation with aws-sdk-s3

use aws_sdk_s3::{Client as S3Client, primitives::ByteStream};
use bytes::Bytes;
use sha2::{Sha256, Digest};
use std::sync::Arc;
use ai_agent_shared::*;
use axum::{
    extract::{Multipart, State, Path},
    http::StatusCode,
    Json,
};
use uuid::Uuid;

// ============================================================================
// Storage Service
// ============================================================================

#[derive(Clone)]
pub struct StorageService {
    client: S3Client,
    bucket: String,
    public_url_base: String,
}

impl StorageService {
    /// Create from environment variables
    pub async fn from_env(bucket: String, public_url_base: String) -> Result<Self> {
        let config = aws_config::load_from_env().await;
        let client = S3Client::new(&config);

        Ok(Self {
            client,
            bucket,
            public_url_base,
        })
    }

    /// Create with explicit credentials
    pub async fn new(
        bucket: String,
        region: String,
        access_key: String,
        secret_key: String,
        public_url_base: String,
        endpoint: Option<String>,
    ) -> Result<Self> {
        use aws_sdk_s3::config::{Credentials, Region};
        
        let creds = Credentials::new(
            access_key,
            secret_key,
            None,
            None,
            "manual",
        );

        let mut config_builder = aws_sdk_s3::Config::builder()
            .region(Region::new(region))
            .credentials_provider(creds);

        if let Some(ep) = endpoint {
            config_builder = config_builder.endpoint_url(ep);
        }

        let client = S3Client::from_conf(config_builder.build());

        Ok(Self {
            client,
            bucket,
            public_url_base,
        })
    }

    pub async fn upload_image(
        &self,
        user_id: &Uuid,
        node_id: &Uuid,
        image_data: Bytes,
        filename: &str,
    ) -> Result<StorageResult> {
        let hash = self.compute_hash(&image_data);
        let extension = std::path::Path::new(filename)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("jpg");
        
        let storage_path = format!(
            "images/{}/{}/{}.{}",
            user_id, node_id, hash, extension
        );

        let mime_type = mime_guess::from_path(filename)
            .first_or_octet_stream()
            .to_string();

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&storage_path)
            .body(ByteStream::from(image_data.clone()))
            .content_type(&mime_type)
            .send()
            .await
            .map_err(|e| AppError::new(
                ErrorCode::StorageError,
                format!("S3 upload failed: {}", e)
            ))?;

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

    pub async fn download_image(&self, storage_path: &str) -> Result<Bytes> {
        let response = self.client
            .get_object()
            .bucket(&self.bucket)
            .key(storage_path)
            .send()
            .await
            .map_err(|e| AppError::new(
                ErrorCode::StorageError,
                format!("S3 download failed: {}", e)
            ))?;

        let bytes = response
            .body
            .collect()
            .await
            .map_err(|e| AppError::internal(format!("Failed to read body: {}", e)))?
            .into_bytes();

        Ok(bytes)
    }

    pub async fn delete_image(&self, storage_path: &str) -> Result<()> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(storage_path)
            .send()
            .await
            .map_err(|e| AppError::new(
                ErrorCode::StorageError,
                format!("S3 delete failed: {}", e)
            ))?;

        Ok(())
    }

    pub async fn exists(&self, storage_path: &str) -> Result<bool> {
        match self.client
            .head_object()
            .bucket(&self.bucket)
            .key(storage_path)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(e) => {
                if e.to_string().contains("404") || e.to_string().contains("NotFound") {
                    Ok(false)
                } else {
                    Err(AppError::new(
                        ErrorCode::StorageError,
                        format!("S3 head failed: {}", e)
                    ))
                }
            }
        }
    }

    pub async fn get_metadata(&self, storage_path: &str) -> Result<ImageMetadata> {
        let response = self.client
            .head_object()
            .bucket(&self.bucket)
            .key(storage_path)
            .send()
            .await
            .map_err(|e| AppError::new(
                ErrorCode::StorageError,
                format!("S3 head failed: {}", e)
            ))?;

        Ok(ImageMetadata {
            size: response.content_length().unwrap_or(0) as u64,
            content_type: response.content_type().map(|s| s.to_string()),
            last_modified: response.last_modified().map(|dt| dt.to_string()),
        })
    }

    pub async fn list_user_images(&self, user_id: &Uuid) -> Result<Vec<String>> {
        let prefix = format!("images/{}/", user_id);
        
        let response = self.client
            .list_objects_v2()
            .bucket(&self.bucket)
            .prefix(&prefix)
            .send()
            .await
            .map_err(|e| AppError::new(
                ErrorCode::StorageError,
                format!("S3 list failed: {}", e)
            ))?;

        let paths = response
            .contents()
            .iter()
            .filter_map(|obj| obj.key().map(|k| k.to_string()))
            .collect();

        Ok(paths)
    }

    pub async fn generate_presigned_url(
        &self,
        storage_path: &str,
        expires_in_secs: u64,
    ) -> Result<String> {
        use aws_sdk_s3::presigning::PresigningConfig;
        use std::time::Duration;

        let presigning_config = PresigningConfig::expires_in(
            Duration::from_secs(expires_in_secs)
        ).map_err(|e| AppError::internal(format!("Presigning config error: {}", e)))?;

        let presigned = self.client
            .get_object()
            .bucket(&self.bucket)
            .key(storage_path)
            .presigned(presigning_config)
            .await
            .map_err(|e| AppError::internal(format!("Failed to generate presigned URL: {}", e)))?;

        Ok(presigned.uri().to_string())
    }

    pub async fn copy_image(&self, source_path: &str, dest_path: &str) -> Result<()> {
        let copy_source = format!("{}/{}", self.bucket, source_path);

        self.client
            .copy_object()
            .bucket(&self.bucket)
            .key(dest_path)
            .copy_source(&copy_source)
            .send()
            .await
            .map_err(|e| AppError::new(
                ErrorCode::StorageError,
                format!("S3 copy failed: {}", e)
            ))?;

        Ok(())
    }

    pub async fn delete_batch(&self, paths: Vec<String>) -> Result<Vec<String>> {
        use aws_sdk_s3::types::{Delete, ObjectIdentifier};

        let objects: Vec<ObjectIdentifier> = paths
            .iter()
            .map(|path| ObjectIdentifier::builder().key(path).build().unwrap())
            .collect();

        let delete = Delete::builder()
            .set_objects(Some(objects))
            .build()
            .map_err(|e| AppError::internal(format!("Delete builder failed: {}", e)))?;

        let response = self.client
            .delete_objects()
            .bucket(&self.bucket)
            .delete(delete)
            .send()
            .await
            .map_err(|e| AppError::new(
                ErrorCode::StorageError,
                format!("Batch delete failed: {}", e)
            ))?;

        let deleted = response
            .deleted()
            .iter()
            .filter_map(|d| d.key().map(|k| k.to_string()))
            .collect();

        Ok(deleted)
    }

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

    pub async fn import_external_image(
        &self,
        user_id: &Uuid,
        node_id: &Uuid,
        external_url: &str,
    ) -> Result<StorageResult> {
        let response = reqwest::get(external_url)
            .await
            .map_err(|e| AppError::internal(format!("Failed to download: {}", e)))?;

        let bytes = response
            .bytes()
            .await
            .map_err(|e| AppError::internal(format!("Failed to read bytes: {}", e)))?;

        let filename = external_url.split('/').last().unwrap_or("image.jpg");

        self.storage.upload_image(user_id, node_id, bytes, filename).await
    }

    pub async fn create_thumbnail(
        &self,
        user_id: &Uuid,
        node_id: &Uuid,
        original_path: &str,
        max_width: u32,
        max_height: u32,
    ) -> Result<StorageResult> {
        use image::GenericImageView;

        let data = self.storage.download_image(original_path).await?;
        let img = image::load_from_memory(&data)
            .map_err(|e| AppError::internal(format!("Invalid image: {}", e)))?;

        let thumbnail = img.thumbnail(max_width, max_height);

        let mut buffer = Vec::new();
        thumbnail
            .write_to(
                &mut std::io::Cursor::new(&mut buffer),
                image::ImageOutputFormat::Jpeg(85)
            )
            .map_err(|e| AppError::internal(format!("Failed to encode: {}", e)))?;

        let thumb_node_id = Uuid::new_v4();
        self.storage
            .upload_image(user_id, &thumb_node_id, Bytes::from(buffer), "thumbnail.jpg")
            .await
    }

    pub fn validate_image(&self, data: &Bytes, max_size_mb: u64) -> Result<()> {
        if data.len() as u64 > max_size_mb * 1024 * 1024 {
            return Err(AppError::bad_request(format!("Image too large (max {}MB)", max_size_mb)));
        }

        image::load_from_memory(data)
            .map_err(|e| AppError::validation(format!("Invalid image format: {}", e)))?;

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
            .ok_or_else(|| AppError::not_found("URL in node data"))
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
    State(state): State<Arc<crate::AppState>>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>> {
    let user_id = state.extract_user_id()?;
    let node_id = Uuid::new_v4();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad_request(format!("Multipart error: {}", e)))?
    {
        if field.name() == Some("image") {
            let filename = field
                .file_name()
                .ok_or_else(|| AppError::bad_request("Missing filename"))?
                .to_string();
            
            let data = field
                .bytes()
                .await
                .map_err(|e| AppError::bad_request(format!("Read error: {}", e)))?;

            state.image_processor.validate_image(&data, 10)?;

            let result = state.storage
                .upload_image(&user_id, &node_id, data, &filename)
                .await?;

            // Save to database
            sqlx::query!(
                r#"
                INSERT INTO tree_nodes (id, user_id, parent_id, node_type, data)
                VALUES ($1, $2, $3, $4, $5)
                "#,
                node_id,
                user_id,
                None::<Uuid>,
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
            .await?;

            return Ok(Json(UploadResponse {
                node_id,
                url: result.public_url,
                storage_path: result.storage_path,
                size: result.size,
            }));
        }
    }

    Err(AppError::bad_request("No image field found"))
}

pub async fn get_image_handler(
    State(state): State<Arc<crate::AppState>>,
    Path(node_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>> {
    let user_id = state.extract_user_id()?;

    let node = sqlx::query!(
        r#"SELECT data FROM tree_nodes WHERE id = $1 AND user_id = $2 AND node_type = 'ImageLeaf'"#,
        node_id,
        user_id
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(node.data))
}

pub async fn delete_image_handler(
    State(state): State<Arc<crate::AppState>>,
    Path(node_id): Path<Uuid>,
) -> Result<StatusCode> {
    let user_id = state.extract_user_id()?;

    let node = sqlx::query!(
        r#"SELECT data FROM tree_nodes WHERE id = $1 AND user_id = $2 AND node_type = 'ImageLeaf'"#,
        node_id,
        user_id
    )
    .fetch_one(&state.db)
    .await?;

    let storage_path = node.data
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
    State(state): State<Arc<crate::AppState>>,
    mut multipart: Multipart,
) -> Result<Json<Vec<UploadResponse>>> {
    let user_id = state.extract_user_id()?;
    let mut responses = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad_request(format!("Multipart error: {}", e)))?
    {
        if field.name() == Some("images") {
            let filename = field.file_name().unwrap_or("image.jpg").to_string();
            let data = field.bytes().await
                .map_err(|e| AppError::bad_request(format!("Read error: {}", e)))?;

            let node_id = Uuid::new_v4();

            if let Ok(result) = state.storage
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