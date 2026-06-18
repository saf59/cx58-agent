use crate::agents::stats::AgentStats;
use serde::{Deserialize, Serialize};

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
    /// Sent when the agent needs missing context from the user before it can proceed.
    ///
    /// Triggered in exactly three situations:
    /// 1. `object_id` is absent from context (any intent that needs a tree node).
    /// 2. Intent is `DescribeReport` and neither `current_report_id` nor
    ///    `previous_report_id` is present.
    /// 3. Intent is `CompareReports` and both `current_report_id` and
    ///    `previous_report_id` are absent.
    ///
    /// The UI should render `prompt` as the main message and `suggestions` as a
    /// selectable list (e.g. chips / buttons).  Both fields are already
    /// localized to the request language.
    ContextRequest {
        request_id: String,
        /// Localized human-readable question for the user.
        prompt: String,
        /// Ordered list of localized hint strings the user can pick from.
        /// Empty when no suggestions are available.
        suggestions: Vec<String>,
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
