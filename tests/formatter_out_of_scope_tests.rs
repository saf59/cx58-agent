
mod common;

use common::formatter_test_helpers::FormatterTestContext;
use cx58_agent::agents::types::*;
use anyhow::Result;

#[tokio::test]
async fn test_out_of_scope_english() -> Result<()> {
    let ctx = FormatterTestContext::new();
    
    let message = ctx.formatter
        .format_out_of_scope(&Language::English, "What's the weather?")
        .await?;
    
    // Should explain what the agent CAN do
    assert!(message.len() > 50);
    assert!(
        message.contains("construction") || message.contains("monitoring"),
        "Should mention construction monitoring"
    );
    
    // Should list capabilities
    assert!(
        message.contains("report") || message.contains("photo") || message.contains("progress"),
        "Should mention capabilities"
    );
    
    Ok(())
}

#[tokio::test]
async fn test_out_of_scope_german() -> Result<()> {
    let ctx = FormatterTestContext::new();
    
    let message = ctx.formatter
        .format_out_of_scope(&Language::German, "Wie ist das Wetter?")
        .await?;
    
    // Should be in German
    assert!(
        message.contains("Baustelle") || 
        message.contains("Überwachung") ||
        message.contains("helfen"),
        "Should be in German"
    );
    
    Ok(())
}

#[tokio::test]
async fn test_out_of_scope_not_apologetic() -> Result<()> {
    let ctx = FormatterTestContext::new();
    
    let message = ctx.formatter
        .format_out_of_scope(&Language::English, "Tell me a joke")
        .await?;
    
    // Should not be overly apologetic
    let apologetic_phrases = ["I'm sorry", "I apologize", "Unfortunately"];
    let apology_count = apologetic_phrases.iter()
        .filter(|phrase| message.contains(*phrase))
        .count();
    
    assert!(
        apology_count <= 1,
        "Message should not be overly apologetic"
    );
    
    Ok(())
}

#[tokio::test]
async fn test_out_of_scope_helpful_tone() -> Result<()> {
    let ctx = FormatterTestContext::new();
    
    let message = ctx.formatter
        .format_out_of_scope(&Language::English, "What's 2+2?")
        .await?;
    
    // Should ask how to help with construction projects
    assert!(
        message.contains("?") || message.contains("help"),
        "Should offer to help"
    );
    
    Ok(())
}