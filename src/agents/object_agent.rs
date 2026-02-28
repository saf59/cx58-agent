use crate::agents::agent_error::AgentError;
use crate::agents::filter_objects::get_filtered_tree;
use crate::db::get_tree;
use crate::{AgentContext, AppState, TaskParameters};
use serde_json::json;
use std::sync::Arc;

pub struct ObjectAgent {
    context: AgentContext,
}
/// It is not real AI agent at all.
/// Just parser of parameters, get and return tree. ( with_tree=false )
/// Input for querying a tree without leaves
/// Used to list objects that have changed over a given period
impl ObjectAgent {
    pub fn new(
        context: AgentContext,
    ) -> Self {
        Self {
            context,
        }
    }

    pub async fn execute(
        &self,
        state:Arc<AppState>,
        parameters: &TaskParameters,
    ) -> Result<serde_json::Value, AgentError> {

        let tree = get_tree(&state.db, &self.context.user_id, true).await?;
        let filtered = get_filtered_tree(tree, parameters).await?;
        if filtered.is_empty() {
            tracing::warn!("ObjectAgent: no objects found for user {}", self.context.user_id);
            return Err(AgentError::NoDocumentsFound);
        }
        let json_data = json!(filtered);
        Ok(json_data)
    }
}