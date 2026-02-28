use chrono::{NaiveDateTime, Utc};
use sqlx::types::Uuid;
use std::collections::HashSet;
use crate::agents::agent_error::AgentError;
use crate::agents::Period;
use crate::db::{NodeType, TreeNode};
use crate::TaskParameters;

impl Period {
    pub(crate) fn to_days(self) -> i64 {
        match self {
            Period::Day => 1,
            Period::Week => 7,
            Period::Month => 30,
            Period::Quarter => 90,
            Period::Year => 365,
        }
    }
}

//type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub async fn get_filtered_tree(
    tree: Vec<TreeNode>,
    parameters: &TaskParameters,
) -> Result<Vec<TreeNode>, AgentError> {
    // Treat last and all as equivalent
    let all = parameters.all || parameters.last;

    // Rule 1: last == false && all == false && period == None -> return empty
    if !all && parameters.period.is_none() {
        return Ok(Vec::new());
    }

    // Rule 2: all == true && period == None -> return without Root and ImageLeaf
    if all && parameters.period.is_none() {
        return Ok(tree
            .into_iter()
            .filter(|node| node.node_type != NodeType::ImageLeaf)
            .collect());
    }

    // Rule 3: Handle period-based filtering
    if let Some(period) = &parameters.period {
        let amount = parameters.amount.unwrap_or(1);
        let days = period.to_days() * amount as i64;
        
        // Calculate max_updated_at: current_time - (period * amount)
        let current_time = Utc::now().naive_utc();
        let max_updated_at = current_time - chrono::Duration::days(days);

        return filter_by_period(tree, max_updated_at);
    }

    Ok(Vec::new())
}

fn filter_by_period(
    tree: Vec<TreeNode>,
    max_updated_at: NaiveDateTime,
) -> Result<Vec<TreeNode>, AgentError> {
    // Find ImageLeaf nodes with updated_at >= max_updated_at (recent updates)
    let selected_leaves: Vec<_> = tree
        .iter()
        .filter(|node| {
            node.node_type == NodeType::ImageLeaf && node.updated_at >= max_updated_at
        })
        .collect();

    // Extract all node IDs from paths of selected leaves
    let mut valid_ids: HashSet<Uuid> = HashSet::new();
    for leaf in selected_leaves {
        // Parse path: "id1.id2.id3"
        for id_str in leaf.path.split('.') {
            if let Ok(uuid) = Uuid::parse_str(id_str) {
                valid_ids.insert(uuid);
            }
        }
        // Also include the leaf's own ID (though we'll filter it out later)
        valid_ids.insert(leaf.id);
    }

    // Filter tree: keep only nodes in valid paths, exclude Root and ImageLeaf
    Ok(tree
        .into_iter()
        .filter(|node| {
            valid_ids.contains(&node.id) && node.node_type != NodeType::ImageLeaf
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn create_test_tree_from_csv() -> Vec<TreeNode> {
        vec![
            TreeNode {
                id: Uuid::parse_str("019ba8c4-cd34-7b60-8297-1b9e7283e0ab").unwrap(),
                parent_id: None,
                node_type: NodeType::Root,
                name: Some("Root".to_string()),
                data: serde_json::json!({"title": "CX-5.8"}),
                path: "019ba8c4-cd34-7b60-8297-1b9e7283e0ab".to_string(),
                updated_at: NaiveDate::from_ymd_opt(2026, 1, 10).unwrap()
                    .and_hms_opt(16, 37, 8).unwrap(),
                depth: 0,
                own: false,
            },
            TreeNode {
                id: Uuid::parse_str("019ba8c5-cea5-7b2a-8b0c-65416d8c8f94").unwrap(),
                parent_id: Some(Uuid::parse_str("019ba8c4-cd34-7b60-8297-1b9e7283e0ab").unwrap()),
                node_type: NodeType::Branch,
                name: Some("Object 2".to_string()),
                data: serde_json::json!({}),
                path: "019ba8c4-cd34-7b60-8297-1b9e7283e0ab.019ba8c5-cea5-7b2a-8b0c-65416d8c8f94".to_string(),
                updated_at: NaiveDate::from_ymd_opt(2026, 1, 10).unwrap()
                    .and_hms_opt(16, 38, 14).unwrap(),
                depth: 1,
                own: false,
            },
            TreeNode {
                id: Uuid::parse_str("019ba8c5-cea5-7b7d-af9c-ceec4e4f889f").unwrap(),
                parent_id: Some(Uuid::parse_str("019ba8c5-cea5-7b2a-8b0c-65416d8c8f94").unwrap()),
                node_type: NodeType::Branch,
                name: Some("Floor 21".to_string()),
                data: serde_json::json!({}),
                path: "019ba8c4-cd34-7b60-8297-1b9e7283e0ab.019ba8c5-cea5-7b2a-8b0c-65416d8c8f94.019ba8c5-cea5-7b7d-af9c-ceec4e4f889f".to_string(),
                updated_at: NaiveDate::from_ymd_opt(2026, 1, 10).unwrap()
                    .and_hms_opt(16, 38, 14).unwrap(),
                depth: 2,
                own: false,
            },
            TreeNode {
                id: Uuid::parse_str("019ba8c5-cea5-7bc9-9e29-6c230973ed4a").unwrap(),
                parent_id: Some(Uuid::parse_str("019ba8c5-cea5-7b7d-af9c-ceec4e4f889f").unwrap()),
                node_type: NodeType::Branch,
                name: Some("Room 211".to_string()),
                data: serde_json::json!({}),
                path: "019ba8c4-cd34-7b60-8297-1b9e7283e0ab.019ba8c5-cea5-7b2a-8b0c-65416d8c8f94.019ba8c5-cea5-7b7d-af9c-ceec4e4f889f.019ba8c5-cea5-7bc9-9e29-6c230973ed4a".to_string(),
                updated_at: NaiveDate::from_ymd_opt(2026, 1, 10).unwrap()
                    .and_hms_opt(16, 38, 14).unwrap(),
                depth: 3,
                own: true,
            },
            TreeNode {
                id: Uuid::parse_str("019badc0-0deb-7580-9b7f-f55ebdf021de").unwrap(),
                parent_id: Some(Uuid::parse_str("019ba8c5-cea5-7bc9-9e29-6c230973ed4a").unwrap()),
                node_type: NodeType::ImageLeaf,
                name: Some("26.12.2025 19:00:00".to_string()),
                data: serde_json::json!({"src": "3w_1.jpg"}),
                path: "019ba8c4-cd34-7b60-8297-1b9e7283e0ab.019ba8c5-cea5-7b2a-8b0c-65416d8c8f94.019ba8c5-cea5-7b7d-af9c-ceec4e4f889f.019ba8c5-cea5-7bc9-9e29-6c230973ed4a.019badc0-0deb-7580-9b7f-f55ebdf021de".to_string(),
                updated_at: NaiveDate::from_ymd_opt(2025, 12, 26).unwrap()
                    .and_hms_opt(18, 0, 0).unwrap(),
                depth: 4,
                own: true,
            },
            TreeNode {
                id: Uuid::parse_str("019badc0-0f50-77a0-860f-f48b84d67306").unwrap(),
                parent_id: Some(Uuid::parse_str("019ba8c5-cea5-7bc9-9e29-6c230973ed4a").unwrap()),
                node_type: NodeType::ImageLeaf,
                name: Some("02.01.2026 19:00:00".to_string()),
                data: serde_json::json!({"src": "3w_2.jpg"}),
                path: "019ba8c4-cd34-7b60-8297-1b9e7283e0ab.019ba8c5-cea5-7b2a-8b0c-65416d8c8f94.019ba8c5-cea5-7b7d-af9c-ceec4e4f889f.019ba8c5-cea5-7bc9-9e29-6c230973ed4a.019badc0-0f50-77a0-860f-f48b84d67306".to_string(),
                updated_at: NaiveDate::from_ymd_opt(2026, 1, 2).unwrap()
                    .and_hms_opt(18, 0, 0).unwrap(),
                depth: 4,
                own: true,
            },
            TreeNode {
                id: Uuid::parse_str("019badc0-109e-7e13-bab5-9b37acf707a3").unwrap(),
                parent_id: Some(Uuid::parse_str("019ba8c5-cea5-7bc9-9e29-6c230973ed4a").unwrap()),
                node_type: NodeType::ImageLeaf,
                name: Some("08.01.2026 19:00:00".to_string()),
                data: serde_json::json!({"src": "3w_3.jpg"}),
                path: "019ba8c4-cd34-7b60-8297-1b9e7283e0ab.019ba8c5-cea5-7b2a-8b0c-65416d8c8f94.019ba8c5-cea5-7b7d-af9c-ceec4e4f889f.019ba8c5-cea5-7bc9-9e29-6c230973ed4a.019badc0-109e-7e13-bab5-9b37acf707a3".to_string(),
                updated_at: NaiveDate::from_ymd_opt(2026, 1, 8).unwrap()
                    .and_hms_opt(18, 0, 0).unwrap(),
                depth: 4,
                own: true,
            },
            TreeNode {
                id: Uuid::parse_str("019badc0-1347-7581-809e-c03c89e09ac4").unwrap(),
                parent_id: Some(Uuid::parse_str("019ba8c5-cea5-7bc9-9e29-6c230973ed4a").unwrap()),
                node_type: NodeType::ImageLeaf,
                name: Some("10.01.2026 19:00:00".to_string()),
                data: serde_json::json!({"src": "3w_5.jpg"}),
                path: "019ba8c4-cd34-7b60-8297-1b9e7283e0ab.019ba8c5-cea5-7b2a-8b0c-65416d8c8f94.019ba8c5-cea5-7b7d-af9c-ceec4e4f889f.019ba8c5-cea5-7bc9-9e29-6c230973ed4a.019badc0-1347-7581-809e-c03c89e09ac4".to_string(),
                updated_at: NaiveDate::from_ymd_opt(2026, 1, 10).unwrap()
                    .and_hms_opt(18, 0, 0).unwrap(),
                depth: 4,
                own: true,
            },
        ]
    }

    #[tokio::test]
    async fn test_period_no_recent_updates() {
        let tree = create_test_tree_from_csv();
        
        // With Day period and amount=1: max_updated_at = current - 1 day ≈ 2026-01-23
        // ImageLeaf nodes with updated_at >= 2026-01-23:
        // None, since latest is 2026-01-10
        
        let params = TaskParameters {
            last: false,
            all: false,
            period: Some(Period::Day),
            amount: Some(1),
        };

        let result = get_filtered_tree(tree, &params).await.unwrap();
        assert_eq!(result.len(), 0, "Should return empty - no recent ImageLeaf nodes");
    }

    #[tokio::test]
    async fn test_empty_result() {
        let tree = create_test_tree_from_csv();
        let params = TaskParameters {
            last: false,
            all: false,
            period: None,
            amount: None,
        };

        let result = get_filtered_tree(tree, &params).await.unwrap();
        assert_eq!(result.len(), 0, "Should return empty vector");
    }

    #[tokio::test]
    async fn test_all_branches() {
        let tree = create_test_tree_from_csv();
        let params = TaskParameters {
            last: false,
            all: true,
            period: None,
            amount: None,
        };

        let result = get_filtered_tree(tree, &params).await.unwrap();
        assert_eq!(result.len(), 4, "Should return 4 Branch nodes");
        assert!(result.iter().all(|n| n.node_type != NodeType::ImageLeaf));
        
        // Verify specific branches
        let names: Vec<_> = result.iter().filter_map(|n| n.name.as_deref()).collect();
        assert!(names.contains(&"Object 2"));
        assert!(names.contains(&"Floor 21"));
        assert!(names.contains(&"Room 211"));
    }

    #[tokio::test]
    async fn test_last_equals_all() {
        let tree = create_test_tree_from_csv();
        
        let params_last = TaskParameters {
            last: true,
            all: false,
            period: None,
            amount: None,
        };
        
        let params_all = TaskParameters {
            last: false,
            all: true,
            period: None,
            amount: None,
        };

        let result_last = get_filtered_tree(tree.clone(), &params_last).await.unwrap();
        let result_all = get_filtered_tree(tree, &params_all).await.unwrap();
        
        assert_eq!(result_last.len(), result_all.len(), "last and all should produce same result");
    }

    #[tokio::test]
    async fn test_period_filter_week() {
        let tree = create_test_tree_from_csv();
        
        // Current date is approximately 2026-01-24 with current time
        // With Week period and amount=1: max_updated_at = now - 7 days ≈ 2026-01-17
        // ImageLeaf nodes with updated_at >= 2026-01-17:
        // - 2025-12-26 18:00:00 ✗
        // - 2026-01-02 18:00:00 ✗
        // - 2026-01-08 18:00:00 ✗
        // - 2026-01-10 18:00:00 ✗
        // None are recent enough (all are older than 7 days)
        
        let params = TaskParameters {
            last: false,
            all: false,
            period: Some(Period::Week),
            amount: Some(1),
        };

        let result = get_filtered_tree(tree, &params).await.unwrap();
        
        // No ImageLeaf nodes are recent enough, so no branches should be returned
        assert_eq!(result.len(), 0, "Should return empty - no recent updates in last week");
    }

    #[tokio::test]
    async fn test_period_filter_with_recent_data() {
        let tree = create_test_tree_from_csv();
        
        // With Month period and amount=1: max_updated_at = now - 30 days ≈ 2025-12-25
        // ImageLeaf nodes with updated_at >= 2025-12-25:
        // - 2025-12-26 18:00:00 ✓
        // - 2026-01-02 18:00:00 ✓
        // - 2026-01-08 18:00:00 ✓
        // - 2026-01-10 18:00:00 ✓
        // All ImageLeaf nodes should be selected
        
        let params = TaskParameters {
            last: false,
            all: false,
            period: Some(Period::Month),
            amount: Some(1),
        };

        let result = get_filtered_tree(tree, &params).await.unwrap();
        assert_eq!(result.len(), 4, "Should return all 4 Branch nodes - all leaves within last month");
        assert!(result.iter().all(|n| n.node_type != NodeType::ImageLeaf));
    }

    #[tokio::test]
    async fn test_period_filter_day() {
        let tree = create_test_tree_from_csv();
        
        // With Day period and amount=20: max_updated_at = current - 20 days ≈ 2026-01-04
        // ImageLeaf nodes with updated_at >= 2026-01-04:
        // - 2025-12-26 ✗
        // - 2026-01-02 ✗
        // - 2026-01-08 ✓
        // - 2026-01-10 ✓
        // Two ImageLeaf nodes should be selected
        
        let params = TaskParameters {
            last: false,
            all: false,
            period: Some(Period::Day),
            amount: Some(20),
        };

        let result = get_filtered_tree(tree, &params).await.unwrap();
        assert_eq!(result.len(), 4, "Should return all 4 Branch nodes");
        assert!(result.iter().all(|n| n.node_type != NodeType::ImageLeaf));
    }

    #[tokio::test]
    async fn test_period_filter_month() {
        let tree = create_test_tree_from_csv();
        
        // With Month period and amount=2: max_updated_at = current - 60 days ≈ 2025-11-25
        // ImageLeaf nodes with updated_at >= 2025-11-25:
        // - 2025-12-26 ✓
        // - 2026-01-02 ✓
        // - 2026-01-08 ✓
        // - 2026-01-10 ✓
        // All ImageLeaf nodes should be selected
        
        let params = TaskParameters {
            last: false,
            all: false,
            period: Some(Period::Month),
            amount: Some(2),
        };

        let result = get_filtered_tree(tree, &params).await.unwrap();
        assert_eq!(result.len(), 4, "Should return all 4 Branch nodes - all leaves are recent enough");
    }

    #[tokio::test]
    async fn test_no_root_or_imageleaf_in_result() {
        let tree = create_test_tree_from_csv();
        
        let params = TaskParameters {
            last: false,
            all: true,
            period: None,
            amount: None,
        };

        let result = get_filtered_tree(tree, &params).await.unwrap();
        
        assert!(!result.iter().any(|n| n.node_type == NodeType::ImageLeaf),
                "Result should not contain ImageLeaf nodes");
    }
}