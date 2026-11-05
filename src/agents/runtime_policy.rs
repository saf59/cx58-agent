use crate::agents::CancellationToken;
use crate::agents::agent_error::AgentError;
use std::future::Future;
use std::time::Duration;
use tokio::sync::{Semaphore, SemaphorePermit};

pub const TEXT_LLM_TIMEOUT: Duration = Duration::from_secs(120);
pub const VISION_LLM_TIMEOUT: Duration = Duration::from_secs(240);
pub const STORAGE_TIMEOUT: Duration = Duration::from_secs(30);
pub const IMAGE_PROCESS_TIMEOUT: Duration = Duration::from_secs(30);
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

pub static TEXT_LLM_SLOTS: Semaphore = Semaphore::const_new(4);
pub static VISION_LLM_SLOTS: Semaphore = Semaphore::const_new(1);
pub static IMAGE_PROCESS_SLOTS: Semaphore = Semaphore::const_new(2);

/// Runs a fallible stage with cancellation and a hard deadline.
pub async fn run_cancellable<T, F>(
    token: &CancellationToken,
    timeout: Duration,
    stage: &'static str,
    future: F,
) -> Result<T, AgentError>
where
    F: Future<Output = Result<T, AgentError>>,
{
    tokio::select! {
        _ = token.cancelled() => Err(AgentError::Cancelled),
        result = tokio::time::timeout(timeout, future) => {
            result.map_err(|_| AgentError::DeadlineExceeded { stage })?
        }
    }
}

/// Acquires a bounded stage slot while remaining cancellable and time-limited.
pub async fn acquire_slot<'a>(
    token: &CancellationToken,
    semaphore: &'a Semaphore,
    timeout: Duration,
    stage: &'static str,
) -> Result<SemaphorePermit<'a>, AgentError> {
    run_cancellable(token, timeout, stage, async {
        semaphore
            .acquire()
            .await
            .map_err(|_| AgentError::internal("Runtime semaphore closed"))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::run_cancellable;
    use crate::agents::CancellationToken;
    use crate::agents::agent_error::AgentError;
    use std::time::Duration;

    /// Verifies that stage deadlines return a typed error.
    #[tokio::test]
    async fn stage_timeout_is_typed() {
        let token = CancellationToken::new();
        let result = run_cancellable(&token, Duration::from_millis(1), "test_stage", async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok::<_, AgentError>(())
        })
        .await;

        assert!(matches!(
            result,
            Err(AgentError::DeadlineExceeded {
                stage: "test_stage"
            })
        ));
    }

    /// Verifies that cancellation wins over a pending stage.
    #[tokio::test]
    async fn stage_can_be_cancelled() {
        let token = CancellationToken::new();
        token.cancel().await;
        let result = run_cancellable(&token, Duration::from_secs(1), "test_stage", async {
            std::future::pending::<()>().await;
            Ok::<_, AgentError>(())
        })
        .await;

        assert!(matches!(result, Err(AgentError::Cancelled)));
    }
}
