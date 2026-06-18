// src/agents/document_agent.rs
//
// Retrieves and manages document objects based on user requests.
// Supports parameters for filtering and limiting results.
// Used to request 1-2 or several specific leafs id for a period.
// Result: id of 1 or 2 owner leafs/photos for a period.
// Sorted by update time desc. If 2 - with the oldest date of update

use std::sync::Arc;
use uuid::Uuid;

use crate::agents::ReportPair;
use crate::agents::agent_error::AgentError;
use crate::agents::document_agent::get_documents;
use crate::db::{NodeType, NodeWithLeaf};
use crate::{AgentContext, AppState, TaskParameters};

pub struct DocumentsIdFinder {
    context: AgentContext,
}

impl DocumentsIdFinder {
    pub fn new(context: AgentContext) -> Self {
        Self { context }
    }

    pub async fn execute(
        &self,
        state: Arc<AppState>,
        parameters: &TaskParameters,
        node_id_str: &str,
    ) -> Result<ReportPair, AgentError> {
        let pool = &state.db;
        tracing::info!("TaskParameters received: {:?}", parameters);
        // Validate UUID format.
        let node_id = Uuid::parse_str(node_id_str).map_err(|_| AgentError::InvalidUuid {
            raw: node_id_str.to_string(),
        })?;

        let data = get_documents(&parameters, pool, node_id).await?;
        tracing::info!(
            "DocumentsIdFinder: get_documents returned {} records",
            data.len()
        );

        let mut images: Vec<NodeWithLeaf> = data
            .into_iter()
            .filter(|n| n.node_type == NodeType::ImageLeaf)
            .collect();

        // No ImageLeaf results found for this node.
        if images.is_empty() {
            tracing::warn!(
                node_id = ?node_id,
                user_id = %self.context.user_id,
                "DocumentsIdFinder: no image leafs found for node"
            );
            return Err(AgentError::NoDocumentsFound);
        }

        images.sort_by_key(|p| std::cmp::Reverse(p.updated_at.clone()));

        let first_id = images.first().unwrap().id.to_string();
        if images.len() > 1 {
            let last_id = images.last().unwrap().id.to_string();
            Ok(ReportPair {
                prev: first_id,
                next: Some(last_id),
            })
        } else {
            Ok(ReportPair {
                prev: first_id,
                next: None,
            })
        }
    }
}
