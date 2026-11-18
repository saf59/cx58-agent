use crate::agents::agent_error::AgentError;
use crate::db::{NodeType, TreeNode, get_tree};
use crate::{AgentContext, AppState};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

pub struct ObjectIdFinder {
    context: AgentContext,
}
/// Resolves an object identifier within the user's accessible tree.
impl ObjectIdFinder {
    pub fn new(context: AgentContext) -> Self {
        Self { context }
    }

    pub async fn execute(
        &self,
        state: Arc<AppState>,
        object_name: &str,
    ) -> Result<String, AgentError> {
        find_named_node(
            &state.db,
            &self.context.user_id,
            NodeType::Branch,
            None,
            object_name,
        )
        .await
        .map(|node| node.id.to_string())
    }
}

/// Resolves an owned node by display name, falling back to its CX ID.
pub(crate) async fn find_named_node(
    pool: &PgPool,
    user_id: &str,
    node_type: NodeType,
    parent_id: Option<Uuid>,
    identifier: &str,
) -> Result<TreeNode, AgentError> {
    let is_report = node_type == NodeType::ImageLeaf;
    let tree = get_tree(pool, user_id, is_report).await?;
    let owned_objects: std::collections::HashSet<_> = tree
        .iter()
        .filter(|node| node.own && node.node_type == NodeType::Branch)
        .map(|node| node.id)
        .collect();
    let candidates: Vec<_> = tree
        .into_iter()
        .filter(|node| {
            node.own
                && node.node_type == node_type
                && (!is_report
                    || node.parent_id.is_some_and(|parent| {
                        parent_id.is_none_or(|selected| selected == parent)
                            && owned_objects.contains(&parent)
                    }))
        })
        .collect();
    let identifier = identifier.trim();
    let key = ObjectNameKey::new(identifier);
    let mut named = candidates.iter().filter(|node| {
        node.name
            .as_deref()
            .is_some_and(|name| ObjectNameKey::new(name).matches(&key))
    });
    let ambiguous = || {
        if is_report {
            AgentError::AmbiguousReport {
                name: identifier.to_string(),
            }
        } else {
            AgentError::AmbiguousObject {
                name: identifier.to_string(),
            }
        }
    };
    if let Some(node) = named.next() {
        return if named.next().is_some() {
            Err(ambiguous())
        } else {
            Ok(node.clone())
        };
    }

    if !identifier.is_empty() && !identifier.contains('\0') && !candidates.is_empty() {
        let candidate_ids: Vec<_> = candidates.iter().map(|node| node.id).collect();
        let matches = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM tree_nodes WHERE id = ANY($1) AND lower(btrim(cx_id)) = lower($2) LIMIT 2",
        )
        .bind(candidate_ids)
        .bind(identifier)
        .fetch_all(pool)
        .await?;
        match matches.as_slice() {
            [id] => {
                if let Some(node) = candidates.into_iter().find(|node| node.id == *id) {
                    return Ok(node);
                }
            }
            [_, _] => return Err(ambiguous()),
            _ => {}
        }
    }
    Err(if is_report {
        AgentError::ReportNotFound {
            id: identifier.to_string(),
        }
    } else {
        AgentError::ObjectNotFound {
            id: identifier.to_string(),
        }
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ObjectNameKey {
    lowercase: String,
    ascii_folded: String,
    german_transliterated: String,
}

impl ObjectNameKey {
    fn new(value: &str) -> Self {
        let lowercase = value.to_lowercase();
        Self {
            ascii_folded: fold_german_umlauts(&lowercase, false),
            german_transliterated: fold_german_umlauts(&lowercase, true),
            lowercase,
        }
    }

    fn matches(&self, other: &Self) -> bool {
        self.lowercase == other.lowercase
            || self.ascii_folded == other.ascii_folded
            || self.german_transliterated == other.german_transliterated
    }
}

fn fold_german_umlauts(value: &str, transliterate: bool) -> String {
    let mut folded = String::with_capacity(value.len());

    for ch in value.chars() {
        match ch {
            'ä' if transliterate => folded.push_str("ae"),
            'ö' if transliterate => folded.push_str("oe"),
            'ü' if transliterate => folded.push_str("ue"),
            'ä' => folded.push('a'),
            'ö' => folded.push('o'),
            'ü' => folded.push('u'),
            'ß' => folded.push_str("ss"),
            _ => folded.push(ch),
        }
    }

    folded
}

#[cfg(test)]
mod tests {
    use super::ObjectNameKey;

    #[test]
    fn object_name_key_matches_german_umlaut_forms() {
        let canonical = ObjectNameKey::new("EG - Küche");

        assert!(canonical.matches(&ObjectNameKey::new("EG - Küche")));
        assert!(canonical.matches(&ObjectNameKey::new("eg - kuche")));
        assert!(canonical.matches(&ObjectNameKey::new("EG - Kueche")));
    }
}
