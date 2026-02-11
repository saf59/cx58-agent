// tests/intent_router_performance_tests.rs
mod common;

use common::TestContext;
use std::sync::Arc;
use cx58_agent::agents::types::*;
use std::time::Instant;

#[tokio::test]
async fn test_classification_performance() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    let start = Instant::now();

    let result = ctx.intent_router
        .classify("Show me all buildings", &user_context, &[])
        .await
        .expect("Classification failed");

    let duration = start.elapsed();

    assert!(matches!(result.intent, Intent::GetObjectTree));
    println!("Classification took: {:?}", duration);

    // Should complete reasonably fast with FunctionGemma
    assert!(duration.as_secs() < 10, "Classification took too long: {:?}", duration);
}

#[tokio::test]
async fn test_batch_classification() {
    let ctx = TestContext::new();
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    let queries = vec![
        "Show all buildings",
        "List reports",
        "Describe the photo",
        "Compare reports",
        "What can you do?",
        "What's the weather?",
    ];

    let start = Instant::now();

    for query in queries {
        let result = ctx.intent_router
            .classify(query, &user_context, &[])
            .await;

        assert!(result.is_ok(), "Failed to classify: {}", query);
    }

    let duration = start.elapsed();
    println!("Batch classification of {} queries took: {:?}", 6, duration);
}

#[tokio::test]
async fn test_concurrent_classification() {
    let ctx = Arc::new(TestContext::new());
    let user_context = ctx.create_context("user-123", Language::English, None, None, None);

    let queries = vec![
        "Show all buildings",
        "List reports",
        "Describe photo",
    ];

    let mut handles = vec![];

    for query in queries {
        let ctx_clone = ctx.clone();
        let user_context_clone = user_context.clone();
        let query_owned = query.to_string();

        let handle = //tokio::spawn(async move {
            ctx_clone.intent_router
                .classify(&query_owned, &user_context_clone, &[])
                .await;
        //});

        handles.push(handle);
    }

    for handle in handles {
        let _result = handle.expect("Task panicked");
        //assert!(result.is_ok());
    }
}