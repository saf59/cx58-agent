// src/agents/document_agent.rs
//
// Retrieves and manages document objects based on user requests.
// Supports parameters for filtering and limiting results.
// Used to request 1-2 or several specific leafs/photos for a period.
// Result: 1 owner with leaves

use chrono::{LocalResult, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Europe::Berlin;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::agents::LocalizationManager;
use crate::agents::agent_error::AgentError;
use crate::db::{NodeType, NodeWithLeaf, get_node_with_leafs};
use crate::storage::set_storage_url;
use crate::{AgentContext, AppState, StreamEvent, TaskParameters};

pub const MAX_DOCUMENTS_ALL: i32 = 366;
pub struct DocumentAgent {
    context: AgentContext,
    event_tx: mpsc::Sender<StreamEvent>,
    lang_manager: Arc<LocalizationManager>,
}

impl DocumentAgent {
    pub fn new(
        context: AgentContext,
        event_tx: mpsc::Sender<StreamEvent>,
        lang_manager: Arc<LocalizationManager>,
    ) -> Self {
        Self {
            context,
            event_tx,
            lang_manager,
        }
    }

    pub async fn execute(
        &self,
        state: Arc<AppState>,
        parameters: &TaskParameters,
    ) -> Result<Value, AgentError> {
        let pool = &state.db;

        // Validate object_id presence.
        let node_id_str = self
            .context
            .object_id
            .as_deref()
            .ok_or(AgentError::MissingObjectId)?;

        // Validate UUID format.
        let node_id = Uuid::parse_str(node_id_str).map_err(|_| AgentError::InvalidUuid {
            raw: node_id_str.to_string(),
        })?;

        let mut data = get_documents(&parameters, pool, node_id).await?;

        // No results — send a localized info message and return empty.
        if data.is_empty() {
            tracing::warn!(
                node_id = ?node_id,
                user_id = %self.context.user_id,
                "DocumentAgent: no documents found for node"
            );
            let err = AgentError::NoDocumentsFound;
            err.send_to_client(&self.event_tx, &self.context, &self.lang_manager)
                .await;
            return Err(err.into());
        }

        // Attach storage URLs to image-leaf nodes.
        for node in &mut data {
            if matches!(node.node_type, NodeType::ImageLeaf)
                && let Some(obj) = node.data.as_object_mut()
            {
                let node_id = &node.id;
                set_storage_url(state.clone(), obj, node_id).await;
            }
        }

        Ok(json!(data))
    }
}
pub async fn get_documents(
    parameters: &&TaskParameters,
    pool: &PgPool,
    node_id: Uuid,
) -> Result<Vec<NodeWithLeaf>, AgentError> {
    let limit = if parameters.all || parameters.exact_datetime.is_some() {
        MAX_DOCUMENTS_ALL
    } else {
        2
    };
    let amount = if parameters.all {
        MAX_DOCUMENTS_ALL as usize
    } else {
        parameters.amount.unwrap_or(1)
    };

    let data = if let Some(period) = &parameters.period {
        let to = Utc::now().naive_utc();
        let days = period.to_days() * amount as i64;
        let from = to - chrono::Duration::days(days);
        tracing::info!(
            "DocumentAgent: fetching documents with period filter - from {} to {}, limit {}, amount {}, period {:?}, days {}",
            from,
            to,
            limit,
            amount,
            period,
            days
        );
        get_node_with_leafs(pool, node_id, Some(limit), Some(from), Some(to)).await?
    } else {
        get_node_with_leafs(pool, node_id, Some(limit), None, None).await?
    };
    if let Some(exact_datetime) = parameters.exact_datetime {
        Ok(filter_exact_datetime(data, exact_datetime))
    } else {
        Ok(data)
    }
}

fn filter_exact_datetime(
    data: Vec<NodeWithLeaf>,
    exact_datetime: NaiveDateTime,
) -> Vec<NodeWithLeaf> {
    let exact_label = exact_datetime.format("%d.%m.%Y %H:%M:%S").to_string();
    let exact_short_label = exact_datetime.format("%d.%m.%Y %H:%M").to_string();
    let exact_utc_candidates = berlin_local_to_utc_candidates(exact_datetime);

    let (owners, image_leafs): (Vec<_>, Vec<_>) = data
        .into_iter()
        .partition(|node| node.node_type != NodeType::ImageLeaf);

    let matching_images: Vec<_> = image_leafs
        .into_iter()
        .filter(|node| {
            let name_matches = node.name.as_deref().is_some_and(|name| {
                name.contains(&exact_label) || name.contains(&exact_short_label)
            });
            let full_name_matches = node.full_name.as_deref().is_some_and(|name| {
                name.contains(&exact_label) || name.contains(&exact_short_label)
            });
            let updated_at_matches = exact_utc_candidates
                .iter()
                .any(|candidate| (node.updated_at - *candidate).num_seconds().abs() <= 60);

            name_matches || full_name_matches || updated_at_matches
        })
        .collect();

    if matching_images.is_empty() {
        Vec::new()
    } else {
        owners.into_iter().chain(matching_images).collect()
    }
}

fn berlin_local_to_utc_candidates(berlin_local: NaiveDateTime) -> Vec<NaiveDateTime> {
    match Berlin.from_local_datetime(&berlin_local) {
        LocalResult::Single(dt) => vec![dt.naive_utc()],
        LocalResult::Ambiguous(earliest, latest) => {
            vec![earliest.naive_utc(), latest.naive_utc()]
        }
        LocalResult::None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use serde_json::json;

    fn node(
        node_type: NodeType,
        name: &str,
        updated_at: NaiveDateTime,
        parent_id: Option<Uuid>,
    ) -> NodeWithLeaf {
        NodeWithLeaf {
            id: Uuid::now_v7(),
            parent_id,
            node_type,
            name: Some(name.to_string()),
            data: json!({}),
            path: "/test".to_string(),
            updated_at,
            full_name: Some(name.to_string()),
        }
    }

    #[test]
    fn exact_datetime_filter_keeps_owner_rows_for_carousel() {
        let exact =
            NaiveDateTime::parse_from_str("2026-05-22 20:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let branch_id = Uuid::now_v7();
        let branch = NodeWithLeaf {
            id: branch_id,
            parent_id: None,
            node_type: NodeType::Branch,
            name: Some("EG - Bad".to_string()),
            data: json!({}),
            path: "/eg-bad".to_string(),
            updated_at: exact,
            full_name: Some("EG - Bad".to_string()),
        };
        let matching = node(
            NodeType::ImageLeaf,
            "EG - Bad 22.05.2026 20:00:00",
            exact,
            Some(branch_id),
        );
        let other = node(
            NodeType::ImageLeaf,
            "EG - Bad 22.05.2026 17:00:00",
            exact - Duration::hours(3),
            Some(branch_id),
        );

        let filtered = filter_exact_datetime(vec![branch.clone(), matching.clone(), other], exact);

        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().any(|node| node.id == branch.id));
        assert!(filtered.iter().any(|node| node.id == matching.id));
    }

    #[test]
    fn exact_datetime_filter_returns_empty_when_no_image_matches() {
        let exact =
            NaiveDateTime::parse_from_str("2026-05-22 20:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let branch_id = Uuid::now_v7();
        let branch = NodeWithLeaf {
            id: branch_id,
            parent_id: None,
            node_type: NodeType::Branch,
            name: Some("EG - Bad".to_string()),
            data: json!({}),
            path: "/eg-bad".to_string(),
            updated_at: exact,
            full_name: Some("EG - Bad".to_string()),
        };
        let other = node(
            NodeType::ImageLeaf,
            "EG - Bad 22.05.2026 17:00:00",
            exact - Duration::hours(3),
            Some(branch_id),
        );

        let filtered = filter_exact_datetime(vec![branch, other], exact);

        assert!(filtered.is_empty());
    }

    #[test]
    fn exact_datetime_filter_treats_updated_at_as_berlin_local_time() {
        let exact =
            NaiveDateTime::parse_from_str("2026-05-22 20:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let branch_id = Uuid::now_v7();
        let branch = NodeWithLeaf {
            id: branch_id,
            parent_id: None,
            node_type: NodeType::Branch,
            name: Some("EG - Bad".to_string()),
            data: json!({}),
            path: "/eg-bad".to_string(),
            updated_at: exact,
            full_name: Some("EG - Bad".to_string()),
        };
        let matching = node(
            NodeType::ImageLeaf,
            "report without display datetime",
            NaiveDateTime::parse_from_str("2026-05-22 18:00:00", "%Y-%m-%d %H:%M:%S").unwrap(),
            Some(branch_id),
        );
        let ukraine_local_fallback = node(
            NodeType::ImageLeaf,
            "another report without display datetime",
            NaiveDateTime::parse_from_str("2026-05-22 17:00:00", "%Y-%m-%d %H:%M:%S").unwrap(),
            Some(branch_id),
        );

        let filtered = filter_exact_datetime(
            vec![branch, matching.clone(), ukraine_local_fallback],
            exact,
        );

        assert_eq!(
            filtered
                .iter()
                .filter(|node| node.node_type == NodeType::ImageLeaf)
                .count(),
            1
        );
        assert!(filtered.iter().any(|node| node.id == matching.id));
    }
}
