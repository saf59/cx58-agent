// ============================================================================
// REQUEST STRUCTURES
// ============================================================================

use crate::{AgentRequest, CancellationToken};
use crate::agents::{Language, UserContext};

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
    /// Converts this request context into the routing `UserContext`.
    ///
    /// # Field mapping (intentional inversion)
    ///
    /// The front-end uses "leaf" terminology relative to the UI tree:
    /// - `prev_leaf` — the leaf *above* in the UI = the **current** (newer) report
    /// - `next_leaf` — the leaf *below* in the UI = the **previous** (older) report
    ///
    /// The agent subsystem uses chronological naming:
    /// - `current_report_id`  ← `prev_leaf`  (newer, the one being viewed right now)
    /// - `previous_report_id` ← `next_leaf`  (older, used for comparisons)
    ///
    /// This is the **single authoritative place** for this mapping.
    /// Do not reproduce it anywhere else.
    pub fn to_user_context(&self) -> UserContext {
        let language = Language::from_short(&self.language);
        UserContext {
            user_id: self.user_id.clone(),
            chat_id: self.chat_id.clone(),
            language,
            object_id: self.object_id.clone(),
            current_report_id: self.prev_leaf.clone(),
            previous_report_id: self.next_leaf.clone(),
        }
    }
}
