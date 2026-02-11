
mod common;

use common::orchestrator_test_helpers::OrchestratorTestContext;
use common::formatter_test_helpers::FormatterTestContext;
use cx58_agent::agents::types::*;
use anyhow::Result;

#[tokio::test]
async fn test_full_describe_workflow() -> Result<()> {
    let orch_ctx = OrchestratorTestContext::new();
    let fmt_ctx = FormatterTestContext::new();
    
    // Step 1: Orchestrator decides to execute vision worker
    let classification = orch_ctx.create_classification(
        Intent::DescribeReport,
        0.95,
        None,
        None,
        vec![],
    );
    
    let user_context = orch_ctx.create_context(
        Language::English,
        Some("obj-123".to_string()),
        Some("rpt-456".to_string()),
        None,
    );
    
    let decision = orch_ctx.orchestrator
        .decide_next_step(&classification, &user_context, "describe this report", &[])
        .await?;
    
    // Should execute vision worker
    assert!(matches!(decision, OrchestratorDecision::ExecuteWorker(_)));
    
    // Step 2: Simulate vision worker response
    let vision_data = fmt_ctx.create_vision_data();
    
    // Step 3: Format the description
    let description = fmt_ctx.formatter
        .format_description(&vision_data, &Language::English, "rpt-456")
        .await?;
    
    assert!(description.len() > 50);
    assert!(description.matches('.').count() >= 3);
    
    Ok(())
}

#[tokio::test]
async fn test_full_comparison_workflow() -> Result<()> {
    //let orch_ctx = OrchestratorTestContext::new();
    let fmt_ctx = FormatterTestContext::new();
    /*
    // Step 1: Orchestrator with comparison intent
    let classification = orch_ctx.create_classification(
        Intent::CompareReports,
        0.90,
        Some(TaskParameters {
            last: true,
            all: false,
            period: None,
            amount: Some(2),
        }),
        None,
        vec![],
    );
    
    let user_context = orch_ctx.create_context(
        Language::English,
        Some("obj-123".to_string()),
        None,
        None,
    );
    */
    // Step 2: Get report IDs (simulated)
    let (desc1, desc2) = fmt_ctx.create_comparison_data();
    
    // Step 3: Format comparison
    let comparison = fmt_ctx.formatter
        .format_comparison(&desc1, &desc2, &Language::English, "rpt-1", "rpt-2")
        .await?;
    
    assert!(comparison.is_object());
    assert!(comparison["differences"].as_array().unwrap().len() > 0);
    
    Ok(())
}