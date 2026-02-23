use serde::{Deserialize, Serialize};
use crate::agents::stats::AgentStats;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    // Lifecycle events
    Started {
        request_id: String,
        timestamp: i64,
    },
    // Coordinator events
    Progress {
        request_id: String,
        status: String,
        percent: u8,
        message: String,
    },
    // Specialized chunk events
    ObjectTree {
        request_id: String,
        data: serde_json::Value,
    },
    // Content generation events
    TextChunk {
        request_id: String,
        chunk: String,
    },

    ReportList {
        request_id: String,
        data: serde_json::Value,
    },

    Description {
        request_id: String,
        data: serde_json::Value,
    },

    Comparison {
        request_id: String,
        data: serde_json::Value,
    },
    // Completion events
    Completed {
        request_id: String,
        //final_result: String,
        //timestamp: i64,
        total_time_ms: u64,
        stats: AgentStats,
    },
    // Error events
    Error {
        request_id: String,
        error: String,
    },
    // Canceled events
    Cancelled {
        request_id: String,
        reason: String,
    },
}