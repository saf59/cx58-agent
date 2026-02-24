use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::agents::Period;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationResult {
    pub intent: Intent,
    pub confidence: f32,
    pub extracted_parameters: ExtractedParameters,
    pub missing_context: Vec<ContextField>,
    pub reasoning: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Intent {
    GetObjectTree,
    GetReportList,
    DescribeReport,
    CompareReports,
    RagQuery,
    OutOfScope,
    Ambiguous,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedParameters {
    #[serde(deserialize_with = "deserialize_task_parameters_opt")]
    pub task_params: Option<TaskParameters>,
    pub object_identifier: Option<String>, // "Building A", "Site 123", etc.
    pub time_reference: Option<String>, // "last week", "yesterday", etc.
    pub report_references: Vec<String>, // "latest", "from Monday", etc.
}

/// Custom deserializer for Optional TaskParameters
fn deserialize_task_parameters_opt<'de, D>(deserializer: D) -> Result<Option<TaskParameters>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<serde_json::Value>::deserialize(deserializer)?;
    match opt {
        Some(v) => {
            if v.is_null() {
                Ok(None)
            } else {
                // Check if it's an empty object {}
                if let serde_json::Value::Object(map) = &v && map.is_empty() {
                        return Ok(None);
                    }

                // Check if it's an object with all empty/default values
                // Note: we must also check 'all' and 'last' fields - they are not defaults!
                if let serde_json::Value::Object(map) = &v {
                    let has_no_meaningful_data = map.get("period").is_none_or( |p| {
                        p.is_null() || (p.is_string() && p.as_str().unwrap_or("").is_empty())
                    }) && map.get("amount").is_none_or( |a| {
                        a.is_null() || (a.is_number() && a.as_u64().unwrap_or(0) == 0)
                    }) && map.get("all").is_none_or( |a| {
                        a.is_null() || !a.is_boolean() || !a.as_bool().unwrap_or(false)
                    }) && map.get("last").is_none_or( |l| {
                        l.is_null() || !l.is_boolean() || !l.as_bool().unwrap_or(false)
                    });
                    if has_no_meaningful_data {
                        return Ok(None);
                    }
                }
                TaskParameters::deserialize(v).map(Some).map_err(|e| serde::de::Error::custom(e.to_string()))
            }
        }
        None => Ok(None),
    }
}
/// Custom deserializer for Period to handle empty strings as None
fn deserialize_period_opt<'de, D>(deserializer: D) -> Result<Option<Period>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    match opt {
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => match s.as_str() {
            "Day" => Ok(Some(Period::Day)),
            "Week" => Ok(Some(Period::Week)),
            "Month" => Ok(Some(Period::Month)),
            "Quarter" => Ok(Some(Period::Quarter)),
            "Year" => Ok(Some(Period::Year)),
            _ => Ok(None),
            //_ => Err(serde::de::Error::unknown_variant(&s, &["Day", "Week", "Month", "Quarter", "Year"])),
        },
        None => Ok(None),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskParameters {
    pub last: bool,
    pub all: bool,
    #[serde(deserialize_with = "deserialize_period_opt")]
    pub period: Option<Period>,
    pub amount: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserContext {
    pub user_id: String,
    pub chat_id: String,
    pub language: Language,
    pub object_id: Option<String>,
    #[doc = "Maps to current_report_id (inverted from AgentRequest.prev_leaf)"]
    pub current_report_id: Option<String>,
    #[doc = "Maps to previous_report_id (inverted from AgentRequest.next_leaf)"]
    pub previous_report_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ContextField {
    ObjectId,
    CurrentReportId,
    PreviousReportId,
}
#[derive(Debug, Clone)]
pub struct WorkerRequest {
    pub worker_type: WorkerType,
    pub parameters: WorkerParameters,
    pub context: WorkerContext,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportPair {
    pub prev: String,
    pub next: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
//#[serde(tag = "chunk_type", content = "data")]
pub enum WorkerParameters {
    GetObjectTree(TaskParameters),
    GetReportList {
        object_id: String,
        task_params: TaskParameters,
    },
    DescribeReport {
        reports: ReportPair,
    },
    CompareReports {
        reports: Value,
    },
    RagQuery {
        query: String,
    },
}
// Orchestrator Decision
#[derive(Debug, Clone)]
pub enum OrchestratorDecision {
    ExecuteWorker(WorkerRequest),
    RequestContextFromUser {
        missing_field: ContextField,
        prompt: String,
        suggestions: Vec<String>,
    },
    SendProgress {
        status: String,
        percent: u8,
        message: String,
    },
    FormatAndReturn {
        worker_results: Vec<WorkerResponse>,
    },
    Reject {
        reason: String,
        message: String,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
//#[serde(tag = "chunk_type", content = "data")]
pub enum WorkerType {
    GetObjectTree,
    GetReportList,
    DescribeReport,
    CompareReports,
    RagQuery,
}

#[derive(Debug, Clone)]
pub struct WorkerContext {
    pub user_id: String,
    pub language: Language,
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerResponse {
    pub worker_type: WorkerType,
    pub status: WorkerStatus,
    pub data: serde_json::Value,
    pub metadata: WorkerMetadata,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WorkerStatus {
    Success,
    PartialSuccess,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerMetadata {
    pub execution_time_ms: u64,
    pub data_source: String,
    pub cache_hit: bool,
    pub tokens_used: Option<u64>,
    pub llm_calls: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Language {
    #[serde(rename = "en")]
    English,
    #[serde(rename = "de")]
    German,
}

impl Language {
    pub fn from_short(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "de" | "german" => Language::German,
            _ => Language::English,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Language::English => "English",
            Language::German => "German",
        }
    }

    pub fn to_code(&self) -> &str {
        match self {
            Language::English => "en",
            Language::German => "de",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequest {
    pub message: String,
    pub user_id: String,
    pub chat_id: String,
    pub language: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_id: Option<String>,
    //#[serde(skip_serializing_if = "Option::is_none")]
    //pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev_leaf: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_leaf: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    pub ai_config: AiConfig,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    pub url: String,
    pub text_model: String,
    pub vision_model: String,
    pub chat_model: String,
    pub agent_secret: String
}

