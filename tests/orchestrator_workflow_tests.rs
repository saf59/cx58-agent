
mod common;

use common::orchestrator_test_helpers::OrchestratorTestContext;
use cx58_agent::agents::types::*;
use anyhow::Result;

#[tokio::test]
async fn test_compare_workflow_step1_fetch_reports() -> Result<()> {
    let ctx = OrchestratorTestContext::new();
    
    let classification = ctx.create_classification(
        Intent::CompareReports,
        0.90,
        Some(TaskParameters {
            last: true,
            all: false,
            period: None,
            amount: Some(2),
        }),
        None,
        vec![ContextField::CurrentReportId, ContextField::PreviousReportId],
    );
    
    let user_context = ctx.create_context(
        Language::English,
        Some("obj-123".to_string()),
        None,
        None,
    );
    
    // Step 1: No workers executed yet
    let decision = ctx.orchestrator
        .decide_next_step(&classification, &user_context, "compare last two", &[])
        .await?;
    
    // Should fetch report list
    match decision {
        OrchestratorDecision::ExecuteWorker(worker_req) => {
            assert!(matches!(worker_req.worker_type, WorkerType::GetReportList));
        }
        _ => panic!("Expected ReportList worker execution"),
    }
    
    Ok(())
}

#[tokio::test]
async fn test_compare_workflow_step2_analyze_reports() -> Result<()> {
    let ctx = OrchestratorTestContext::new();
    
    let classification = ctx.create_classification(
        Intent::CompareReports,
        0.90,
        None,
        None,
        vec![],
    );
    
    let user_context = ctx.create_context(
        Language::English,
        Some("obj-123".to_string()),
        None,
        None,
    );
    
    // Step 2: ReportList worker has returned report IDs
    let report_list_response = ctx.create_worker_response(
        WorkerType::GetReportList,
        WorkerStatus::Success,
        serde_json::json!({
            "reports": [
                {"report_id": "rpt-456", "date": "2024-01-30"},
                {"report_id": "rpt-400", "date": "2024-01-23"}
            ]
        }),
    );
    
    let decision = ctx.orchestrator
        .decide_next_step(
            &classification,
            &user_context,
            "compare last two",
            &[report_list_response],
        )
        .await?;
    
    // Should now execute VisionAnalysis for reports
    match decision {
        OrchestratorDecision::ExecuteWorker(worker_req) => {
            //tracing::info!("Worker request: {:?}", worker_req);
            assert!(matches!(worker_req.worker_type, WorkerType::CompareReports));
        }
        _ => panic!("Expected VisionAnalysis worker execution"),
    }
    
    Ok(())
}

#[tokio::test]
async fn test_describe_latest_workflow() -> Result<()> {
    let ctx = OrchestratorTestContext::new();
    
    let classification = ctx.create_classification(
        Intent::DescribeReport,
        0.92,
        Some(TaskParameters {
            last: true,
            all: false,
            period: None,
            amount: Some(1),
        }),
        None,
        vec![ContextField::CurrentReportId],
    );
    
    let user_context = ctx.create_context(
        Language::English,
        Some("obj-123".to_string()),
        None,
        None,
    );
    
    // Step 1: Should fetch latest report
    let decision = ctx.orchestrator
        .decide_next_step(&classification, &user_context, "describe latest report", &[])
        .await?;
    
    match decision {
        OrchestratorDecision::ExecuteWorker(worker_req) => {
            tracing::info!("Worker request: {:?}", worker_req);
            assert!(matches!(worker_req.worker_type, WorkerType::GetReportList));
            // Verify parameters are correctly set
            match worker_req.parameters {
                WorkerParameters::GetReportList { object_id, task_params } => {
                    assert_eq!(object_id, "obj-123");
                    assert!(task_params.last);
                    assert_eq!(task_params.amount, Some(1));
                }
                _ => panic!("Expected GetReportList parameters"),
            }
        }
        OrchestratorDecision::RequestContextFromUser { .. } => {
            // Also acceptable if the orchestrator needs to ask for report confirmation
            // This can happen if it cannot infer from task_params alone
        }
        _ => panic!("Expected ExecuteWorker or RequestContextFromUser"),
    }
    
    Ok(())
}

#[tokio::test]
async fn test_complete_workflow_format_and_return() -> Result<()> {
    let ctx = OrchestratorTestContext::new();
    
    let classification = ctx.create_classification(
        Intent::DescribeReport,
        0.95,
        None,
        None,
        vec![],
    );
    
    let user_context = ctx.create_context(
        Language::English,
        Some("obj-123".to_string()),
        Some("rpt-456".to_string()),
        None,
    );
    
    // All workers completed successfully
    let vision_response = ctx.create_worker_response(
        WorkerType::DescribeReport,
        WorkerStatus::Success,
        serde_json::json!({
            "description": "Foundation work at 80% completion..."
        }),
    );
    
    let decision = ctx.orchestrator
        .decide_next_step(
            &classification,
            &user_context,
            "describe this report",
            &[vision_response],
        )
        .await?;
    tracing::info!("Final decision: {:?}", decision);
    // Should be ready to format and return with worker results from orchestrator
    match decision {
        OrchestratorDecision::FormatAndReturn { worker_results } => {
            assert_eq!(worker_results.len(), 1, "worker_results should contain results from previous workers");
            assert_eq!(worker_results[0].worker_type, WorkerType::DescribeReport);
            assert_eq!(worker_results[0].status, WorkerStatus::Success);
        }
        _ => panic!("Expected FormatAndReturn decision"),
    }
    
    Ok(())
}