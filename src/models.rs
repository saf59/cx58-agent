// shared/src/models.rs

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use sqlx::Type;
// ============================================================================
// Tree Node Models
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TreeNode {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub node_type: NodeType,
    pub data: NodeData,
    #[serde(default)]
    pub children: Vec<TreeNode>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Type)]
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

    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    pub fn depth(&self) -> usize {
        if self.children.is_empty() {
            0
        } else {
            1 + self.children.iter().map(|c| c.depth()).max().unwrap_or(0)
        }
    }

    pub fn count_nodes(&self) -> usize {
        1 + self.children.iter().map(|c| c.count_nodes()).sum::<usize>()
    }

    pub fn find_node(&self, id: &Uuid) -> Option<&TreeNode> {
        if self.id == *id {
            Some(self)
        } else {
            self.children.iter().find_map(|c| c.find_node(id))
        }
    }

    pub fn collect_leaves(&self) -> Vec<&TreeNode> {
        if self.is_leaf() {
            vec![self]
        } else {
            self.children
                .iter()
                .flat_map(|c| c.collect_leaves())
                .collect()
        }
    }
}

// ============================================================================
// Chat Message Models
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub id: Uuid,
    pub chat_id: Uuid,
    pub user_id: Uuid,
    pub session_id: String,
    pub role: MessageRole,
    pub content: String,
    #[serde(default)]
    pub tree_refs: Vec<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<i32>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Type)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

impl std::fmt::Display for MessageRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageRole::User => write!(f, "user"),
            MessageRole::Assistant => write!(f, "assistant"),
            MessageRole::System => write!(f, "system"),
        }
    }
}

// ============================================================================
// Agent Request/Response Models
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequest {
    pub message: String,
    pub chat_id: Uuid,
    pub user_id: Uuid,
    pub session_id: String,
    pub language: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tree_context: Option<Vec<Uuid>>,
}

impl AgentRequest {
    pub fn new(
        message: String,
        chat_id: Uuid,
        user_id: Uuid,
        session_id: String,
        language: String,
    ) -> Self {
        Self {
            message,
            chat_id,
            user_id,
            session_id,
            language,
            tree_context: None,
        }
    }

    pub fn with_tree_context(mut self, context: Vec<Uuid>) -> Self {
        self.tree_context = Some(context);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    // Life cycle events
    Started {
        request_id: String,
        timestamp: i64,
    },

    // Coordinator events
    CoordinatorThinking {
        request_id: String,
        message: String,
    },

    ToolSelected {
        request_id: String,
        tool_name: String,
        parameters: serde_json::Value,
    },

    // Pipeline events
    PipelineStarted {
        request_id: String,
        pipeline_name: String,
        steps: Vec<String>,
    },

    PipelineStepStarted {
        request_id: String,
        step_name: String,
        step_index: usize,
    },

    PipelineStepProgress {
        request_id: String,
        step_name: String,
        progress: f32,
        message: String,
    },

    PipelineStepCompleted {
        request_id: String,
        step_name: String,
        result_preview: Option<String>,
    },

    TreeUpdate {
        request_id: String,
        nodes: Vec<TreeNode>
    },

    ToolCall {
        request_id: String,
        tool: String,
        status: String
    },

    // Content generation events
    TextChunk {
        request_id: String,
        chunk: String,
    },

    // Completion events
    Completed {
        request_id: String,
        final_result: String,
        timestamp: i64,
    },

    // Error events
    Error {
        request_id: String,
        error: String,
        recoverable: bool,
    },

    // Cancelled events
    Cancelled {
        request_id: String,
        reason: String,
    },
}

// ============================================================================
// Storage Models
// ============================================================================

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

// ============================================================================
// Agent Intent Models
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIntent {
    pub intent: String,
    pub confidence: f32,
    pub entities: serde_json::Value,
}

impl AgentIntent {
    pub fn is_image_operation(&self) -> bool {
        matches!(
            self.intent.as_str(),
            "describe_image" | "compare_images" | "analyze_image"
        )
    }

    pub fn is_tree_operation(&self) -> bool {
        matches!(
            self.intent.as_str(),
            "create_tree_node" | "update_tree_node" | "delete_tree_node"
        )
    }
}

// ============================================================================
// Tool Call Models (for observability)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentToolCall {
    pub id: Uuid,
    pub message_id: Uuid,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub tool_output: Option<serde_json::Value>,
    pub status: ToolCallStatus,
    pub error_message: Option<String>,
    pub duration_ms: Option<i32>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ToolCallStatus {
    Started,
    Completed,
    Failed,
}

impl std::fmt::Display for ToolCallStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolCallStatus::Started => write!(f, "started"),
            ToolCallStatus::Completed => write!(f, "completed"),
            ToolCallStatus::Failed => write!(f, "failed"),
        }
    }
}

// ============================================================================
// Image Description Cache
// ============================================================================

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

// ============================================================================
// Pagination & Filtering
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginationParams {
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_page() -> u32 {
    1
}

fn default_limit() -> u32 {
    20
}

impl PaginationParams {
    pub fn offset(&self) -> u32 {
        (self.page - 1) * self.limit
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub page: u32,
    pub limit: u32,
    pub total: u64,
    pub total_pages: u32,
}

impl<T> PaginatedResponse<T> {
    pub fn new(data: Vec<T>, page: u32, limit: u32, total: u64) -> Self {
        let total_pages = ((total as f64) / (limit as f64)).ceil() as u32;
        Self {
            data,
            page,
            limit,
            total,
            total_pages,
        }
    }
}

// ============================================================================
// Search & Filter
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchParams {
    pub query: String,
    #[serde(default)]
    pub filters: SearchFilters,
    #[serde(flatten)]
    pub pagination: PaginationParams,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SearchFilters {
    pub node_types: Option<Vec<NodeType>>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub has_description: Option<bool>,
}

// ============================================================================
// Batch Operations
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchUploadRequest {
    pub parent_id: Uuid,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchUploadResponse {
    pub uploaded: Vec<UploadResponse>,
    pub failed: Vec<BatchError>,
    pub total: usize,
    pub success_count: usize,
    pub failure_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchError {
    pub index: usize,
    pub filename: Option<String>,
    pub error: String,
}

// ============================================================================
// Import Operations
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportRequest {
    pub url: String,
    pub parent_id: Uuid,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchImportRequest {
    pub urls: Vec<String>,
    pub parent_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResponse {
    pub node: TreeNode,
    pub storage_result: StorageResult,
}

// ============================================================================
// Statistics & Analytics
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserStats {
    pub user_id: Uuid,
    pub total_nodes: u64,
    pub total_images: u64,
    pub total_messages: u64,
    pub storage_used_bytes: u64,
    pub last_activity: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatStats {
    pub chat_id: Uuid,
    pub message_count: u64,
    pub referenced_nodes: Vec<Uuid>,
    pub total_tokens: u64,
    pub created_at: String,
    pub last_message_at: Option<String>,
}

// ============================================================================
// Health Check
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
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

// ============================================================================
// Config Models
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Config {
    pub bucket: String,
    pub region: String,
    pub endpoint: Option<String>,
    pub access_key: String,
    pub secret_key: String,
    pub public_url_base: String,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tree_node_depth() {
        let leaf = TreeNode {
            id: Uuid::now_v7(),
            parent_id: None,
            node_type: NodeType::ImageLeaf,
            data: NodeData::Image {
                url: "test.jpg".to_string(),
                storage_path: None,
                size: None,
                mime_type: None,
                hash: None,
                description: None,
            },
            children: vec![],
            created_at: "2024-01-01T00:00:00Z".to_string(),
        };

        assert_eq!(leaf.depth(), 0);
        assert!(leaf.is_leaf());
    }

    #[test]
    fn test_agent_request_builder() {
        let request = AgentRequest::new(
            "test".to_string(),
            Uuid::now_v7(),
            Uuid::now_v7(),
            "session".to_string(),
            "en".to_string(),
        )
        .with_tree_context(vec![Uuid::now_v7()]);

        assert!(request.tree_context.is_some());
    }

    #[test]
    fn test_pagination_offset() {
        let params = PaginationParams { page: 3, limit: 10 };
        assert_eq!(params.offset(), 20);
    }
}
