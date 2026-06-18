use crate::agents::LocalizationManager;
use crate::agents::agent_error::AgentError;
use crate::agents::filter_objects::get_filtered_tree;
use crate::db::get_tree;
use crate::{AgentContext, AppState, StreamEvent, TaskParameters};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct ObjectAgent {
    context: AgentContext,
    event_tx: mpsc::Sender<StreamEvent>,
    lang_manager: Arc<LocalizationManager>,
}

/// It is not real AI agent at all.
/// Just parser of parameters, get and return tree. (with_tree=false)
/// Input for querying a tree without leaves.
/// Used to list objects that have changed over a given period.
impl ObjectAgent {
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
    ) -> Result<serde_json::Value, AgentError> {
        let tree = get_tree(&state.db, &self.context.user_id, true).await?;
        let filtered = get_filtered_tree(tree, parameters).await?;
        if filtered.is_empty() {
            tracing::warn!(
                "ObjectAgent: no objects found for user {}",
                self.context.user_id
            );
            let err = AgentError::NoDocumentsFound;
            err.send_to_client(&self.event_tx, &self.context, &self.lang_manager)
                .await;
            return Err(err);
        }
        let json_data = json!(filtered);
        Ok(json_data)
    }
}
