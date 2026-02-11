// tests/intent_router_rag_tests.rs

mod common;

use common::TestContext;
use cx58_agent::agents::types::*;

#[tokio::test]
async fn test_rag_what_can_you_do() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    let result = ctx.intent_router
        .classify("What can you do?", &user_context, &[])
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::RagQuery));
    assert!(result.missing_context.is_empty());
}

#[tokio::test]
async fn test_rag_help() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    let result = ctx.intent_router
        .classify("Help me understand this system", &user_context, &[])
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::RagQuery));
}

#[tokio::test]
async fn test_rag_what_is_project() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    let result = ctx.intent_router
        .classify("What is this project about?", &user_context, &[])
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::RagQuery));
}

#[tokio::test]
async fn test_rag_how_does_it_work() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    let result = ctx.intent_router
        .classify("How does the monitoring work?", &user_context, &[])
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::RagQuery));
}

#[tokio::test]
async fn test_rag_explain_features() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    let result = ctx.intent_router
        .classify("Explain the photo report feature", &user_context, &[])
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::RagQuery));
}