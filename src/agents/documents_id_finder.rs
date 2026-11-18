// src/agents/document_agent.rs
//
// Retrieves and manages document objects based on user requests.
// Supports parameters for filtering and limiting results.
// Used to request 1-2 or several specific leafs id for a period.
// Result: id of 1 or 2 owner leafs/photos for a period.
// Sorted by update time desc. If 2 - with the oldest date of update

use std::sync::Arc;
use uuid::Uuid;

use crate::agents::agent_error::AgentError;
use crate::agents::document_agent::get_documents;
use crate::agents::object_id_finder::find_named_node;
use crate::agents::{ReportPair, UserContext};
use crate::db::{NodeType, NodeWithLeaf};
use crate::{AgentContext, AppState, TaskParameters};

pub(crate) async fn resolve_named_reports(
    pool: &sqlx::PgPool,
    context: &mut UserContext,
    object_identifier: Option<&str>,
    report_identifiers: &[String],
) -> Result<(), AgentError> {
    let mut parent_id = if let Some(identifier) = object_identifier {
        Some(
            find_named_node(pool, &context.user_id, NodeType::Branch, None, identifier)
                .await?
                .id,
        )
    } else {
        context
            .object_id
            .as_deref()
            .map(|id| {
                Uuid::parse_str(id).map_err(|_| AgentError::InvalidUuid {
                    raw: id.to_string(),
                })
            })
            .transpose()?
    };

    let mut reports = Vec::with_capacity(report_identifiers.len());
    for identifier in report_identifiers {
        let report = find_named_node(
            pool,
            &context.user_id,
            NodeType::ImageLeaf,
            parent_id,
            identifier,
        )
        .await?;
        if reports.iter().any(|(_, id)| *id == report.id) {
            return Err(AgentError::InvalidRequest);
        }
        parent_id = report.parent_id;
        reports.push((report.updated_at, report.id));
    }

    reports.sort_unstable_by(|a, b| b.cmp(a));
    context.object_id = parent_id.map(|id| id.to_string());
    context.current_report_id = reports.first().map(|(_, id)| id.to_string());
    context.previous_report_id = reports.get(1).map(|(_, id)| id.to_string());
    Ok(())
}

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

        images.sort_by_key(|p| std::cmp::Reverse(p.updated_at));

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
