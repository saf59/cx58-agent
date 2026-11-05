use crate::agents::agent_error::AgentError;
use crate::agents::{
    ClassificationResult, ContextField, Intent, OrchestratorDecision, ReportPair, TaskParameters,
    UserContext, WorkerContext, WorkerParameters, WorkerRequest, WorkerResponse, WorkerType,
};
use serde_json::Value;

/// Pure workflow planner for deterministic agent state transitions.
pub struct DeterministicPlanner;

/// Implements deterministic routing without a control-plane model call.
impl DeterministicPlanner {
    /// Selects the next worker from intent, context, and completed worker results.
    pub fn decide(
        classification: &ClassificationResult,
        context: &UserContext,
        original_message: &str,
        request_id: &str,
        worker_results: &[WorkerResponse],
    ) -> Result<OrchestratorDecision, AgentError> {
        let worker = match classification.intent {
            Intent::GetObjectTree => Some((
                WorkerType::GetObjectTree,
                WorkerParameters::GetObjectTree {
                    task_params: task_params_or_default(classification, true),
                },
            )),
            Intent::GetReportList => {
                if context.object_id.is_none() {
                    object_resolution_worker(classification)?
                } else {
                    Some((
                        WorkerType::GetReportList,
                        WorkerParameters::GetReportList {
                            task_params: task_params_or_default(classification, false),
                        },
                    ))
                }
            }
            Intent::DescribeReport => {
                if context.object_id.is_none() {
                    object_resolution_worker(classification)?
                } else if context.current_report_id.is_none()
                    && context.previous_report_id.is_none()
                {
                    report_resolution_worker(classification, context)?
                } else {
                    let has_current_report = context.current_report_id.is_some();
                    let current = context
                        .current_report_id
                        .clone()
                        .or_else(|| context.previous_report_id.clone())
                        .ok_or(AgentError::MissingObjectId)?;
                    Some((
                        WorkerType::DescribeReport,
                        WorkerParameters::DescribeReport {
                            reports: ReportPair {
                                prev: current,
                                next: has_current_report
                                    .then(|| context.previous_report_id.clone())
                                    .flatten(),
                            },
                        },
                    ))
                }
            }
            Intent::CompareReports => {
                if context.object_id.is_none() {
                    object_resolution_worker(classification)?
                } else if context.current_report_id.is_none()
                    || context.previous_report_id.is_none()
                {
                    report_resolution_worker(classification, context)?
                } else if worker_results
                    .iter()
                    .any(|result| result.worker_type == WorkerType::DescribeReport)
                {
                    Some((
                        WorkerType::CompareReports,
                        WorkerParameters::CompareReports {
                            reports: Value::Null,
                        },
                    ))
                } else {
                    Some((
                        WorkerType::DescribeReport,
                        WorkerParameters::DescribeReport {
                            reports: ReportPair {
                                prev: context.current_report_id.clone().unwrap_or_default(),
                                next: context.previous_report_id.clone(),
                            },
                        },
                    ))
                }
            }
            Intent::RagQuery => Some((
                WorkerType::RagQuery,
                WorkerParameters::RagQuery {
                    query: original_message.to_string(),
                },
            )),
            Intent::OutOfScope => {
                return Ok(OrchestratorDecision::Reject {
                    reason: "out_of_scope".to_string(),
                    message: String::new(),
                });
            }
            Intent::Ambiguous => {
                return Ok(OrchestratorDecision::RequestContextFromUser {
                    missing_field: ContextField::ObjectId,
                    prompt: String::new(),
                    suggestions: Vec::new(),
                });
            }
        };

        let (worker_type, parameters) = worker.ok_or_else(|| {
            AgentError::internal("Deterministic planner could not select a worker")
        })?;
        Ok(OrchestratorDecision::ExecuteWorker(WorkerRequest {
            worker_type,
            parameters,
            context: WorkerContext {
                user_id: context.user_id.clone(),
                language: context.language.clone(),
                request_id: request_id.to_string(),
            },
        }))
    }
}

/// Creates an object-name resolution worker when no object ID is available.
fn object_resolution_worker(
    classification: &ClassificationResult,
) -> Result<Option<(WorkerType, WorkerParameters)>, AgentError> {
    let object_name = classification
        .extracted_parameters
        .object_identifier
        .clone()
        .ok_or(AgentError::MissingObjectId)?;
    Ok(Some((
        WorkerType::ObjectIdFinder,
        WorkerParameters::ObjectIdFinder { object_name },
    )))
}

/// Creates a report resolution worker from the selected object and task parameters.
fn report_resolution_worker(
    classification: &ClassificationResult,
    context: &UserContext,
) -> Result<Option<(WorkerType, WorkerParameters)>, AgentError> {
    let object_id = context
        .object_id
        .clone()
        .ok_or(AgentError::MissingObjectId)?;
    let task_params = classification
        .extracted_parameters
        .task_params
        .clone()
        .ok_or(AgentError::NoDocumentsFound)?;
    Ok(Some((
        WorkerType::DocumentsIdFinder,
        WorkerParameters::DocumentsIdFinder {
            task_params,
            object_id,
        },
    )))
}

/// Returns classifier parameters or a safe deterministic fallback.
fn task_params_or_default(
    classification: &ClassificationResult,
    tree_request: bool,
) -> TaskParameters {
    classification
        .extracted_parameters
        .task_params
        .clone()
        .unwrap_or(TaskParameters {
            last: !tree_request,
            all: tree_request,
            period: None,
            amount: (!tree_request).then_some(1),
            exact_datetime: None,
        })
}

#[cfg(test)]
mod tests {
    use super::DeterministicPlanner;
    use crate::agents::{
        ClassificationResult, ExtractedParameters, Intent, Language, OrchestratorDecision,
        TaskParameters, UserContext, WorkerMetadata, WorkerParameters, WorkerResponse,
        WorkerStatus, WorkerType,
    };

    /// Builds a classification fixture for planner transition tests.
    fn classification(intent: Intent) -> ClassificationResult {
        ClassificationResult {
            intent,
            confidence: 1.0,
            extracted_parameters: ExtractedParameters {
                task_params: Some(TaskParameters {
                    last: true,
                    all: false,
                    period: None,
                    amount: Some(2),
                    exact_datetime: None,
                }),
                object_identifier: Some("Kitchen".to_string()),
                time_reference: None,
                report_references: Vec::new(),
            },
            missing_context: Vec::new(),
            reasoning: String::new(),
        }
    }

    /// Builds a context fixture with optional resolved identifiers.
    fn context(object: bool, reports: bool) -> UserContext {
        UserContext {
            user_id: "user".to_string(),
            chat_id: "chat".to_string(),
            language: Language::English,
            object_id: object.then(|| "object".to_string()),
            current_report_id: reports.then(|| "current".to_string()),
            previous_report_id: reports.then(|| "previous".to_string()),
        }
    }

    /// Builds a completed description worker response.
    fn description_result() -> WorkerResponse {
        WorkerResponse {
            worker_type: WorkerType::DescribeReport,
            status: WorkerStatus::Success,
            data: serde_json::json!([]),
            metadata: WorkerMetadata {
                execution_time_ms: 0,
                data_source: "test".to_string(),
                cache_hit: true,
                tokens_used: None,
                llm_calls: 0,
            },
        }
    }

    /// Verifies the complete deterministic comparison transition sequence.
    #[test]
    fn comparison_sequence_is_object_reports_describe_compare() {
        let classification = classification(Intent::CompareReports);

        let first = DeterministicPlanner::decide(
            &classification,
            &context(false, false),
            "compare",
            "request",
            &[],
        )
        .unwrap();
        assert!(matches!(
            first,
            OrchestratorDecision::ExecuteWorker(ref request)
                if request.worker_type == WorkerType::ObjectIdFinder
        ));

        let second = DeterministicPlanner::decide(
            &classification,
            &context(true, false),
            "compare",
            "request",
            &[],
        )
        .unwrap();
        assert!(matches!(
            second,
            OrchestratorDecision::ExecuteWorker(ref request)
                if request.worker_type == WorkerType::DocumentsIdFinder
        ));

        let third = DeterministicPlanner::decide(
            &classification,
            &context(true, true),
            "compare",
            "request",
            &[],
        )
        .unwrap();
        assert!(matches!(
            third,
            OrchestratorDecision::ExecuteWorker(ref request)
                if request.worker_type == WorkerType::DescribeReport
        ));

        let fourth = DeterministicPlanner::decide(
            &classification,
            &context(true, true),
            "compare",
            "request",
            &[description_result()],
        )
        .unwrap();
        assert!(matches!(
            fourth,
            OrchestratorDecision::ExecuteWorker(ref request)
                if request.worker_type == WorkerType::CompareReports
        ));
    }

    /// Verifies that direct tree actions need no object context.
    #[test]
    fn tree_request_routes_directly() {
        let decision = DeterministicPlanner::decide(
            &classification(Intent::GetObjectTree),
            &context(false, false),
            "tree",
            "request",
            &[],
        )
        .unwrap();

        assert!(matches!(
            decision,
            OrchestratorDecision::ExecuteWorker(ref request)
                if request.worker_type == WorkerType::GetObjectTree
        ));
    }

    /// Verifies that a lone previous report is described only once.
    #[test]
    fn lone_previous_report_is_not_duplicated() {
        let mut user_context = context(true, false);
        user_context.previous_report_id = Some("only-report".to_string());

        let decision = DeterministicPlanner::decide(
            &classification(Intent::DescribeReport),
            &user_context,
            "describe",
            "request",
            &[],
        )
        .unwrap();

        assert!(matches!(
            decision,
            OrchestratorDecision::ExecuteWorker(ref request)
                if matches!(
                    &request.parameters,
                    WorkerParameters::DescribeReport { reports }
                        if reports.prev == "only-report" && reports.next.is_none()
                )
        ));
    }
}
