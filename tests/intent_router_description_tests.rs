// tests/intent_router_description_tests.rs

mod common;

use common::TestContext;
use cx58_agent::agents::types::*;

#[tokio::test]
async fn test_describe_report_missing_all_context() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    let result = ctx.intent_router
        .classify("Describe the photo report", &user_context, &[])
        .await
        .expect("Classification failed");
    tracing::info!("Classification result: {:#?}", result);
    assert!(matches!(result.intent, Intent::DescribeReport));
    //assert!(result.missing_context.contains(&ContextField::ObjectId));
    assert!(result.missing_context.contains(&ContextField::CurrentReportId));
}

#[tokio::test]
async fn test_describe_report_with_full_context() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context(
        "user-123",
        Language::English,
        Some("obj-123".to_string()),
        Some("report-456".to_string()),
        None,
    );
    println!("User context:{:#?}",&user_context);
    let result = ctx.intent_router
        .classify("Describe this report", &user_context, &[])
        .await
        .expect("Classification failed");
    println!("Classification result: {:#?}", result);
    assert!(matches!(result.intent, Intent::DescribeReport));
    assert!(result.missing_context.is_empty());
}

#[tokio::test]
async fn test_describe_latest_report() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context(
        "user-123",
        Language::English,
        Some("obj-123".to_string()),
        None,
        None,
    );

    let result = ctx.intent_router
        .classify("What's in the latest report?", &user_context, &[])
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::DescribeReport));
    assert_eq!(
        result.extracted_parameters.report_references,
        vec!["latest"]
    );
}

#[tokio::test]
async fn test_analyze_photo() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context(
        "user-123",
        Language::English,
        Some("obj-123".to_string()),
        Some("report-456".to_string()),
        None,
    );

    let result = ctx.intent_router
        .classify("Analyze these photos", &user_context, &[])
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::DescribeReport));
}

#[tokio::test]
async fn test_whats_in_report() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context(
        "user-123",
        Language::English,
        Some("obj-123".to_string()),
        Some("report-456".to_string()),
        None,
    );

    let result = ctx.intent_router
        .classify("What's shown in the report?", &user_context, &[])
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::DescribeReport));
}