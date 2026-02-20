// tests/intent_router_german_tests.rs

mod common;

use common::TestContext;
use cx58_agent::agents::Period;
use cx58_agent::agents::types::*;

#[tokio::test]
async fn test_german_get_object_tree() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::German, None, None, None);

    let result = ctx.intent_router
        .classify("Zeige mir alle Gebäude", &user_context, &[])
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::GetObjectTree));
}

#[tokio::test]
async fn test_german_get_reports() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context(
        "user-123",
        Language::German,
        Some("obj-123".to_string()),
        None,
        None,
    );

    let result = ctx.intent_router
        .classify("Zeige mir die Fotoberichte", &user_context, &[])
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::GetReportList));
}

#[tokio::test]
async fn test_german_describe_report() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context(
        "user-123",
        Language::German,
        Some("obj-123".to_string()),
        Some("report-456".to_string()),
        None,
    );

    let result = ctx.intent_router
        .classify("Beschreibe diesen Bericht", &user_context, &[])
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::DescribeReport));
}

#[tokio::test]
async fn test_german_compare_two_reports() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context(
        "user-123",
        Language::German,
        Some("obj-123".to_string()),
        None,
        None,
    );

    let result = ctx.intent_router
        .classify("Vergleiche die letzten zwei Berichte", &user_context, &[])
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::CompareReports));
}

#[tokio::test]
async fn test_german_last_week() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::German, None, None, None);

    let result = ctx.intent_router
        .classify("Zeige Objekte die sich letzte Woche geändert haben", &user_context, &[])
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::GetObjectTree));

    let params = result.extracted_parameters.task_params
        .expect("Task params should be present");
    assert!(matches!(params.period, Some(Period::Week)));
}

#[tokio::test]
async fn test_german_out_of_scope() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::German, None, None, None);

    let result = ctx.intent_router
        .classify("Wie ist das Wetter?", &user_context, &[])
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::OutOfScope));
}