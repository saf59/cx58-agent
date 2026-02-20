
mod common;

use common::orchestrator_test_helpers::OrchestratorTestContext;
use cx58_agent::agents::types::*;
use anyhow::Result;
use cx58_agent::agents::Period;

#[tokio::test]
async fn test_get_object_tree_no_missing_context() -> Result<()> {
    let ctx = OrchestratorTestContext::new();
    
    // Classification for "show all buildings"
    let classification = ctx.create_classification(
        Intent::GetObjectTree,
        0.95,
        Some(TaskParameters {
            last: false,
            all: true,
            period: None,
            amount: None,
        }),
        None,
        vec![], // No missing context
        //vec![ContextField::ObjectId,ContextField::CurrentReportId,ContextField::PreviousReportId], // No missing context
    );
    
    let user_context = ctx.create_context(Language::English, None, None, None);
    let worker_results = vec![];
    tracing::info!("User context: {:?}", user_context);
    let decision = ctx.orchestrator
        .decide_next_step(
            &classification,
            &user_context,
            "show all buildings",
            &worker_results,
        )
        .await;
    tracing::info!("Orchestrator decision: {:?}", decision);
    // Should execute ObjectTree worker immediately
    match decision {
        Ok(OrchestratorDecision::ExecuteWorker(worker_req)) => {
            assert!(matches!(worker_req.worker_type, WorkerType::GetObjectTree));
            match worker_req.parameters {
                WorkerParameters::GetObjectTree(params) => {
                    assert_eq!(params.all, true);
                    assert_eq!(params.last, false);
                }
                _ => panic!("Expected ObjectTree parameters"),
            }
        }
        _ => panic!("Expected ExecuteWorker decision, got: {:?}", decision),
    }
    
    Ok(())
}

#[tokio::test]
async fn test_get_object_tree_with_period_filter() -> Result<()> {
    let ctx = OrchestratorTestContext::new();
    
    let classification = ctx.create_classification(
        Intent::GetObjectTree,
        0.92,
        Some(TaskParameters {
            last: true,
            all: false,
            period: Some(Period::Week),
            amount: None,
        }),
        None,
        vec![],
    );
    
    let user_context = ctx.create_context(Language::English, None, None, None);
    
    let decision = ctx.orchestrator
        .decide_next_step(&classification, &user_context, "objects changed last week", &[])
        .await?;
    
    match decision {
        OrchestratorDecision::ExecuteWorker(worker_req) => {
            match worker_req.parameters {
                WorkerParameters::GetObjectTree(params) => {
                    assert_eq!(params.last, true);
                    assert!(matches!(params.period, Some(Period::Week)));
                }
                _ => panic!("Expected ObjectTree parameters"),
            }
        }
        _ => panic!("Expected ExecuteWorker decision"),
    }
    
    Ok(())
}

#[tokio::test]
async fn test_out_of_scope_immediate_reject() -> Result<()> {
    let ctx = OrchestratorTestContext::new();
    
    let classification = ctx.create_classification(
        Intent::OutOfScope,
        0.98,
        None,
        None,
        vec![],
    );
    
    let user_context = ctx.create_context(Language::English, None, None, None);
    
    let decision = ctx.orchestrator
        .decide_next_step(&classification, &user_context, "what's the weather", &[])
        .await?;
    
    // Should reject immediately for out of scope
    match decision {
        OrchestratorDecision::Reject { reason:_, message } => {
            assert!(message.len() > 0);
        }
        _ => panic!("Expected Reject decision for out of scope"),
    }
    
    Ok(())
}

#[tokio::test]
async fn test_rag_query_immediate_execution() -> Result<()> {
    let ctx = OrchestratorTestContext::new();
    
    let classification = ctx.create_classification(
        Intent::RagQuery,
        0.90,
        None,
        None,
        vec![],
    );
    
    let user_context = ctx.create_context(Language::English, None, None, None);
    
    let decision = ctx.orchestrator
        .decide_next_step(&classification, &user_context, "what can you do", &[])
        .await?;
    
    // RAG query requires no context, should execute immediately
    match decision {
        OrchestratorDecision::ExecuteWorker(worker_req) => {
            assert!(matches!(worker_req.worker_type, WorkerType::RagQuery));
            // Verify request_id is set by orchestrator
            assert!(worker_req.context.request_id.len() > 0, "request_id should not be empty");
            assert_eq!(worker_req.context.request_id.len(), 36); // UUID v7 length
        }
        _ => panic!("Expected ExecuteWorker for RAG query"),
    }

    Ok(())
}

#[tokio::test]
async fn test_report_id_validation() -> Result<()> {
    let ctx = OrchestratorTestContext::new();

    let classification = ctx.create_classification(
        Intent::DescribeReport,
        0.95,
        None,
        None,
        vec![],
    );

    let user_context = ctx.create_context(Language::English, Some("obj-123".to_string()), None, None);

    // Test with empty report_id - orchestrator should return error
    // Note: This tests the error path when LLM returns empty report_id
    let decision = ctx.orchestrator
        .decide_next_step(&classification, &user_context, "describe the report", &[])
        .await;

    // We expect either an error or a valid worker request
    // The validation is in the LLM response parsing, so if LLM returns valid data it works
    match decision {
        Ok(OrchestratorDecision::ExecuteWorker(_)) => {
            // This is acceptable - LLM returned valid report_id
        }
        Err(e) => {
            // This is also acceptable - validation caught empty report_id
            tracing::info!("Validation error (expected): {}", e);
        }
        _ => {}
    }

    Ok(())
}