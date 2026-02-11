// tests/test_utils.rs

use cx58_agent::agents::types::{ClassificationResult, ContextField, Intent};

pub fn assert_intent_matches(result: &ClassificationResult, expected: Intent) {
    assert_eq!(std::mem::discriminant(&result.intent), std::mem::discriminant(&expected), "Expected intent {:?} but got {:?}", expected, result.intent);
}

pub fn assert_has_task_params(result: &ClassificationResult) {
    assert!(
        result.extracted_parameters.task_params.is_some(),
        "Expected task params to be present"
    );
}

pub fn assert_missing_context(result: &ClassificationResult, field: ContextField) {
    assert!(
        result.missing_context.contains(&field),
        "Expected missing context {:?}",
        field
    );
}

pub fn assert_no_missing_context(result: &ClassificationResult) {
    assert!(
        result.missing_context.is_empty(),
        "Expected no missing context but got {:?}",
        result.missing_context
    );
}

pub fn assert_confidence_threshold(result: &ClassificationResult, threshold: f32) {
    assert!(
        result.confidence >= threshold,
        "Confidence {} is below threshold {}",
        result.confidence,
        threshold
    );
}