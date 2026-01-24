use chrono::Local;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/*#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TreeNode {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    #[sqlx(try_from = "String")]
    pub node_type: NodeType,
    pub name: Option<String>,
    #[sqlx(try_from = "String")]
    pub data: NodeData,
    pub path: Option<String>,
    pub updated_at: DateTime<Utc>,
    pub depth: i32,
    pub own: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, sqlx::Type)]
#[serde(tag = "type")]
#[sqlx(type_name = "node_type_enum")]
pub enum NodeType {
    Root,
    Branch,
    ImageLeaf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
        #[serde(skip_serializing_if = "Option::is_none")]
        storage_path: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        size: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        hash: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
}

impl TreeNode {
    pub fn is_leaf(&self) -> bool {
        matches!(self.node_type, NodeType::ImageLeaf)
    }
}
*/

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageResult {
    pub storage_path: String,
    pub public_url: String,
    pub size: u64,
    pub mime_type: String,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadResponse {
    pub node_id: Uuid,
    pub url: String,
    pub storage_path: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageMetadata {
    pub size: u64,
    pub content_type: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageDescription {
    pub id: Uuid,
    pub node_id: Uuid,
    pub model_name: String,
    pub prompt: String,
    pub description: String,
    pub confidence: Option<f32>,
    pub created_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub timestamp: String,
    pub status: String,
    pub version: String,
    pub services: ServiceHealth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceHealth {
    pub database: bool,
    pub redis: bool,
    pub s3: bool,
    pub ollama: bool,
}

impl HealthStatus {
    pub fn healthy() -> Self {
        Self {
            timestamp: Local::now().naive_utc().to_string(),
            status: "healthy".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            services: ServiceHealth {
                database: true,
                redis: true,
                s3: true,
                ollama: true,
            },
        }
    }

    pub fn is_healthy(&self) -> bool {
        self.services.database && self.services.redis && self.services.s3 && self.services.ollama
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ImageLeafResponse {
    pub node_id: Uuid,
    pub parent_id: Uuid,
    pub url: String,
    pub storage_path: String,
    pub size: u64,
}
