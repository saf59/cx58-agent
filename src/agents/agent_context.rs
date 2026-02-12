// ============================================================================
// REQUEST STRUCTURES
// ============================================================================

use crate::{AgentRequest, CancellationToken};

#[derive(Debug, Clone)]
pub struct AgentContext {
    pub message: String,
    pub request_id: String,
    pub user_id: String,
    pub chat_id: String,
    pub language: String,
    pub object_id: Option<String>,
    pub prev_leaf: Option<String>,
    pub next_leaf: Option<String>,
    pub metadata: serde_json::Value,
    pub cancellation_token: CancellationToken,
}

impl AgentContext {
    pub fn from_request(request_id: String, req: AgentRequest, cancellation_token: CancellationToken) -> Self {
        Self {
            message: req.message,
            request_id,
            user_id: req.user_id,
            chat_id: req.chat_id,
            language: req.language,
            object_id: req.object_id,
            prev_leaf: req.prev_leaf,
            next_leaf: req.next_leaf,
            metadata: req.metadata.unwrap_or(serde_json::json!({})),
            cancellation_token,
        }
    }
}
