#[path = "common/mod.rs"]
mod common;
use common::TestContext;
use cx58_agent::agents::types::*;

#[tokio::test]
async fn test_get_report_list_missing_context() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);
    tracing::info!("User_context: {:#?}",&user_context);
    let result:ClassificationResult  = ctx.intent_router
        .classify("Show me photo reports", &user_context, &Vec::<String>::new())
        .await
        .expect("Classification failed");
    tracing::info!("Classification result: {:#?}", result);
    assert!(matches!(result.intent, Intent::GetReportList));
    // *** object_identifier: Some("".to_string()), TODO remove empty string
    assert!(result.missing_context.contains(&ContextField::ObjectId));
}

#[tokio::test]
async fn test_get_report_list_with_object_context() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context(
        "user-123",
        Language::English,
        Some("obj-123".to_string()),
        None,
        None,
    );

    let result:ClassificationResult  = ctx.intent_router
        .classify("Show me all photo reports", &user_context, &Vec::<String>::new())
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::GetReportList));
    assert!(result.missing_context.is_empty());

    let params = result.extracted_parameters.task_params
        .expect("Task params should be present");
    assert_eq!(params.all, true);
}

#[tokio::test]
async fn test_get_report_list_with_object_identifier() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    let result:ClassificationResult  = ctx.intent_router
        .classify("Show me reports for Building A", &user_context, &Vec::<String>::new())
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::GetReportList));
    assert_eq!(
        result.extracted_parameters.object_identifier,
        Some("Building A".to_string())
    );
}

#[tokio::test]
async fn test_get_report_list_last_week() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context(
        "user-123",
        Language::English,
        Some("obj-123".to_string()),
        None,
        None,
    );

    let result:ClassificationResult  = ctx.intent_router
        .classify("Show me reports from last week", &user_context, &Vec::<String>::new())
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::GetReportList));

    let params = result.extracted_parameters.task_params
        .expect("Task params should be present");
    assert!(matches!(params.period, Some(Period::Week)));
}

#[tokio::test]
async fn test_get_report_list_latest() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context(
        "user-123",
        Language::English,
        Some("obj-123".to_string()),
        None,
        None,
    );

    let result:ClassificationResult  = ctx.intent_router
        .classify("Show me the latest 3 reports", &user_context, &Vec::<String>::new())
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::GetReportList));

    let params = result.extracted_parameters.task_params
        .expect("Task params should be present");
    assert_eq!(params.last, true);
    assert_eq!(params.amount, Some(3));
}