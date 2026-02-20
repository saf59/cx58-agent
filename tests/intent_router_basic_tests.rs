// tests/intent_router_basic_tests.rs

mod common;
use common::TestContext;
use cx58_agent::agents::Period;
use cx58_agent::agents::types::*;

#[tokio::test]
async fn test_get_object_tree_all_objects() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    let result = ctx.intent_router
        .classify("Show me all buildings", &user_context, &[])
        .await
        .expect("Classification failed");
    tracing::info!("Classification result: {:#?}", result);
    assert!(matches!(result.intent, Intent::GetObjectTree));
    assert!(result.confidence > 0.7);

    // Check task parameters
    let params = result.extracted_parameters.task_params
        .expect("Task params should be present");
    tracing::info!("Extracted task parameters: {:#?}", params);
    assert_eq!(params.all, true);
    assert_eq!(params.last, false);
    assert!(result.missing_context.is_empty());
}

#[tokio::test]
async fn test_get_object_tree_with_period() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    let result = ctx.intent_router
        .classify("Show me objects that changed last week", &user_context, &[])
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::GetObjectTree));

    let params = result.extracted_parameters.task_params
        .expect("Task params should be present");
    assert_eq!(params.last, true);
    assert_eq!(params.all, false);
    assert!(matches!(params.period, Some(Period::Week)));
}

#[tokio::test]
async fn test_get_object_tree_with_amount() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    let result = ctx.intent_router
        .classify("Show me the last 5 objects with changes", &user_context, &[])
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::GetObjectTree));

    let params = result.extracted_parameters.task_params
        .expect("Task params should be present");
    assert_eq!(params.last, true);
    assert_eq!(params.amount, Some(5));
}

#[tokio::test]
async fn test_get_object_tree_month_period() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    let result = ctx.intent_router
        .classify("Show me construction sites updated this month", &user_context, &[])
        .await
        .expect("Classification failed");
    println!("Classification result: {:#?}", result);
    // *** assertion failed: matches!(result.intent, Intent::GetObjectTree)
    // but the intent is actually GetReportList
    assert!(matches!(result.intent, Intent::GetObjectTree));
    let params = result.extracted_parameters.task_params
        .expect("Task params should be present");
    assert!(matches!(params.period, Some(Period::Month)));
}