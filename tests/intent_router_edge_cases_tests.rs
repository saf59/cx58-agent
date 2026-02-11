// tests/intent_router_edge_cases_tests.rs

mod common;

use common::TestContext;
use cx58_agent::agents::types::*;

#[tokio::test]
async fn test_ambiguous_query() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    let result = ctx.intent_router
        .classify("Show me something", &user_context, &[])
        .await
        .expect("Classification failed");

    // Should be either Ambiguous or require context
    assert!(
        matches!(result.intent, Intent::Ambiguous) ||
            !result.missing_context.is_empty()
    );
}

#[tokio::test]
async fn test_complex_multi_intent() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context(
        "user-123",
        Language::English,
        Some("obj-123".to_string()),
        None,
        None,
    );

    let result = ctx.intent_router
        .classify(
            "Show me all reports from last month and compare the first and last one",
            &user_context,
            &[]
        )
        .await
        .expect("Classification failed");

    // Should identify primary intent (likely GetReportList or CompareReports)
    assert!(
        matches!(result.intent, Intent::GetReportList) ||
            matches!(result.intent, Intent::CompareReports)
    );
}

#[tokio::test]
async fn test_typos_and_variations() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    // Test with typo
    let result = ctx.intent_router
        .classify("Shwo me al biuldings", &user_context, &[])
        .await
        .expect("Classification failed");

    // Should still understand intent
    assert!(matches!(result.intent, Intent::GetObjectTree));
}

#[tokio::test]
async fn test_very_specific_date_range() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context(
        "user-123",
        Language::English,
        Some("obj-123".to_string()),
        None,
        None,
    );

    let result = ctx.intent_router
        .classify(
            "Show me reports between January 15th and January 30th",
            &user_context,
            &[]
        )
        .await
        .expect("Classification failed");
    println!("Result: {:?}", &result);
    assert!(matches!(result.intent, Intent::GetReportList));
    // Should extract time reference - TODO: implement detailed date range extraction
    // assert!(result.extracted_parameters.time_reference.is_some());
}

#[tokio::test]
async fn test_multiple_objects_mention() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    let result = ctx.intent_router
        .classify(
            "Compare Building A and Building B progress",
            &user_context,
            &[]
        )
        .await
        .expect("Classification failed");
    tracing::info!("Result: {:?}", &result);
    // Should extract multiple object identifiers
    assert!(result.extracted_parameters.object_identifier.is_some());
}

#[tokio::test]
async fn test_negative_query() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context(
        "user-123",
        Language::English,
        Some("obj-123".to_string()),
        None,
        None,
    );

    let result = ctx.intent_router
        .classify("Don't show me old reports", &user_context, &[])
        .await
        .expect("Classification failed");

    // Should understand as GetReportList with "last" preference
    assert!(matches!(result.intent, Intent::GetReportList));
}

#[tokio::test]
async fn test_question_vs_command() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    let result1 = ctx.intent_router
        .classify("Can you show me all buildings?", &user_context, &[])
        .await
        .expect("Classification failed");

    let result2 = ctx.intent_router
        .classify("Show me all buildings", &user_context, &[])
        .await
        .expect("Classification failed");

    // Both should result in same intent
    assert_eq!(
        std::mem::discriminant(&result1.intent),
        std::mem::discriminant(&result2.intent)
    );
}