
mod common;

use common::orchestrator_test_helpers::OrchestratorTestContext;
use cx58_agent::agents::types::*;
use anyhow::Result;

#[tokio::test]
async fn test_report_list_missing_object_id() -> Result<()> {
    let ctx = OrchestratorTestContext::new();
    
    let classification = ctx.create_classification(
        Intent::GetReportList,
        0.88,
        Some(TaskParameters {
            last: false,
            all: true,
            period: None,
            amount: None,
        }),
        None,
        vec![ContextField::ObjectId],
    );
    
    // No object_id in context
    let user_context = ctx.create_context(Language::English, None, None, None);
    
    let decision = ctx.orchestrator
        .decide_next_step(&classification, &user_context, "show me reports", &[])
        .await?;
    
    // Should request object_id from user
    match decision {
        OrchestratorDecision::RequestContextFromUser { missing_field, prompt, suggestions:_ } => {
            assert!(matches!(missing_field, ContextField::ObjectId));
            assert!(prompt.len() > 0);
        }
        _ => panic!("Expected RequestContextFromUser for missing object_id"),
    }

    Ok(())
}

#[tokio::test]
async fn test_request_context_with_suggestions() -> Result<()> {
    let ctx = OrchestratorTestContext::new();

    // Classification with object_identifier extracted from message
    let classification = ctx.create_classification(
        Intent::GetReportList,
        0.88,
        None,
        Some("Building A".to_string()),  // Object identifier extracted
        vec![ContextField::ObjectId],
    );

    let user_context = ctx.create_context(Language::English, None, None, None);

    let decision = ctx.orchestrator
        .decide_next_step(&classification, &user_context, "show reports for Building A", &[])
        .await?;

    tracing::info!("Decision for missing object_id with suggestions: {:?}", decision);

    match decision {
        OrchestratorDecision::RequestContextFromUser { prompt, suggestions:_, .. } => {
            assert!(prompt.len() > 0);
            // Suggestions should be empty since we're not in Ambiguous case
            // (suggestions are only set for Ambiguous intent)
        }
        _ => panic!("Expected RequestContextFromUser"),
    }

    Ok(())
}

#[tokio::test]
async fn test_report_list_with_object_identifier() -> Result<()> {
    let ctx = OrchestratorTestContext::new();
    
    // Classification extracted "Building A" from user message
    let classification = ctx.create_classification(
        Intent::GetReportList,
        0.91,
        Some(TaskParameters {
            last: false,
            all: true,
            period: None,
            amount: None,
        }),
        Some("Building A".to_string()),
        vec![ContextField::ObjectId], // Still missing in context
    );
    
    let user_context = ctx.create_context(Language::English, None, None, None);
    
    let decision = ctx.orchestrator
        .decide_next_step(&classification, &user_context, "reports for Building A", &[])
        .await?;
    
    // Should try to infer object_id from "Building A"
    // In real implementation, this might trigger a search worker
    // For now, should still request clarification
    match decision {
        OrchestratorDecision::RequestContextFromUser { missing_field, prompt:_, .. } => {
            assert!(matches!(missing_field, ContextField::ObjectId));
        }
        OrchestratorDecision::ExecuteWorker(_) => {
            // Also acceptable if orchestrator is smart enough to search
        }
        _ => panic!("Expected context request or worker execution"),
    }
    
    Ok(())
}

#[tokio::test]
async fn test_describe_report_missing_both_ids() -> Result<()> {
    let ctx = OrchestratorTestContext::new();
    
    let classification = ctx.create_classification(
        Intent::DescribeReport,
        0.89,
        None,
        None,
        vec![ContextField::ObjectId, ContextField::CurrentReportId],
    );
    
    let user_context = ctx.create_context(Language::English, None, None, None);
    
    let decision = ctx.orchestrator
        .decide_next_step(&classification, &user_context, "describe the report", &[])
        .await?;
    tracing::info!("Decision for DescribeReport with missing object_id and report_id: {:?}", decision);
    // Should request object_id first (highest priority)
    match decision {
        OrchestratorDecision::RequestContextFromUser { missing_field, .. } => {
            assert!(matches!(missing_field, ContextField::ObjectId));
        }
        _ => panic!("Expected context request for object_id"),
    }
    
    Ok(())
}

#[tokio::test]
async fn test_compare_reports_missing_report_ids() -> Result<()> {
    let ctx = OrchestratorTestContext::new();
    
    let classification = ctx.create_classification(
        Intent::CompareReports,
        0.87,
        Some(TaskParameters {
            last: true,
            all: false,
            period: None,
            amount: Some(2),
        }),
        None,
        vec![ContextField::CurrentReportId, ContextField::PreviousReportId],
    );
    
    // Has object_id but no report IDs
    let user_context = ctx.create_context(
        Language::English,
        Some("obj-123".to_string()),
        None,
        None,
    );
    
    let decision = ctx.orchestrator
        .decide_next_step(&classification, &user_context, "compare last two reports", &[])
        .await?;
    
    // Should execute ReportList worker to fetch last 2 reports
    match decision {
        OrchestratorDecision::ExecuteWorker(worker_req) => {
            assert!(matches!(worker_req.worker_type, WorkerType::GetReportList));
            match worker_req.parameters {
                WorkerParameters::GetReportList { object_id, task_params } => {
                    assert_eq!(object_id, "obj-123");
                    assert_eq!(task_params.amount, Some(2));
                }
                _ => panic!("Expected ReportList parameters"),
            }
        }
        _ => panic!("Expected ExecuteWorker for ReportList"),
    }

    Ok(())
}

#[tokio::test]
async fn test_ambiguous_intent_requests_clarification() -> Result<()> {
    let ctx = OrchestratorTestContext::new();

    let classification = ctx.create_classification(
        Intent::Ambiguous,
        0.50,  // Low confidence indicates ambiguity
        None,
        Some("Building A".to_string()),  // Has some object identifier
        vec![],
    );

    let user_context = ctx.create_context(Language::English, None, None, None);

    let decision = ctx.orchestrator
        .decide_next_step(&classification, &user_context, "show me something", &[])
        .await?;
    tracing::info!("Decision for Ambiguous intent: {:?}", decision);
    // Should request clarification for ambiguous intent
    match decision {
        OrchestratorDecision::RequestContextFromUser { prompt, suggestions, .. } => {
            assert!(prompt.len() > 0, "Prompt should not be empty");
            // Should have suggestions from object_identifier
            assert!(suggestions.len() > 0, "Should have suggestions");
            assert!(suggestions.contains(&"Building A".to_string()), "Suggestions should contain Building A");
        }
        _ => panic!("Expected RequestContextFromUser for ambiguous intent"),
    }

    Ok(())
}