// tests/intent_router_comparison_tests.rs

mod common;

use common::TestContext;
use cx58_agent::agents::types::*;

#[tokio::test]
async fn test_compare_reports_missing_context() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    let result = ctx.intent_router
        .classify("Compare the reports", &user_context, &[])
        .await
        .expect("Classification failed");
    println!("Classification result: {:#?}", result);
    assert!(matches!(result.intent, Intent::CompareReports));
    assert!(result.missing_context.contains(&ContextField::ObjectId));
}

#[tokio::test]
async fn test_compare_last_two_reports() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context(
        "user-123",
        Language::English,
        Some("obj-123".to_string()),
        None,
        None,
    );

    let result = ctx.intent_router
        .classify("Compare the last two reports", &user_context, &[])
        .await
        .expect("Classification failed");
    tracing::info!("Classification result: {:#?}", result);
    assert!(matches!(result.intent, Intent::CompareReports));
    assert!(result.extracted_parameters.time_reference.iter().any(|n:&String| n.contains("last two")));
    let params = result.extracted_parameters.task_params
        .expect("Task params should be present");
    assert_eq!(params.amount, Some(2));
}

#[tokio::test]
async fn test_compare_with_full_context() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context(
        "user-123",
        Language::English,
        Some("obj-123".to_string()),
        Some("report-new".to_string()),
        Some("report-old".to_string()),
    );

    let result = ctx.intent_router
        .classify("Compare these reports", &user_context, &[])
        .await
        .expect("Classification failed");
    println!("Classification result: {:#?}", result);
    assert!(matches!(result.intent, Intent::CompareReports));
    assert!(result.missing_context.is_empty());
}

#[tokio::test]
async fn test_compare_time_references() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context(
        "user-123",
        Language::English,
        Some("obj-123".to_string()),
        None,
        None,
    );

    let result = ctx.intent_router
        .classify("Compare last week vs this week", &user_context, &[])
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::CompareReports));
    assert!(result.extracted_parameters.time_reference.is_some());
    assert!(result.extracted_parameters.report_references.len() >= 2);
}

#[tokio::test]
async fn test_show_differences() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context(
        "user-123",
        Language::English,
        Some("obj-123".to_string()),
        Some("report-new".to_string()),
        Some("report-old".to_string()),
    );

    let result = ctx.intent_router
        .classify("What are the differences between the reports?", &user_context, &[])
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::CompareReports));
}

#[tokio::test]
async fn test_progress_comparison() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context(
        "user-123",
        Language::English,
        Some("obj-123".to_string()),
        None,
        None,
    );

    let result = ctx.intent_router
        .classify("Show me the progress from January to February", &user_context, &[])
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::CompareReports));
    assert!(result.extracted_parameters.time_reference.is_some());
}
