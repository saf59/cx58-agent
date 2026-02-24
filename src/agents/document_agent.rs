// src/agents/document_agent.rs
//
// Retrieves and manages document objects based on user requests.
// Supports parameters for filtering and limiting results.
// Used to request 1-2 or several specific leafs/photos for a period.
// Result: 1 owner with leaves

use chrono::Utc;
use rig::providers::ollama;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::agents::LocalizationManager;
use crate::agents::agent_error::AgentError;
use crate::db::{NodeType, get_node_with_leafs};
use crate::storage::set_storage_url;
use crate::{AgentContext, AppState, StreamEvent, TaskParameters};

const MAX_DOCUMENTS_ALL: i32 = 100;
pub struct DocumentAgent {
    #[allow(unused)]
    client: Arc<ollama::Client>,
    context: AgentContext,
    event_tx: mpsc::Sender<StreamEvent>,
    lang_manager: Arc<LocalizationManager>,
}

impl DocumentAgent {
    pub fn new(
        client: Arc<ollama::Client>,
        context: AgentContext,
        event_tx: mpsc::Sender<StreamEvent>,
        lang_manager: Arc<LocalizationManager>,
    ) -> Self {
        Self {
            client,
            context,
            event_tx,
            lang_manager,
        }
    }

    pub async fn execute(
        &self,
        state: Arc<AppState>,
        parameters: &TaskParameters,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
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

        let limit = if parameters.all { MAX_DOCUMENTS_ALL } else { 2 };

        let mut data = if let Some(period) = &parameters.period {
            let to = Utc::now().naive_utc();
            let amount = parameters.amount.unwrap_or(1);
            let days = period.to_days() * amount as i64;
            let from = to - chrono::Duration::days(days);
            get_node_with_leafs(pool, node_id, Some(limit), Some(from), Some(to)).await?
        } else {
            get_node_with_leafs(pool, node_id, Some(limit), None, None).await?
        };

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
