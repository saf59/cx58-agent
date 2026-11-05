// ============================================================================
// CANCELLATION TOKEN
// ============================================================================

use crate::agents::agent_error::AgentError;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Notify, RwLock};

/// Shared cancellation state with race-free waiter notification.
#[derive(Debug)]
struct CancellationState {
    cancelled: AtomicBool,
    notify: Notify,
}

#[derive(Clone, Debug)]
pub struct CancellationToken {
    state: Arc<CancellationState>,
}
impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}
impl CancellationToken {
    pub fn new() -> Self {
        Self {
            state: Arc::new(CancellationState {
                cancelled: AtomicBool::new(false),
                notify: Notify::new(),
            }),
        }
    }

    pub async fn cancel(&self) {
        if !self.state.cancelled.swap(true, Ordering::AcqRel) {
            self.state.notify.notify_waiters();
        }
    }

    pub async fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire)
    }

    /// Waits until cancellation is requested without polling.
    pub async fn cancelled(&self) {
        loop {
            let notified = self.state.notify.notified();
            if self.state.cancelled.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn completed_request_cannot_be_cancelled_after_unregister() {
        let manager = RequestManager::new();
        manager.register("request-1".to_string()).await;

        manager.unregister("request-1").await;
        assert!(!manager.cancel("request-1").await);
    }

    /// Verifies that an active waiter wakes as soon as cancellation is requested.
    #[tokio::test]
    async fn cancellation_wakes_waiters() {
        let token = CancellationToken::new();
        let waiter = token.clone();
        let task = tokio::spawn(async move { waiter.cancelled().await });

        token.cancel().await;

        tokio::time::timeout(std::time::Duration::from_millis(100), task)
            .await
            .expect("cancellation waiter timed out")
            .expect("cancellation waiter panicked");
    }
}
