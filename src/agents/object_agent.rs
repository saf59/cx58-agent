use std::sync::Arc;
use rig::providers::ollama;
use tokio::sync::mpsc;
use serde_json::json;
use crate::{AgentContext, AppState, StreamEvent, TaskParameters};
use crate::agents::filter_objects::get_filtered_tree;
use crate::db::{get_tree};

pub struct ObjectAgent {
    #[allow(unused)]
    client: Arc<ollama::Client>,
    context: AgentContext,
    event_tx: mpsc::Sender<StreamEvent>,
}
/// It is not real AI agent at all.
/// Just parser of parameters, get and return tree. ( with_tree=false )
/// Input for querying a tree without leaves
/// Used to list objects that have changed over a given period
impl ObjectAgent {
    pub fn new(
        client: Arc<ollama::Client>,
        context: AgentContext,
        event_tx: mpsc::Sender<StreamEvent>,
    ) -> Self {
        Self {
            client,
            context,
            event_tx,
        }
    }

    async fn send_event(&self, event: StreamEvent) {
        let _ = self.event_tx.send(event).await;
    }

    pub async fn execute(
        &self,
        state:Arc<AppState>,
        parameters: &TaskParameters,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {

        let tree = get_tree(&state.db, &self.context.user_id, true).await?;
        let filtered = get_filtered_tree(tree, parameters).await?;
        if filtered.is_empty() {
            self.send_event(StreamEvent::TextChunk {
                request_id: self.context.request_id.clone(),
                chunk: "No objects found matching the criteria.\n".to_string(),
            })
            .await;
            return Ok("No objects found.".to_string());
        }
        let json_data = json!(filtered);
        tracing::info!("Retrieved object tree for user {}: {:?}", self.context.user_id, json_data);
        self.send_event(StreamEvent::ObjectTree {
            request_id: self.context.request_id.clone(),
            data: json_data,
        })
            .await;

        let response = "Object retrieval completed.".to_string();

        Ok(response)
    }
}