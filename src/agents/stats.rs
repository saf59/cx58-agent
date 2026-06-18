use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerStat {
    pub worker_type: String,
    pub execution_time_ms: u64,
    pub llm_calls: u32,
    pub tokens_used: Option<u64>,
}

//noinspection ALL
/// Accumulated statistics for a single agent request lifecycle.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AgentStats {
    /// Tokens consumed by IntentRouter LLM call.
    pub router_tokens: Option<u64>,
    pub router_time: Option<u64>,
    /// Tokens consumed by Orchestrator LLM calls (may be called multiple times in a loop).
    pub orchestrator_tokens: Option<u64>,
    pub orchestrator_time: Option<u64>,
    pub orchestrator_call: Option<u32>,
    /// Per-worker execution stats.
    pub workers: Vec<WorkerStat>,
    /// Total wall-clock time for the whole request, filled by finalize().
    pub total_time_ms: u64,
    #[serde(skip)]
    started_at: Option<Instant>,
}

impl AgentStats {
    pub fn start() -> Self {
        Self {
            router_tokens: None,
            router_time: None,
            orchestrator_tokens: None,
            orchestrator_time: None,
            orchestrator_call: None,
            workers: Vec::new(),
            total_time_ms: 0,
            started_at: Some(Instant::now()),
        }
    }

    /// Records token usage from the IntentRouter classification call.
    pub fn record_router(&mut self, tokens: Option<u64>, time: Option<u64>) {
        self.router_tokens = tokens;
        self.router_time = time;
    }

    /// Accumulates token usage from Orchestrator (called in a loop, hence +=).
    pub fn record_orchestrator(&mut self, tokens: Option<u64>, time: Option<u64>) {
        *self.orchestrator_tokens.get_or_insert(0) += tokens.unwrap_or(0);
        *self.orchestrator_time.get_or_insert(0) += time.unwrap_or(0);
        *self.orchestrator_call.get_or_insert(0) += 1;
    }

    /// Records a completed worker execution with its timing and optional token usage.
    pub fn record_worker(
        &mut self,
        worker_type: &str,
        execution_time_ms: u64,
        llm_calls: u32,
        tokens_used: Option<u64>,
    ) {
        self.workers.push(WorkerStat {
            worker_type: worker_type.to_string(),
            execution_time_ms,
            llm_calls,
            tokens_used,
        });
    }

    /// Finalizes total elapsed time. Call once before sending Completed event.
    pub fn finalize(&mut self) {
        if let Some(started) = self.started_at {
            self.total_time_ms = started.elapsed().as_millis() as u64;
        }
    }

    pub fn total_tokens(&self) -> u64 {
        let router = self.router_tokens.unwrap_or(0);
        let orch = self.orchestrator_tokens.unwrap_or(0);
        let workers: u64 = self.workers.iter().filter_map(|w| w.tokens_used).sum();
        router + orch + workers
    }
}
