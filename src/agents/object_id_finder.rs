use crate::agents::agent_error::AgentError;
use crate::db::get_tree;
use crate::{AgentContext, AppState};
use std::sync::Arc;

pub struct ObjectIdFinder {
    context: AgentContext,
}
/// It is not real AI agent at all.
/// Used to find object id by name in the list user of objects
impl ObjectIdFinder {
    pub fn new(context: AgentContext) -> Self {
        Self { context }
    }

    pub async fn execute(
        &self,
        state: Arc<AppState>,
        object_name: &str,
    ) -> Result<String, AgentError> {
        let tree = get_tree(&state.db, &self.context.user_id, true).await?;
        let object_name_lower = object_name.to_lowercase();

        let branch = tree.iter().find(|it| {
            it.own
                && it.node_type == crate::db::NodeType::Branch
                && it.name.as_deref().map(|n| n.to_lowercase()) == Some(object_name_lower.clone())
        });

        if branch.is_none() {
            tracing::warn!("ObjectIdFinder: object with name {} not found", object_name);
            return Err(AgentError::ObjectNotFound {
                id: object_name.to_string(),
            });
        }
        let uuid = branch.unwrap().id;
        Ok(uuid.to_string())
    }
}
