use crate::agents::AgentContext;
use crate::agents::agent_error::AgentError;
use crate::db::{NodeType, TreeNode, get_tree};
use sqlx::PgPool;
use uuid::Uuid;

/// Authorizes every explicit context node against the user's accessible tree.
pub async fn authorize_context(pool: &PgPool, context: &AgentContext) -> Result<(), AgentError> {
    if context.object_id.is_none() && context.prev_leaf.is_none() && context.next_leaf.is_none() {
        return Ok(());
    }

    let tree = get_tree(pool, &context.user_id, true).await?;
    authorize_context_nodes(
        &tree,
        context.object_id.as_deref(),
        context.prev_leaf.as_deref(),
        context.next_leaf.as_deref(),
    )
}

/// Validates ownership, node types, and report-to-object relationships.
fn authorize_context_nodes(
    tree: &[TreeNode],
    object_id: Option<&str>,
    current_report_id: Option<&str>,
    previous_report_id: Option<&str>,
) -> Result<(), AgentError> {
    let object = object_id
        .map(|raw| find_owned_node(tree, raw, NodeType::Branch))
        .transpose()?;

    for report_id in [current_report_id, previous_report_id]
        .into_iter()
        .flatten()
    {
        let report = find_owned_node(tree, report_id, NodeType::ImageLeaf)?;
        if let Some(object) = object
            && report.parent_id != Some(object.id)
        {
            return Err(AgentError::ObjectNotFound {
                id: report_id.to_string(),
            });
        }
    }

    Ok(())
}

/// Finds a user-owned node with the required type without revealing foreign IDs.
fn find_owned_node<'tree>(
    tree: &'tree [TreeNode],
    raw_id: &str,
    expected_type: NodeType,
) -> Result<&'tree TreeNode, AgentError> {
    let node_id = Uuid::parse_str(raw_id).map_err(|_| AgentError::InvalidUuid {
        raw: raw_id.to_string(),
    })?;

    tree.iter()
        .find(|node| node.id == node_id && node.own && node.node_type == expected_type)
        .ok_or_else(|| AgentError::ObjectNotFound {
            id: raw_id.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::authorize_context_nodes;
    use crate::agents::agent_error::AgentError;
    use crate::db::{NodeType, TreeNode};
    use chrono::NaiveDateTime;
    use uuid::Uuid;

    /// Creates a minimal tree node for access-control tests.
    fn node(id: Uuid, parent_id: Option<Uuid>, node_type: NodeType, own: bool) -> TreeNode {
        TreeNode {
            id,
            parent_id,
            node_type,
            name: Some("node".to_string()),
            data: serde_json::json!({}),
            path: id.to_string(),
            updated_at: NaiveDateTime::default(),
            depth: 1,
            own,
        }
    }

    /// Verifies that an owned report under the selected object is authorized.
    #[test]
    fn owned_report_under_selected_object_is_allowed() {
        let object_id = Uuid::now_v7();
        let report_id = Uuid::now_v7();
        let tree = vec![
            node(object_id, None, NodeType::Branch, true),
            node(report_id, Some(object_id), NodeType::ImageLeaf, true),
        ];

        assert!(
            authorize_context_nodes(
                &tree,
                Some(&object_id.to_string()),
                Some(&report_id.to_string()),
                None,
            )
            .is_ok()
        );
    }

    /// Verifies that visible ancestors without ownership cannot be selected directly.
    #[test]
    fn visible_but_unowned_object_is_rejected() {
        let object_id = Uuid::now_v7();
        let tree = vec![node(object_id, None, NodeType::Branch, false)];

        assert!(matches!(
            authorize_context_nodes(&tree, Some(&object_id.to_string()), None, None),
            Err(AgentError::ObjectNotFound { .. })
        ));
    }

    /// Verifies that a report from another owned object cannot cross context boundaries.
    #[test]
    fn report_from_another_owned_object_is_rejected() {
        let selected_object = Uuid::now_v7();
        let other_object = Uuid::now_v7();
        let report_id = Uuid::now_v7();
        let tree = vec![
            node(selected_object, None, NodeType::Branch, true),
            node(other_object, None, NodeType::Branch, true),
            node(report_id, Some(other_object), NodeType::ImageLeaf, true),
        ];

        assert!(matches!(
            authorize_context_nodes(
                &tree,
                Some(&selected_object.to_string()),
                Some(&report_id.to_string()),
                None,
            ),
            Err(AgentError::ObjectNotFound { .. })
        ));
    }

    /// Verifies that malformed identifiers fail before any node lookup succeeds.
    #[test]
    fn malformed_identifier_is_rejected() {
        assert!(matches!(
            authorize_context_nodes(&[], Some("not-a-uuid"), None, None),
            Err(AgentError::InvalidUuid { .. })
        ));
    }
}
