
mod common;

use common::formatter_test_helpers::FormatterTestContext;
use cx58_agent::agents::types::*;
use anyhow::Result;

#[tokio::test]
async fn test_format_description_english() -> Result<()> {
    let ctx = FormatterTestContext::new();
    
    let vision_data = ctx.create_vision_data();
    
    let description: /* Type */ = ctx.formatter
        .format_description(
            &vision_data,
            &Language::English,
            "rpt-456",
        )
        .await?;
    
    // Should contain construction terminology
    assert!(description.len() > 50, "Description too short");
    assert!(
        description.contains("foundation") || 
        description.contains("concrete") ||
        description.contains("progress"),
        "Missing construction terminology"
    );
    
    // Should be structured (check for multiple sentences)
    assert!(description.matches('.').count() >= 3, "Should have multiple sentences");
    
    Ok(())
}

#[tokio::test]
async fn test_format_description_german() -> Result<()> {
    let ctx = FormatterTestContext::new();
    
    let vision_data = ctx.create_vision_data();
    
    let description = ctx.formatter
        .format_description(
            &vision_data,
            &Language::German,
            "rpt-456",
        )
        .await?;
    
    // Should be in German
    assert!(description.len() > 50);
    assert!(
        description.contains("Fundament") || 
        description.contains("Beton") ||
        description.contains("Fortschritt") ||
        description.contains("Bauarbeiten"),
        "Missing German construction terminology"
    );
    
    Ok(())
}

#[tokio::test]
async fn test_description_contains_completion_percentage() -> Result<()> {
    let ctx = FormatterTestContext::new();
    
    let vision_data = serde_json::json!({
        "completion_estimate": "75%",
        "observations": ["Foundation at 75% completion"]
    });
    
    let description = ctx.formatter
        .format_description(&vision_data, &Language::English, "rpt-456")
        .await?;
    
    // Should mention completion percentage
    assert!(
        description.contains("75") || description.contains("percent") || description.contains("%"),
        "Should mention completion percentage"
    );
    
    Ok(())
}

#[tokio::test]
async fn test_description_professional_tone() -> Result<()> {
    let ctx = FormatterTestContext::new();
    
    let vision_data = ctx.create_vision_data();
    
    let description = ctx.formatter
        .format_description(&vision_data, &Language::English, "rpt-456")
        .await?;
    
    // Should use professional terminology, not casual language
    let casual_words = ["stuff", "thing", "kinda", "sorta", "basically"];
    for word in casual_words {
        assert!(
            !description.to_lowercase().contains(word),
            "Should not contain casual word: {}",
            word
        );
    }
    
    Ok(())
}