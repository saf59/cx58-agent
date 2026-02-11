mod common;

use common::formatter_test_helpers::FormatterTestContext;
use cx58_agent::agents::types::*;
use anyhow::Result;

#[tokio::test]
async fn test_description_with_empty_data() -> Result<()> {
    let ctx = FormatterTestContext::new();
    
    let empty_data = serde_json::json!({});
    
    let description = ctx.formatter
        .format_description(&empty_data, &Language::English, "rpt-123")
        .await?;
    
    // Should handle gracefully
    assert!(description.len() > 0, "Should return some description even with empty data");
    
    Ok(())
}

#[tokio::test]
async fn test_description_with_minimal_data() -> Result<()> {
    let ctx = FormatterTestContext::new();
    
    let minimal_data = serde_json::json!({
        "observations": ["Foundation visible"]
    });
    
    let description = ctx.formatter
        .format_description(&minimal_data, &Language::English, "rpt-123")
        .await?;
    
    assert!(description.contains("foundation") || description.contains("Foundation"));
    
    Ok(())
}

#[tokio::test]
async fn test_comparison_identical_reports() -> Result<()> {
    let ctx = FormatterTestContext::new();
    
    let same_desc = "Foundation at 75% completion".to_string();
    
    let comparison = ctx.formatter
        .format_comparison(&same_desc, &same_desc, &Language::English, "rpt-1", "rpt-1")
        .await?;
    
    let similarity = comparison["similarity_score"].as_f64().unwrap();
    
    // Should have high similarity for identical reports
    assert!(similarity > 0.85, "Identical reports should have high similarity");
    
    Ok(())
}

#[tokio::test]
async fn test_comparison_very_different_reports() -> Result<()> {
    let ctx = FormatterTestContext::new();
    
    let desc1 = "Excavation phase. Ground cleared. No structures.".to_string();
    let desc2 = "Finishing phase. Building complete. Interior work ongoing.".to_string();
    
    let comparison = ctx.formatter
        .format_comparison(&desc1, &desc2, &Language::English, "rpt-1", "rpt-100")
        .await?;

    tracing::info!("Comparison result: {:?}", comparison);
    let similarity = comparison["similarity_score"].as_f64().unwrap();
    
    // Should have low similarity for very different reports
    assert!(similarity < 0.6, "Very different reports should have low similarity");
    
    let differences = comparison["differences"].as_array().unwrap();
    assert!(differences.len() > 0, "Should identify differences");
    
    Ok(())
}