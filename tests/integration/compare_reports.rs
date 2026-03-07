// tests/integration/compare_reports.rs
use super::common::*;
use serde_json::Value;

fn assert_comparison(events: &[SseEvent]) {
    let data = assert_event(events, "comparison");
    let parsed: Value = serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("comparison is not JSON: {}\ndata: {}", e, data));
    assert!(
        parsed.is_object() && parsed.get("object_name").is_some(),
        "comparison must contain 'object_name', got: {}", data
    );
}

mod en {
    use super::super::common::*;
    use super::assert_comparison;

    #[tokio::test]
    async fn full_context() {
        let state = shared_state().await;
        let object_id = resolve_object_id(&state).await;
        let (current_id, previous_id) = resolve_report_ids(&state, &object_id).await;
        let events = send_and_collect(&state, &build_request_with_reports("Compare the two reports", "en", &object_id, &current_id, &previous_id)).await;
        assert_no_error(&events);
        assert_completed(&events);
        assert_comparison(&events);
    }

    #[tokio::test]
    async fn auto_resolve_reports() {
        let state = shared_state().await;
        let object_id = resolve_object_id(&state).await;
        let events = send_and_collect(&state, &build_request_with_object("Compare reports for the last month", "en", &object_id)).await;
        assert_no_error(&events);
        assert_completed(&events);
        assert_comparison(&events);
    }

    #[tokio::test]
    async fn full_auto_resolve_period() {
        let state = shared_state().await;
        let events = send_and_collect(&state, &build_request(&format!("Compare the reports from last month for \"{}\"", TEST_OBJECT_NAME), "en")).await;
        assert_no_error(&events);
        assert_completed(&events);
        assert_comparison(&events);
    }

    #[tokio::test]
    async fn full_auto_resolve_last_month() {
        let state = shared_state().await;
        let events = send_and_collect(&state, &build_request(&format!("Compare the changes over the last month for \"{}\"", TEST_OBJECT_NAME), "en")).await;
        assert_no_error(&events);
        assert_completed(&events);
        assert_comparison(&events);
    }
}

mod de {
    use super::super::common::*;
    use super::assert_comparison;

    #[tokio::test]
    async fn full_context() {
        let state = shared_state().await;
        let object_id = resolve_object_id(&state).await;
        let (current_id, previous_id) = resolve_report_ids(&state, &object_id).await;
        let events = send_and_collect(&state, &build_request_with_reports("Vergleiche die zwei Reports", "de", &object_id, &current_id, &previous_id)).await;
        assert_no_error(&events);
        assert_completed(&events);
        assert_comparison(&events);
    }

    #[tokio::test]
    async fn auto_resolve_reports() {
        let state = shared_state().await;
        let object_id = resolve_object_id(&state).await;
        let events = send_and_collect(&state, &build_request_with_object("Vergleiche die Reports des letzten Monats", "de", &object_id)).await;
        assert_no_error(&events);
        assert_completed(&events);
        assert_comparison(&events);
    }

    #[tokio::test]
    async fn full_auto_resolve_period() {
        let state = shared_state().await;
        let events = send_and_collect(&state, &build_request(&format!("Vergleiche die Reports vom letzten Monat für \"{}\"", TEST_OBJECT_NAME), "de")).await;
        assert_no_error(&events);
        assert_completed(&events);
        assert_comparison(&events);
    }

    #[tokio::test]
    async fn full_auto_resolve_last_month() {
        let state = shared_state().await;
        let events = send_and_collect(&state, &build_request(&format!("Vergleiche die Änderungen des letzten Monats für \"{}\"", TEST_OBJECT_NAME), "de")).await;
        assert_no_error(&events);
        assert_completed(&events);
        assert_comparison(&events);
    }
}
