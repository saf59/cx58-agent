use serde::{Deserialize, Serialize};
use uuid::Uuid;

use sqlx::Type;

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
}
