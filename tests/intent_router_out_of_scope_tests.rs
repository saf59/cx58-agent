// tests/intent_router_out_of_scope_tests.rs

mod common;

use common::TestContext;
use cx58_agent::agents::types::*;

#[tokio::test]
async fn test_out_of_scope_weather() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    let result = ctx.intent_router
        .classify("What's the weather today?", &user_context, &[])
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::OutOfScope));
    assert!(result.confidence > 0.8);
}

#[tokio::test]
async fn test_out_of_scope_cooking() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    let result = ctx.intent_router
        .classify("How do I make pasta?", &user_context, &[])
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::OutOfScope));
}

#[tokio::test]
async fn test_out_of_scope_math() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    let result = ctx.intent_router
        .classify("What is 25 * 47?", &user_context, &[])
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::OutOfScope));
}

#[tokio::test]
async fn test_out_of_scope_news() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    let result = ctx.intent_router
        .classify("Tell me the latest news", &user_context, &[])
        .await
        .expect("Classification failed");
    tracing::info!("Classification result: {:?}", result);
    assert!(matches!(result.intent, Intent::OutOfScope));
}

#[tokio::test]
async fn test_out_of_scope_general_chat() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    let result = ctx.intent_router
        .classify("How are you doing today?", &user_context, &[])
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::OutOfScope));
}

#[tokio::test]
async fn test_out_of_scope_sports() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    let result = ctx.intent_router
        .classify("Who won the game last night?", &user_context, &[])
        .await
        .expect("Classification failed");

    assert!(matches!(result.intent, Intent::OutOfScope));
}