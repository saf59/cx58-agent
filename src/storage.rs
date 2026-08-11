use crate::agents::master_agent::MasterAgent;
use crate::db::NodeType;
use crate::error::*;
use crate::init::S3Config;
use crate::models::*;
use crate::report_datetime::parse_berlin_datetime_as_utc_naive;
use axum::body::Body;
use axum::http::header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue};
use axum::response::IntoResponse;
use axum::{
    Json,
    extract::{Multipart, Path, State},
    http::StatusCode,
};
use bytes::Bytes;
use futures::{StreamExt, stream};
use image::DynamicImage;
use image::codecs::jpeg::JpegEncoder;
use reqwest::header::HeaderName;
use s3::BucketConfiguration;
use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::region::Region;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::io::Cursor;
use std::path::Path as StdPath;
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

pub const MAX_UPLOAD_BODY_BYTES: usize = 25 * 1024 * 1024;
const MAX_STORED_IMAGE_MB: u64 = 10;
const MAX_STORED_IMAGE_SIDE: u32 = 1800;
const JPEG_UPLOAD_QUALITY: u8 = 85;
const STORAGE_URL_CONCURRENCY: usize = 8;

struct PreparedUploadImage {
    data: Bytes,
    reencoded_as_jpeg: bool,
}

// ============================================================================
// AppState && AiConfig
// ============================================================================
#[derive(Clone, Default)]
pub struct AiConfig {
    pub url: String,
    pub text_model: String,
    pub vision_model: String,
    pub chat_model: String,
    pub agent_secret: String,
}
impl AiConfig {
    pub fn from_env() -> std::result::Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            url: std::env::var("OLLAMA_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:11434".to_string()),
            text_model: std::env::var("TEXT_MODEL").unwrap_or_else(|_| "qwen3.5:9b".to_string()),
            vision_model: std::env::var("VISION_MODEL")
                .unwrap_or_else(|_| "llama3.2-vision".to_string()),
            chat_model: std::env::var("CHAT_MODEL").unwrap_or_else(|_| "qwen3.5:9b".to_string()),
            agent_secret: required_agent_secret()?,
        })
    }
}

fn required_agent_secret() -> std::result::Result<String, Box<dyn std::error::Error>> {
    let secret = std::env::var("AGENT_SECRET")?;
    if secret.trim().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "AGENT_SECRET must not be empty",
        )
        .into());
    }
    Ok(secret)
}
#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::PgPool,
    pub storage: Arc<StorageService>,
    pub master_agent: Arc<MasterAgent>,
    pub ai_config: AiConfig,
}
//pub redis: redis::aio::ConnectionManager,

// ============================================================================
// Storage Service with rust-s3
// ============================================================================

#[derive(Clone)]
pub struct StorageService {
    bucket: Bucket,
    #[allow(unused)]
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
        self.upload_image_type(node_id, image_data, filename, true)
            .await
    }

    pub async fn upload_image(
        &self,
        node_id: &Uuid,
        image_data: Bytes,
        filename: &str,
    ) -> Result<StorageResult> {
        self.upload_image_type(node_id, image_data, filename, false)
            .await
    }

    /// Upload image to S3
    pub async fn upload_image_type(
        &self,
        node_id: &Uuid,
        image_data: Bytes,
        filename: &str,
        is_thumbnail: bool,
    ) -> Result<StorageResult> {
        let hash = self.compute_hash(&image_data);
        let extension = std::path::Path::new(filename)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("jpg");
        let image_type = if is_thumbnail { "thumbnails" } else { "images" };
        let storage_path = format!("{}/{}/{}.{}", image_type, node_id, hash, extension);
        let mime_type = mime_guess::from_path(filename)
            .first_or_octet_stream()
            .to_string();

        if is_thumbnail {
            let mut headers = HeaderMap::new();
            headers.insert(
                HeaderName::from_static("x-amz-acl"),
                "public-read".parse().unwrap(),
            );
            self.save_image(&image_data, &storage_path, &mime_type, headers)
                .await?;
        } else {
            self.bucket
                .put_object(&storage_path, &image_data)
                .await
                .map_err(|e| {
                    AppError::new(ErrorCode::StorageError, format!("S3 upload failed: {}", e))
                })?;
        }

        // Generate presigned URL for both thumbnails and images
        // This ensures consistent access pattern
        let public_url = self
            .bucket
            .presign_get(&storage_path, 86400, None) // 24 hours validity
            .await
            .map_err(|e| {
                AppError::new(
                    ErrorCode::StorageError,
                    format!("Failed to generate presigned URL: {}", e),
                )
            })?;

        Ok(StorageResult {
            storage_path,
            public_url,
            size: image_data.len() as u64,
            mime_type,
            hash,
        })
    }

    async fn save_image(
        &self,
        image_data: &Bytes,
        storage_path: &String,
        mime_type: &String,
        headers: HeaderMap,
    ) -> std::result::Result<(), AppError> {
        let _response_data = self
            .bucket
            .put_object_with_content_type_and_headers(
                &storage_path,
                &image_data,
                &mime_type,
                Some(headers),
            )
            .await
            .map_err(|e| {
                AppError::new(ErrorCode::StorageError, format!("S3 upload failed: {}", e))
            })?;
        Ok(())
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

    pub async fn generate_presigned_url(
        &self,
        storage_path: &str,
        expires_in_secs: u32,
    ) -> Result<String> {
        use std::collections::HashMap;

        let mime_type = mime_guess::from_path(storage_path)
            .first_or_octet_stream()
            .to_string();

        let mut custom_queries = HashMap::new();
        custom_queries.insert(
            "response-content-disposition".to_string(),
            "inline".to_string(),
        );
        custom_queries.insert("response-content-type".to_string(), mime_type);

        let url = self
            .bucket
            .presign_get(storage_path, expires_in_secs, Some(custom_queries))
            .await
            .map_err(|e| AppError::internal(format!("Presigned URL failed: {}", e)))?;

        Ok(url)
    }
    fn get_thumbnail_path(&self, original_path: &str) -> Option<String> {
        if original_path.starts_with("images/") {
            Some(original_path.replacen("images/", "thumbnails/", 1))
        } else {
            None
        }
    }
    pub async fn get_or_create_thumbnail(
        &self,
        node_id: &Uuid,
        original_path: &str,
        max_width: u32,
        max_height: u32,
    ) -> Result<StorageResult> {
        let thumbnail_path = self
            .get_thumbnail_path(original_path)
            .ok_or_else(|| AppError::internal("Invalid original path format"))?;

        if self.object_exists(&thumbnail_path).await? {
            let mime_type = mime_guess::from_path(&thumbnail_path)
                .first_or_octet_stream()
                .to_string();

            // Generate fresh presigned URL or use public URL
            let public_url = format!("{}/{}", self.bucket.url(), thumbnail_path);

            // Get file size from S3 metadata
            let (head_result, _) = self
                .bucket
                .head_object(&thumbnail_path)
                .await
                .map_err(|e| {
                    AppError::new(
                        ErrorCode::StorageError,
                        format!("Failed to get thumbnail metadata: {}", e),
                    )
                })?;

            let size = head_result.content_length.unwrap_or(0) as u64;

            // Download to compute hash (optional - can be slow)
            let data = self.download_image(&thumbnail_path).await?;
            let hash = self.compute_hash(&data);

            Ok(StorageResult {
                storage_path: thumbnail_path,
                public_url,
                size,
                mime_type,
                hash,
            })
        } else {
            // Thumbnail doesn't exist - create it
            self.create_thumbnail(node_id, original_path, max_width, max_height)
                .await
        }
    }
    pub async fn create_thumbnail(
        &self,
        node_id: &Uuid,
        original_path: &str,
        max_width: u32,
        max_height: u32,
    ) -> Result<StorageResult> {
        // Download original image
        let data = self.download_image(original_path).await?;
        let img = load_oriented_image(&data)
            .map_err(|e| AppError::internal(format!("Invalid image: {}", e)))?;

        // Create thumbnail
        let thumbnail = img.thumbnail(max_width, max_height);

        let mut buffer = Vec::new();
        thumbnail
            .write_to(
                &mut std::io::Cursor::new(&mut buffer),
                image::ImageFormat::Jpeg,
            )
            .map_err(|e| AppError::internal(format!("Encode failed: {}", e)))?;

        // Convert to Bytes once
        let buffer_bytes = Bytes::from(buffer);

        // Extract filename from original_path
        let filename = original_path
            .split('/')
            .next_back()
            .ok_or_else(|| AppError::internal("Invalid original path"))?;

        // Get the original hash from filename
        let extension = std::path::Path::new(filename)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("jpg");

        let original_hash = filename
            .strip_suffix(&format!(".{}", extension))
            .ok_or_else(|| AppError::internal("Invalid filename format"))?;

        // Build the thumbnail path with original hash
        let storage_path = format!("thumbnails/{}/{}.{}", node_id, original_hash, extension);
        let mime_type = "image/jpeg".to_string();

        // Upload directly with the exact path we want
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("x-amz-acl"),
            "public-read".parse().unwrap(),
        );
        self.save_image(&buffer_bytes, &storage_path, &mime_type, headers)
            .await?;
        let public_url = self
            .bucket
            .presign_get(&storage_path, 86400, None)
            .await
            .map_err(|e| {
                AppError::new(
                    ErrorCode::StorageError,
                    format!("Failed to generate presigned URL: {}", e),
                )
            })?;

        let thumbnail_hash = self.compute_hash(&buffer_bytes);

        Ok(StorageResult {
            storage_path,
            public_url,
            size: buffer_bytes.len() as u64,
            mime_type,
            hash: thumbnail_hash,
        })
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
                    tracing::warn!("Failed to delete {}: {}", path, e);
                }
            }
        }

        Ok(deleted)
    }
    pub async fn object_exists(&self, path: &str) -> Result<bool> {
        match self.bucket.head_object(path).await {
            Ok((_, code)) => {
                // Check the HTTP status code
                Ok(code == 200)
            }
            Err(e) => {
                // Any error means file doesn't exist (404, NoSuchKey, etc.)
                let err_str = e.to_string();
                if err_str.contains("404")
                    || err_str.contains("NoSuchKey")
                    || err_str.contains("Not Found")
                {
                    Ok(false)
                } else {
                    // Real error, not just "file doesn't exist"
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
/*
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

        self.storage.upload_image(node_id, bytes, filename).await
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
*/
// ============================================================================
// HTTP Handlers
// ============================================================================

pub async fn upload_image_handler(
    State(state): State<Arc<AppState>>,
    Path(parent_id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<Json<ImageLeafResponse>> {
    let mut filename = String::new();
    let mut image_data = Bytes::new();
    let mut berlin_datetime = String::new();

    // Parse multipart form
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad_request(format!("Multipart error: {}", e)))?
    {
        match field.name() {
            Some("image") => {
                filename = field
                    .file_name()
                    .ok_or_else(|| AppError::bad_request("Missing filename"))?
                    .to_string();
                image_data = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::bad_request(format!("Read error: {}", e)))?;
            }
            Some("berlin_datetime") => {
                berlin_datetime = field
                    .text()
                    .await
                    .map_err(|e| AppError::bad_request(format!("Text error: {}", e)))?;
            }
            _ => {}
        }
    }

    // Validate required fields
    if image_data.is_empty() {
        return Err(AppError::bad_request("No image provided"));
    }
    if berlin_datetime.is_empty() {
        return Err(AppError::bad_request("Missing berlin_datetime"));
    }
    let updated_at_utc = parse_berlin_datetime_as_utc_naive(&berlin_datetime)
        .map_err(|e| AppError::bad_request(e.to_string()))?;

    let prepared_image = tokio::task::spawn_blocking(move || normalize_upload_image(image_data))
        .await
        .map_err(|e| AppError::internal(format!("Image normalization task failed: {}", e)))??;
    let storage_filename = if prepared_image.reencoded_as_jpeg {
        jpeg_storage_filename(&filename)
    } else {
        filename.clone()
    };

    // Validate image
    validate_image(&prepared_image.data, MAX_STORED_IMAGE_MB)?;

    // Upload to S3
    let node_id = Uuid::now_v7();
    let storage_result = state
        .storage
        .upload_image(&node_id, prepared_image.data, &storage_filename)
        .await?;

    // Insert into database with parent_id
    let insert_result = sqlx::query(
        r#"
        INSERT INTO tree_nodes (id, parent_id, node_type, data, updated_at)
        VALUES (
            $1,
            $2,
            $3,
            $4,
            $5
        )
        "#,
    )
    .bind(node_id)
    .bind(parent_id)
    .bind(NodeType::ImageLeaf)
    .bind(serde_json::json!({
        "url": storage_result.public_url,
        "src": filename,
        "storage_path": storage_result.storage_path,
        "size": storage_result.size,
        "mime_type": storage_result.mime_type,
        "hash": storage_result.hash,
    }))
    .bind(updated_at_utc)
    .execute(&state.db)
    .await;

    if let Err(e) = insert_result {
        if let Err(cleanup_error) = state
            .storage
            .delete_image(&storage_result.storage_path)
            .await
        {
            tracing::error!(
                node_id = %node_id,
                storage_path = %storage_result.storage_path,
                error = %cleanup_error,
                "Failed to clean up S3 object after DB insert failure"
            );
        }
        return Err(e.into());
    }

    Ok(Json(ImageLeafResponse {
        node_id,
        parent_id,
        url: storage_result.public_url,
        storage_path: storage_result.storage_path,
        size: storage_result.size,
    }))
}

fn normalize_upload_image(image_data: Bytes) -> Result<PreparedUploadImage> {
    let original_len = image_data.len();
    let orientation = exif_orientation(&image_data).unwrap_or(1);
    let image = load_oriented_image(&image_data)
        .map_err(|e| AppError::validation(format!("Invalid image: {}", e)))?;
    let needs_resize =
        image.width() > MAX_STORED_IMAGE_SIDE || image.height() > MAX_STORED_IMAGE_SIDE;

    if orientation == 1 && !needs_resize && original_len as u64 <= MAX_STORED_IMAGE_MB * 1024 * 1024
    {
        return Ok(PreparedUploadImage {
            data: image_data,
            reencoded_as_jpeg: false,
        });
    }

    let prepared = if needs_resize {
        image.thumbnail(MAX_STORED_IMAGE_SIDE, MAX_STORED_IMAGE_SIDE)
    } else {
        image
    };

    let normalized = encode_jpeg(&prepared, JPEG_UPLOAD_QUALITY)
        .map_err(|e| AppError::validation(format!("Image encode failed: {}", e)))?;
    Ok(PreparedUploadImage {
        data: Bytes::from(normalized),
        reencoded_as_jpeg: true,
    })
}

fn jpeg_storage_filename(filename: &str) -> String {
    let stem = StdPath::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    format!("{}.jpg", stem)
}

fn load_oriented_image(data: &[u8]) -> std::result::Result<DynamicImage, image::ImageError> {
    let orientation = exif_orientation(data).unwrap_or(1);
    let image = image::load_from_memory(data)?;
    Ok(apply_exif_orientation(image, orientation))
}

fn exif_orientation(image_data: &[u8]) -> Option<u32> {
    if image_data.starts_with(b"Exif\0\0") {
        return tiff_orientation(&image_data[6..]);
    }

    if !image_data.starts_with(&[0xFF, 0xD8]) {
        return None;
    }

    let mut offset = 2;
    while offset + 4 <= image_data.len() {
        if image_data[offset] != 0xFF {
            return None;
        }

        let marker = image_data[offset + 1];
        if marker == 0xDA || marker == 0xD9 {
            return None;
        }

        let segment_len =
            u16::from_be_bytes([image_data[offset + 2], image_data[offset + 3]]) as usize;
        if segment_len < 2 {
            return None;
        }

        let segment_start = offset + 4;
        let segment_end = offset + 2 + segment_len;
        if segment_end > image_data.len() {
            return None;
        }

        let segment = &image_data[segment_start..segment_end];
        if marker == 0xE1 && segment.starts_with(b"Exif\0\0") {
            return tiff_orientation(&segment[6..]);
        }

        offset = segment_end;
    }

    None
}

fn tiff_orientation(tiff: &[u8]) -> Option<u32> {
    if tiff.len() < 8 {
        return None;
    }

    let little_endian = match &tiff[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };

    if read_u16(tiff, 2, little_endian)? != 42 {
        return None;
    }

    let ifd_offset = read_u32(tiff, 4, little_endian)? as usize;
    if ifd_offset + 2 > tiff.len() {
        return None;
    }

    let entry_count = read_u16(tiff, ifd_offset, little_endian)? as usize;
    let entries_start = ifd_offset + 2;

    for index in 0..entry_count {
        let entry_offset = entries_start + index * 12;
        if entry_offset + 12 > tiff.len() {
            return None;
        }

        let tag = read_u16(tiff, entry_offset, little_endian)?;
        let field_type = read_u16(tiff, entry_offset + 2, little_endian)?;
        let count = read_u32(tiff, entry_offset + 4, little_endian)?;

        if tag == 0x0112 && field_type == 3 && count >= 1 {
            return read_u16(tiff, entry_offset + 8, little_endian).map(u32::from);
        }
    }

    None
}

fn read_u16(data: &[u8], offset: usize, little_endian: bool) -> Option<u16> {
    let bytes = [*data.get(offset)?, *data.get(offset + 1)?];
    Some(if little_endian {
        u16::from_le_bytes(bytes)
    } else {
        u16::from_be_bytes(bytes)
    })
}

fn read_u32(data: &[u8], offset: usize, little_endian: bool) -> Option<u32> {
    let bytes = [
        *data.get(offset)?,
        *data.get(offset + 1)?,
        *data.get(offset + 2)?,
        *data.get(offset + 3)?,
    ];
    Some(if little_endian {
        u32::from_le_bytes(bytes)
    } else {
        u32::from_be_bytes(bytes)
    })
}

fn apply_exif_orientation(image: DynamicImage, orientation: u32) -> DynamicImage {
    match orientation {
        2 => image.fliph(),
        3 => image.rotate180(),
        4 => image.flipv(),
        5 => image.rotate90().fliph(),
        6 => image.rotate90(),
        7 => image.rotate270().fliph(),
        8 => image.rotate270(),
        _ => image,
    }
}

fn encode_jpeg(
    image: &DynamicImage,
    quality: u8,
) -> std::result::Result<Vec<u8>, image::ImageError> {
    let mut buffer = Cursor::new(Vec::new());
    let mut encoder = JpegEncoder::new_with_quality(&mut buffer, quality);
    encoder.encode_image(image)?;
    Ok(buffer.into_inner())
}

pub fn validate_image(data: &Bytes, max_size_mb: u64) -> Result<()> {
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

pub async fn get_image_handler(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<Uuid>,
) -> Result<impl IntoResponse> {
    let node = sqlx::query!(
        r#"SELECT data FROM tree_nodes WHERE id = $1 AND node_type = 'ImageLeaf'"#,
        node_id
    )
    .fetch_one(&state.db)
    .await?;

    let storage_path = node
        .data
        .get("storage_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::not_found("Storage path"))?;

    let mime_type = node
        .data
        .get("mime_type")
        .and_then(|v| v.as_str())
        .unwrap_or("application/octet-stream");

    let file_data = state.storage.download_image(storage_path).await?;

    let mut headers = HeaderMap::new();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_str(mime_type)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );

    headers.insert(CONTENT_DISPOSITION, HeaderValue::from_static("inline"));

    headers.insert(
        CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=3600"),
    );

    Ok((headers, Body::from(file_data)))
}

pub async fn delete_image_handler(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<Uuid>,
) -> Result<StatusCode> {
    let node = sqlx::query(
        r#"
        DELETE FROM tree_nodes
        WHERE id = $1 AND node_type = 'ImageLeaf'::node_type_enum
        RETURNING data
        "#,
    )
    .bind(node_id)
    .fetch_one(&state.db)
    .await?;

    let storage_path = node
        .try_get::<Value, _>("data")?
        .get("storage_path")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| AppError::bad_request("No storage path"))?;

    if let Err(e) = state.storage.delete_image(&storage_path).await {
        tracing::error!(
            node_id = %node_id,
            storage_path = %storage_path,
            error = %e,
            "Deleted DB record but failed to delete S3 object"
        );
    }
    tracing::debug!(node_id = %node_id, storage_path = %storage_path, "Deleted image DB record");
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

            if let Ok(result) = state.storage.upload_image(&node_id, data, &filename).await {
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
/// Create bucket with public access for reading
pub async fn create_public_bucket(config: &S3Config, bucket_name: &str) -> Result<Box<Bucket>> {
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
    )
    .map_err(|_| AppError::unauthorized("Bad S3 credentials".to_string()))?;

    let create_bucket_config = BucketConfiguration::public();

    let response = Bucket::create_with_path_style(
        bucket_name,
        region.clone(),
        credentials.clone(),
        create_bucket_config,
    )
    .await
    .unwrap();

    tracing::info!(
        "✓ Public bucket '{}' created with code {}\n{}",
        bucket_name,
        &response.response_code,
        &response.response_text
    );

    Ok(response.bucket)
}

#[cfg(test)]
mod tests {
    use crate::init::S3Config;
    use crate::storage::create_public_bucket;
    use crate::storage::{StorageUrlUpdate, apply_storage_url_update};
    use crate::storage::{apply_exif_orientation, tiff_orientation};
    use bytes::Bytes;
    use image::DynamicImage;
    use serde_json::Map;
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn test_tiff_orientation_parser() {
        let tiff = [
            b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0,
            0,
        ];

        assert_eq!(tiff_orientation(&tiff), Some(6));
    }

    #[test]
    fn test_apply_exif_orientation_rotate90() {
        let image = DynamicImage::new_rgb8(2, 3);
        let rotated = apply_exif_orientation(image, 6);

        assert_eq!(rotated.width(), 3);
        assert_eq!(rotated.height(), 2);
    }

    #[test]
    fn test_apply_storage_url_update() {
        let mut obj = Map::new();
        apply_storage_url_update(
            &mut obj,
            StorageUrlUpdate {
                url: Some("https://signed.example/image".to_string()),
                thumbnail_url: Some("https://signed.example/thumb".to_string()),
            },
        );

        assert_eq!(
            obj.get("url").and_then(|v| v.as_str()),
            Some("https://signed.example/image")
        );
        assert_eq!(
            obj.get("thumbnail_url").and_then(|v| v.as_str()),
            Some("https://signed.example/thumb")
        );
    }

    #[tokio::test]
    async fn example_usage() {
        dotenv::from_path(".env.test").ok();

        let config = S3Config::from_env().unwrap();
        let storage = crate::init::setup_storage(&config).unwrap();

        let s3_path = format!("example_usage_{}.file", Uuid::now_v7());
        let test = b"I'm going to S3!";

        // Upload file
        let response_data = storage.bucket.put_object(&s3_path, test).await.unwrap();
        assert_eq!(response_data.status_code(), 200);
        //tracing::info!("{:#?}", response_data.headers());

        // Get file via SDK
        let response_data = storage.bucket.get_object(&s3_path).await.unwrap();
        assert_eq!(response_data.status_code(), 200);
        assert_eq!(test, response_data.as_slice());

        // Get file range
        let response_data = storage
            .bucket
            .get_object_range(&s3_path, 1, Some(10))
            .await
            .unwrap();
        assert_eq!(response_data.status_code(), 206);

        // Head request
        let (head_object_result, code) = storage.bucket.head_object(&s3_path).await.unwrap();
        assert_eq!(code, 200);
        assert_eq!(
            head_object_result.content_type.unwrap_or_default(),
            "application/octet-stream".to_owned()
        );

        // Generate presigned URL
        let presigned_url = storage
            .bucket
            .presign_get(&s3_path, 300, None)
            .await
            .unwrap();
        tracing::info!("Presigned URL: {}", presigned_url);

        // Verify presigned URL via HTTP request
        let client = reqwest::Client::new();
        let presigned_response = client.get(&presigned_url).send().await.unwrap();
        assert_eq!(presigned_response.status(), 200);
        let presigned_body = presigned_response.bytes().await.unwrap();
        assert_eq!(test, presigned_body.as_ref());
        tracing::info!("✓ Presigned URL works correctly");

        // Generate public URL (if bucket is public)
        let public_url = format!(
            "{}/{}/{}",
            storage.bucket.url(),
            storage.bucket.name(),
            s3_path
        );
        tracing::info!("Public URL: {}", public_url);

        // Try to access public URL
        // Note: this only works if bucket is configured as public
        let public_response = client.get(&public_url).send().await;
        match public_response {
            Ok(resp) => {
                if resp.status() == 200 {
                    let public_body = resp.bytes().await.unwrap();
                    assert_eq!(test, public_body.as_ref());
                    tracing::info!("✓ Public URL works correctly");
                } else {
                    tracing::info!(
                        "⚠ Public URL returned status: {} (bucket might not be public)",
                        resp.status()
                    );
                }
            }
            Err(e) => {
                tracing::info!(
                    "⚠ Public URL access failed: {} (bucket might not be public)",
                    e
                );
            }
        }

        // Test presigned URL expiration (optional)
        // Generate URL with short lifetime
        let short_presigned_url = storage.bucket.presign_get(&s3_path, 1, None).await.unwrap();
        tracing::info!("Short-lived presigned URL: {}", short_presigned_url);

        // Wait for expiration
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        let expired_response = client.get(&short_presigned_url).send().await.unwrap();
        if expired_response.status() == 200 {
            tracing::info!(
                "Short-lived presigned URL was still accepted; backend clock granularity may vary"
            );
        } else {
            tracing::info!("✓ Presigned URL correctly expires");
        }

        // Delete file
        let response_data = storage.bucket.delete_object(&s3_path).await.unwrap();
        assert_eq!(response_data.status_code(), 204);

        // Verify that URLs no longer work after deletion
        let deleted_response = client.get(&presigned_url).send().await.unwrap();
        assert_eq!(deleted_response.status(), 404);
        tracing::info!("✓ URLs return 404 after object deletion");
    }

    #[tokio::test]
    async fn test_presigned_put_url() {
        dotenv::from_path(".env.test").ok();

        let config = S3Config::from_env().unwrap();
        let storage = crate::init::setup_storage(&config).unwrap();

        let s3_path = "test_presigned_upload.file";
        let test_data = b"Uploaded via presigned URL!";

        // Generate presigned URL for upload
        let presigned_put_url = storage
            .bucket
            .presign_put(s3_path, 300, None, None)
            .await
            .unwrap();
        tracing::info!("Presigned PUT URL: {}", presigned_put_url);

        // Upload via presigned URL
        let client = reqwest::Client::new();
        let upload_response = client
            .put(&presigned_put_url)
            .body(test_data.to_vec())
            .send()
            .await
            .unwrap();

        assert_eq!(upload_response.status(), 200);
        tracing::info!("✓ Upload via presigned PUT URL successful");

        // Verify that file was actually uploaded
        let response_data = storage.bucket.get_object(s3_path).await.unwrap();
        assert_eq!(test_data, response_data.as_slice());
        tracing::info!("✓ File uploaded correctly");

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
        storage
            .bucket
            .put_object_with_content_type(s3_path, test_data, "text/plain")
            .await
            .unwrap();

        // Generate presigned URL
        let presigned_url = storage
            .bucket
            .presign_get(s3_path, 3000, None)
            .await
            .unwrap();

        // Verify via HTTP
        let client = reqwest::Client::new();
        let response = client.get(&presigned_url).send().await.unwrap();

        assert_eq!(response.status(), 200);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/plain"
        );
        tracing::info!("✓ Custom headers preserved correctly");

        // Cleanup
        storage.bucket.delete_object(s3_path).await.unwrap();
    }

    #[tokio::test]
    async fn test_presigned_url_through_proxy() {
        dotenv::from_path(".env.test").ok();
        let config = S3Config::from_env().unwrap();
        let storage = crate::init::setup_storage(&config).unwrap();

        let s3_path = format!("test_proxy_{}.file", Uuid::now_v7());
        let test_data = b"Test data";

        let response = storage
            .bucket
            .put_object(&s3_path, test_data)
            .await
            .unwrap();
        tracing::info!("Put response code: {}", response.status_code());
        assert_eq!(response.status_code(), 200);

        let presigned_url = storage
            .bucket
            .presign_get(&s3_path, 300, None)
            .await
            .unwrap();

        tracing::info!("Generated presigned URL: {}", presigned_url);

        let client = reqwest::Client::new();
        let response = client.get(&presigned_url).send().await.unwrap();

        tracing::info!("Response status: {}", response.status());
        tracing::info!("Response headers: {:?}", response.headers());

        if response.status() == 404 {
            tracing::info!("URL path in request: {:?}", response.url());
        }

        assert_eq!(response.status(), 200);

        storage.bucket.delete_object(&s3_path).await.unwrap();
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

        tracing::info!("{:#?}", &result);

        // Verify the upload result
        assert_eq!(result.size, 527174); // Expected size of noise_1.jpg
        assert_eq!(result.mime_type, "image/jpeg");
        assert!(
            result
                .storage_path
                .starts_with(&format!("images/{}", node_id))
        );
        assert!(result.public_url.contains(&format!("images/{}", node_id)));
        assert!(!result.hash.is_empty());
        assert_eq!(result.hash.len(), 64); // SHA256 hash length

        // Verify the file exists
        let exists = storage.object_exists(&result.storage_path).await.unwrap();
        assert!(exists);

        // Cleanup - delete the uploaded file
        storage
            .bucket
            .delete_object(&result.storage_path)
            .await
            .unwrap();
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

        tracing::info!("{:#?}", &result);

        // Verify the upload result
        assert_eq!(result.mime_type, "image/jpeg");
        assert!(
            result
                .storage_path
                .starts_with(&format!("thumbnails/{}", node_id))
        );
        assert!(
            result
                .public_url
                .contains(&format!("thumbnails/{}", node_id))
        );
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
                    tracing::info!("✓ Thumbnail is publicly accessible");
                } else {
                    tracing::info!("⚠ Thumbnail URL returned status: {}", resp.status());
                }
            }
            Err(e) => {
                tracing::info!("⚠ Thumbnail public access failed: {}", e);
            }
        }

        // Cleanup
        storage
            .bucket
            .delete_object(&result.storage_path)
            .await
            .unwrap();
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
        storage
            .bucket
            .delete_object(&result.storage_path)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_create_thumbnail() {
        dotenv::from_path(".env.test").ok();

        let config = S3Config::from_env().unwrap();
        let storage = crate::init::setup_storage(&config).unwrap();

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

        tracing::info!("Original image: {:#?}", upload_result);

        // Create thumbnail from the original
        let thumbnail_result = storage
            .create_thumbnail(&node_id, &upload_result.storage_path, 100, 100)
            .await
            .expect("Failed to create thumbnail");

        tracing::info!("Thumbnail: {:#?}", thumbnail_result);

        // Verify thumbnail properties
        assert_eq!(thumbnail_result.mime_type, "image/jpeg");
        assert!(thumbnail_result.storage_path.starts_with("thumbnails/"));
        assert!(thumbnail_result.storage_path.contains(&node_id.to_string()));

        // Extract filename from original path and verify it's used in thumbnail
        let original_filename = upload_result.storage_path.split('/').next_back().unwrap();
        assert!(thumbnail_result.storage_path.ends_with(original_filename));

        assert!(thumbnail_result.size > 0);
        assert!(!thumbnail_result.hash.is_empty());
        assert_eq!(thumbnail_result.hash.len(), 64); // SHA256 hash length

        // Verify the thumbnail exists
        let exists = storage
            .object_exists(&thumbnail_result.storage_path)
            .await
            .unwrap();
        assert!(exists);

        // Cleanup - delete both original and thumbnail
        storage
            .bucket
            .delete_object(&upload_result.storage_path)
            .await
            .unwrap();
        storage
            .bucket
            .delete_object(&thumbnail_result.storage_path)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_get_or_create_thumbnail() {
        dotenv::from_path(".env.test").ok();

        let config = S3Config::from_env().unwrap();
        let storage = crate::init::setup_storage(&config).unwrap();

        // Upload original image
        let image_data = fs::read("data/noise_1.jpg").expect("Failed to read test image");
        let image_bytes = Bytes::from(image_data);
        let node_id = Uuid::now_v7();
        let filename = "noise_1.jpg";

        let upload_result = storage
            .upload_image_type(&node_id, image_bytes, filename, false)
            .await
            .expect("Failed to upload original image");

        tracing::info!("Original image: {:#?}", upload_result);

        // First call - should create thumbnail
        let thumbnail_result_1 = storage
            .get_or_create_thumbnail(&node_id, &upload_result.storage_path, 100, 100)
            .await
            .expect("Failed to get or create thumbnail");

        tracing::info!("First call (created): {:#?}", thumbnail_result_1);

        // Verify thumbnail was created
        assert!(thumbnail_result_1.storage_path.starts_with("thumbnails/"));
        let thumbnail_path = thumbnail_result_1.storage_path.clone();

        // Second call - should return existing thumbnail without recreation
        let thumbnail_result_2 = storage
            .get_or_create_thumbnail(&node_id, &upload_result.storage_path, 100, 100)
            .await
            .expect("Failed to get existing thumbnail");

        tracing::info!("Second call (retrieved): {:#?}", thumbnail_result_2);

        // Verify same thumbnail is returned
        assert_eq!(
            thumbnail_result_1.storage_path,
            thumbnail_result_2.storage_path
        );
        assert_eq!(thumbnail_result_1.hash, thumbnail_result_2.hash);

        // Verify thumbnail exists
        let exists = storage.object_exists(&thumbnail_path).await.unwrap();
        assert!(exists);

        // Cleanup
        storage
            .bucket
            .delete_object(&upload_result.storage_path)
            .await
            .unwrap();
        storage.bucket.delete_object(&thumbnail_path).await.unwrap();
    }

    #[tokio::test]
    async fn test_thumbnail_path_conversion() {
        dotenv::from_path(".env.test").ok();

        let config = S3Config::from_env().unwrap();
        let storage = crate::init::setup_storage(&config).unwrap();

        let node_id = Uuid::now_v7();
        let hash = "abc123def456";
        let original_path = format!("images/{}/{}.jpg", node_id, hash);

        // Test path conversion
        let thumbnail_path = storage
            .get_thumbnail_path(&original_path)
            .expect("Failed to convert path");

        let expected_path = format!("thumbnails/{}/{}.jpg", node_id, hash);
        assert_eq!(thumbnail_path, expected_path);

        tracing::info!("✓ Path conversion works correctly");
        tracing::info!("  Original:  {}", original_path);
        tracing::info!("  Thumbnail: {}", thumbnail_path);
    }

    #[tokio::test]
    async fn test_object_exists() {
        dotenv::from_path(".env.test").ok();

        let config = S3Config::from_env().unwrap();
        let storage = crate::init::setup_storage(&config).unwrap();

        // Use unique filename to avoid conflicts between test runs
        let test_id = Uuid::now_v7();
        let s3_path = format!("test_exists_{}.file", test_id);
        let test_data = b"Test data for existence check";

        // Ensure file doesn't exist from previous runs (cleanup first)
        let _ = storage.bucket.delete_object(&s3_path).await;

        // File should not exist initially
        let result = storage.object_exists(&s3_path).await;
        let exists_before = result.unwrap();
        assert!(!exists_before);

        // Upload file
        storage
            .bucket
            .put_object(&s3_path, test_data)
            .await
            .unwrap();

        // File should exist now
        let exists_after = storage.object_exists(&s3_path).await.unwrap();
        assert!(exists_after);

        // Cleanup
        storage.bucket.delete_object(&s3_path).await.unwrap();

        // File should not exist after deletion
        let exists_deleted = storage.object_exists(&s3_path).await.unwrap();
        assert!(!exists_deleted);

        tracing::info!("✓ Object existence check works correctly");
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

        tracing::info!("✓ Image download works correctly");

        // Cleanup
        storage
            .bucket
            .delete_object(&upload_result.storage_path)
            .await
            .unwrap();
    }
}

#[derive(Debug, Default)]
pub struct StorageUrlUpdate {
    pub url: Option<String>,
    pub thumbnail_url: Option<String>,
}

pub async fn resolve_storage_url_updates(
    state: Arc<AppState>,
    requests: Vec<(usize, Uuid, String)>,
) -> Vec<(usize, StorageUrlUpdate)> {
    stream::iter(requests.into_iter().map(|(index, node_id, storage_path)| {
        let state = state.clone();
        async move {
            let update = resolve_storage_url_update(state, node_id, storage_path).await;
            (index, update)
        }
    }))
    .buffer_unordered(STORAGE_URL_CONCURRENCY)
    .collect()
    .await
}

pub fn apply_storage_url_update(obj: &mut Map<String, Value>, update: StorageUrlUpdate) {
    if let Some(url) = update.url {
        obj.insert("url".to_string(), serde_json::json!(url));
    }
    if let Some(thumbnail_url) = update.thumbnail_url {
        obj.insert(
            "thumbnail_url".to_string(),
            serde_json::json!(thumbnail_url),
        );
    }
}

pub async fn set_storage_url(state: Arc<AppState>, obj: &mut Map<String, Value>, node_id: &Uuid) {
    if let Some(storage_path) = storage_path_from_json(obj) {
        let update = resolve_storage_url_update(state, *node_id, storage_path).await;
        apply_storage_url_update(obj, update);
    }
}

fn storage_path_from_json(obj: &Map<String, Value>) -> Option<String> {
    obj.get("storage_path")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

async fn resolve_storage_url_update(
    state: Arc<AppState>,
    node_id: Uuid,
    storage_path: String,
) -> StorageUrlUpdate {
    let mut update = StorageUrlUpdate::default();

    match state
        .storage
        .generate_presigned_url(&storage_path, 86400)
        .await
    {
        Ok(url) => update.url = Some(url),
        Err(e) => {
            tracing::error!("Failed to generate URL for {}: {}", storage_path, e);
        }
    }

    match state
        .storage
        .get_or_create_thumbnail(&node_id, &storage_path, 300, 300)
        .await
    {
        Ok(thumbnail) => update.thumbnail_url = Some(thumbnail.public_url),
        Err(e) => {
            tracing::error!("Failed to create thumbnail for {}: {}", storage_path, e);
        }
    }

    update
}
