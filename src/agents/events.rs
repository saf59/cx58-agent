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
    CoordinatorThinking {
        request_id: String,
        message: String,
    },

    // Content generation events
    TextChunk {
        request_id: String,
        chunk: String,
    },

    // Specialized chunk events
    ObjectChunk {
        request_id: String,
        data: serde_json::Value,
    },

    DocumentChunk {
        request_id: String,
        data: serde_json::Value,
    },

    DescriptionChunk {
        request_id: String,
        data: serde_json::Value,
    },

    ComparisonChunk {
        request_id: String,
        data: serde_json::Value,
    },

    // Completion events
    Completed {
        request_id: String,
        final_result: String,
        timestamp: i64,
    },

    // Error events
    Error {
        request_id: String,
        error: String,
        recoverable: bool,
    },

    // Cancelled events
    Cancelled {
        request_id: String,
        reason: String,
    },
}