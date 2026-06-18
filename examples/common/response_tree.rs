#![allow(unused)]
use chrono::NaiveDateTime;
use cx58_agent::db::{NodeType, TreeNode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageData {
    pub hash: Option<String>,
    pub mime_type: Option<String>,
    pub size: Option<u64>,
    pub src: Option<String>,
    pub storage_path: Option<String>,
    pub thumbnail_url: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchData {
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeData {
    Branch(BranchData),
    Image(ImageData),
    Empty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tree {
    pub id: Uuid,
    pub node_type: NodeType,
    pub name: Option<String>,
    pub data: NodeData,
    pub raw_data: serde_json::Value,
    pub path: String,
    pub updated_at: NaiveDateTime,
    pub depth: i32,
    pub own: bool,
    pub children: Vec<Tree>,
}
// Helper method for Tree
impl Tree {
    pub fn node_type_str(&self) -> &str {
        match self.node_type {
            NodeType::Root => "Root",
            NodeType::Branch => "Branch",
            NodeType::ImageLeaf => "ImageLeaf",
        }
    }
}

/// Parse raw JSON data into typed NodeData based on node type
fn parse_node_data(node_type: NodeType, raw_data: &serde_json::Value) -> NodeData {
    match node_type {
        NodeType::ImageLeaf => match serde_json::from_value::<ImageData>(raw_data.clone()) {
            Ok(image_data) => NodeData::Image(image_data),
            Err(_) => NodeData::Empty,
        },
        NodeType::Branch | NodeType::Root => {
            match serde_json::from_value::<BranchData>(raw_data.clone()) {
                Ok(branch_data) => NodeData::Branch(branch_data),
                Err(_) => NodeData::Empty,
            }
        }
    }
}

/// Converts a flat list of TreeNodes into a hierarchical tree structure
/// Root nodes are discarded, and their children become top-level nodes
pub fn build_tree(nodes: Vec<TreeNode>) -> Vec<Tree> {
    // Create a HashMap for quick lookup by id
    let mut node_map: HashMap<Uuid, TreeNode> =
        nodes.into_iter().map(|node| (node.id, node)).collect();

    // Find and remove the root node(s)
    let root_ids: Vec<Uuid> = node_map
        .values()
        .filter(|node| node.node_type == NodeType::Root)
        .map(|node| node.id)
        .collect();

    // Remove root nodes from the map
    for root_id in &root_ids {
        node_map.remove(root_id);
    }

    // Build parent-to-children mapping
    let mut children_map: HashMap<Option<Uuid>, Vec<Uuid>> = HashMap::new();

    for node in node_map.values() {
        children_map
            .entry(node.parent_id)
            .or_default()
            .push(node.id);
    }

    // Helper function to recursively build tree
    fn build_subtree(
        node_id: Uuid,
        node_map: &HashMap<Uuid, TreeNode>,
        children_map: &HashMap<Option<Uuid>, Vec<Uuid>>,
    ) -> Tree {
        let node = node_map.get(&node_id).unwrap();

        let children = children_map
            .get(&Some(node_id))
            .map(|child_ids| {
                child_ids
                    .iter()
                    .map(|&child_id| build_subtree(child_id, node_map, children_map))
                    .collect()
            })
            .unwrap_or_default();

        let parsed_data = parse_node_data(node.node_type, &node.data);

        Tree {
            id: node.id,
            node_type: node.node_type,
            name: node.name.clone(),
            data: parsed_data,
            raw_data: node.data.clone(),
            path: node.path.clone(),
            updated_at: node.updated_at,
            depth: node.depth,
            own: node.own,
            children,
        }
    }

    // Find all nodes that were children of root nodes
    let top_level_ids: Vec<Uuid> = root_ids
        .iter()
        .filter_map(|root_id| children_map.get(&Some(*root_id)))
        .flatten()
        .copied()
        .collect();

    // Build trees for each top-level node
    top_level_ids
        .into_iter()
        .map(|node_id| build_subtree(node_id, &node_map, &children_map))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_tree_with_typed_data() {
        let root_id = Uuid::now_v7();
        let branch1_id = Uuid::now_v7();
        let leaf1_id = Uuid::now_v7();

        let nodes = vec![
            TreeNode {
                id: root_id,
                parent_id: None,
                node_type: NodeType::Root,
                name: Some("Root".to_string()),
                data: serde_json::json!({"title": "CX-5.8"}),
                path: root_id.to_string(),
                updated_at: NaiveDateTime::default(),
                depth: 0,
                own: false,
            },
            TreeNode {
                id: branch1_id,
                parent_id: Some(root_id),
                node_type: NodeType::Branch,
                name: Some("Object 1".to_string()),
                data: serde_json::json!({}),
                path: format!("{}.{}", root_id, branch1_id),
                updated_at: NaiveDateTime::default(),
                depth: 1,
                own: false,
            },
            TreeNode {
                id: leaf1_id,
                parent_id: Some(branch1_id),
                node_type: NodeType::ImageLeaf,
                name: Some("Image 1".to_string()),
                data: serde_json::json!({
                    "url": "http://example.com/image.jpg",
                    "hash": "abc123",
                    "mime_type": "image/jpeg",
                    "size": 12345
                }),
                path: format!("{}.{}.{}", root_id, branch1_id, leaf1_id),
                updated_at: NaiveDateTime::default(),
                depth: 2,
                own: true,
            },
        ];

        let tree = build_tree(nodes);

        // Root should be discarded, so we should have 1 top-level node
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].name, Some("Object 1".to_string()));

        // Check that data is parsed correctly
        assert!(matches!(tree[0].data, NodeData::Branch(_)));

        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].name, Some("Image 1".to_string()));

        // Check that image data is parsed correctly
        if let NodeData::Image(ref img) = tree[0].children[0].data {
            assert_eq!(img.url.as_deref(), Some("http://example.com/image.jpg"));
            assert_eq!(img.hash.as_deref(), Some("abc123"));
        } else {
            panic!("Expected NodeData::Image");
        }
    }
}
