
mod common;

use common::formatter_test_helpers::FormatterTestContext;
use cx58_agent::agents::types::*;
use anyhow::Result;

#[tokio::test]
async fn test_format_comparison_english() -> Result<()> {
    let ctx = FormatterTestContext::new();
    
    let (desc1, desc2) = ctx.create_comparison_data();
    
    let comparison = ctx.formatter
        .format_comparison(
            &desc1,
            &desc2,
            &Language::English,
            "rpt-400",
            "rpt-456",
        )
        .await?;
    
    // Should return valid JSON
    assert!(comparison.is_object());
    
    // Should have required fields
    assert!(comparison["summary"].is_string());
    assert!(comparison["differences"].is_array());
    assert!(comparison["overall_assessment"].is_string());
    
    Ok(())
}

#[tokio::test]
async fn test_comparison_has_categorized_differences() -> Result<()> {
    let ctx = FormatterTestContext::new();
    
    let (desc1, desc2) = ctx.create_comparison_data();
    
    let comparison = ctx.formatter
        .format_comparison(&desc1, &desc2, &Language::English, "rpt-400", "rpt-456")
        .await?;
    
    let differences = comparison["differences"].as_array()
        .expect("differences should be array");
    
    assert!(differences.len() > 0, "Should have at least one difference");
    
    // Check first difference has required structure
    let first_diff = &differences[0];
    assert!(first_diff["category"].is_string());
    assert!(first_diff["description"].is_string());
    assert!(first_diff["severity"].is_string());
    
    // Category should be one of valid types
    let category = first_diff["category"].as_str().unwrap();
    let valid_categories = ["Structural", "Materials", "Progress", "Equipment", "Environmental"];
    assert!(
        valid_categories.contains(&category),
        "Invalid category: {}",
        category
    );
    
    Ok(())
}

#[tokio::test]
async fn test_comparison_severity_levels() -> Result<()> {
    let ctx = FormatterTestContext::new();
    
    let (desc1, desc2) = ctx.create_comparison_data();
    
    let comparison = ctx.formatter
        .format_comparison(&desc1, &desc2, &Language::English, "rpt-400", "rpt-456")
        .await?;
    
    let differences = comparison["differences"].as_array().unwrap();
    
    for diff in differences {
        let severity = diff["severity"].as_str().unwrap();
        assert!(
            ["Major", "Minor", "Cosmetic"].contains(&severity),
            "Invalid severity: {}",
            severity
        );
    }
    
    Ok(())
}

#[tokio::test]
async fn test_comparison_similarity_score() -> Result<()> {
    let ctx = FormatterTestContext::new();
    
    let (desc1, desc2) = ctx.create_comparison_data();
    
    let comparison = ctx.formatter
        .format_comparison(&desc1, &desc2, &Language::English, "rpt-400", "rpt-456")
        .await?;
    
    let similarity = comparison["similarity_score"].as_f64()
        .expect("similarity_score should be a number");
    
    // Score should be between 0 and 1
    assert!(similarity >= 0.0 && similarity <= 1.0, "Similarity score out of range: {}", similarity);
    
    Ok(())
}

#[tokio::test]
async fn test_comparison_quantifies_changes() -> Result<()> {
    let ctx = FormatterTestContext::new();
    
    let desc1 = "Foundation at 65% completion".to_string();
    let desc2 = "Foundation at 80% completion".to_string();
    
    let comparison = ctx.formatter
        .format_comparison(&desc1, &desc2, &Language::English, "rpt-1", "rpt-2")
        .await?;
    
    let summary = comparison["summary"].as_str().unwrap();
    
    // Should mention percentage change or numbers
    assert!(
        summary.contains("65") || summary.contains("80") || summary.contains("%") || summary.contains("15"),
        "Should quantify the change"
    );
    
    Ok(())
}

#[tokio::test]
async fn test_comparison_german() -> Result<()> {
    let ctx = FormatterTestContext::new();
    
    let (desc1, desc2) = ctx.create_comparison_data();
    
    let comparison = ctx.formatter
        .format_comparison(&desc1, &desc2, &Language::German, "rpt-400", "rpt-456")
        .await?;
    
    let summary = comparison["summary"].as_str().unwrap();
    
    // Should contain German words
    assert!(
        summary.contains("Fortschritt") || 
        summary.contains("Änderungen") ||
        summary.contains("Unterschied"),
        "Summary should be in German"
    );
    
    Ok(())
}