// ============================================================================
// CANCELLATION TOKEN
// ============================================================================

use crate::agents::agent_error::AgentError;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Debug)]
pub struct CancellationToken {
    cancelled: Arc<RwLock<bool>>,
}
impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}
impl CancellationToken {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(RwLock::new(false)),
        }
    }

    pub async fn cancel(&self) {
        let mut cancelled = self.cancelled.write().await;
        *cancelled = true;
    }

    pub async fn is_cancelled(&self) -> bool {
        *self.cancelled.read().await
    }

    pub async fn check(&self) -> Result<(), AgentError> {
        if self.is_cancelled().await {
            tracing::warn!("Operation cancelled");
            let err = AgentError::Cancelled;
            Err(err)
        } else {
            Ok(())
        }
    }
}

// ============================================================================
// REQUEST MANAGER
// ============================================================================

pub struct RequestManager {
    active_requests: Arc<RwLock<HashMap<String, CancellationToken>>>,
}
impl Default for RequestManager {
    fn default() -> Self {
        Self::new()
    }
}
impl RequestManager {
    pub fn new() -> Self {
        Self {
            active_requests: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register(&self, request_id: String) -> CancellationToken {
        let token = CancellationToken::new();
        tracing::info!("Register request: {}", request_id);
        {
            let mut requests = self.active_requests.write().await;
            requests.insert(request_id, token.clone());
        }
        token
    }

    pub async fn cancel(&self, request_id: &str) -> bool {
        let requests = self.active_requests.read().await;
        if let Some(token) = requests.get(request_id) {
            token.cancel().await;
            true
        } else {
            false
        }
    }

    pub async fn unregister(&self, request_id: &str) {
        let mut requests = self.active_requests.write().await;
        requests.remove(request_id);
    }
}
