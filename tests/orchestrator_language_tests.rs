
mod common;

use common::orchestrator_test_helpers::OrchestratorTestContext;
use cx58_agent::agents::types::*;
use anyhow::Result;

#[tokio::test]
async fn test_german_context_request() -> Result<()> {
    let ctx = OrchestratorTestContext::new();
    
    let classification = ctx.create_classification(
        Intent::GetReportList,
        0.89,
        None,
        None,
        vec![ContextField::ObjectId],
    );
    
    let user_context = ctx.create_context(Language::German, None, None, None);
    
    let decision = ctx.orchestrator
        .decide_next_step(&classification, &user_context, "Zeige mir Berichte", &[])
        .await?;
    
    match decision {
        OrchestratorDecision::RequestContextFromUser { prompt, .. } => {
            // Prompt should be in German
            assert!(
                prompt.contains("Gebäude") || prompt.contains("Baustelle"),
                "Expected German prompt, got: {}",
                prompt
            );
        }
        _ => panic!("Expected context request"),
    }
    
    Ok(())
}

#[tokio::test]
async fn test_english_reject_message() -> Result<()> {
    let ctx = OrchestratorTestContext::new();
    
    let classification = ctx.create_classification(
        Intent::OutOfScope,
        0.97,
        None,
        None,
        vec![],
    );
    
    let user_context = ctx.create_context(Language::English, None, None, None);
    
    let decision = ctx.orchestrator
        .decide_next_step(&classification, &user_context, "what's the weather", &[])
        .await?;
    
    match decision {
        OrchestratorDecision::Reject { message, .. } => {
            // Message should be in English
            assert!(
                message.contains("construction") || message.contains("monitoring"),
                "Expected English rejection message"
            );
        }
        _ => panic!("Expected rejection"),
    }
    
    Ok(())
}