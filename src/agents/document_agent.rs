use std::sync::Arc;
use chrono::Utc;
use rig::providers::ollama;
use tokio::sync::mpsc;
use serde_json::json;
use uuid::Uuid;
use crate::{AgentContext, AppState, StreamEvent, TaskParameters};
use crate::db::{get_node_with_leafs, NodeType};
use crate::storage::set_storage_url;

pub struct DocumentAgent {
    #[allow(unused)]
    client: Arc<ollama::Client>,
    context: AgentContext,
    event_tx: mpsc::Sender<StreamEvent>,
}
/// Retrieves and manages document objects based on user requests.
/// Supports parameters for filtering and limiting results.
/// Used to request 1-2 or several specific leafs/photos for a period.
/// Result: 1 owner with leaves
impl DocumentAgent {
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

        let pool = &state.db;
        let node_id: &str = self.context.object_id.as_ref()
            .expect("Object ID is required for DescriptionAgent");
        let node_id = Uuid::parse_str(node_id).expect("Invalid UUID format for Object ID");
        let limit = if parameters.all { 100 } else { 2 };
        let mut data = if let Some(period) = &parameters.period {
            let from = Utc::now().naive_utc();
            let amount = parameters.amount.unwrap_or(1);
            let days = period.to_days() * amount as i64;
            let to = from + chrono::Duration::days(days);
            get_node_with_leafs(pool, node_id, Some(limit), Some(from), Some(to)).await?
        } else {
            get_node_with_leafs(pool, node_id, Some(limit), None, None).await?
        };
        if data.is_empty() {
            self.send_event(StreamEvent::TextChunk {
                request_id: self.context.request_id.clone(),
                chunk: "No documents found matching the criteria.\n".to_string(),
            })
            .await;
            return Ok("No documents found.".to_string());
        }
        // Process ImageLeaf nodes
        for node in &mut data {
            if matches!(node.node_type, NodeType::ImageLeaf)
                && let Some(obj) = node.data.as_object_mut()
            {
                let node_id = &node.id;
                set_storage_url(state.clone(), obj, node_id).await;
            }
        }
        let json_data = json!(data);
        self.send_event(StreamEvent::DocumentChunk {
            request_id: self.context.request_id.clone(),
            data: json_data,
        })
            .await;
        let response = "Document retrieval completed.".to_string();
        Ok(response)
    }
}