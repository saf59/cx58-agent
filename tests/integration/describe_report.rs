// tests/integration/describe_report.rs
use super::common::*;
use serde_json::Value;

fn assert_description(events: &[SseEvent]) {
    let data = assert_event(events, "description");
    let parsed: Value = serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("description is not JSON: {}\ndata: {}", e, data));
    assert!(
        parsed.is_array() && !parsed.as_array().unwrap().is_empty(),
        "description must be a non-empty array, got: {}", data
    );
}

mod en {
    use super::super::common::*;
    use super::assert_description;

    #[tokio::test]
    async fn full_context() {
        let state = shared_state().await;
        let object_id = resolve_object_id(&state).await;
        let (current_id, previous_id) = resolve_report_ids(&state, &object_id).await;
        let events = send_and_collect(&state, &build_request_with_reports("Describe the current report", "en", &object_id, &current_id, &previous_id)).await;
        assert_no_error(&events);
        assert_completed(&events);
        assert_description(&events);
    }

    #[tokio::test]
    async fn auto_resolve_reports() {
        let state = shared_state().await;
        let object_id = resolve_object_id(&state).await;
        let events = send_and_collect(&state, &build_request_with_object("Describe the latest report", "en", &object_id)).await;
        assert_no_error(&events);
        assert_completed(&events);
        assert_description(&events);
    }

    #[tokio::test]
    async fn full_auto_resolve_period() {
        let state = shared_state().await;
        let events = send_and_collect(&state, &build_request(&format!("Describe the report from last month for \"{}\"", TEST_OBJECT_NAME), "en")).await;
        //println!("Events: {:#?}",&events );
        assert_no_error(&events);
        assert_completed(&events);
        assert_description(&events);
    }

    #[tokio::test]
    async fn full_auto_resolve_last() {
        let state = shared_state().await;
        let events = send_and_collect(&state, &build_request(&format!("Show me the latest report for \"{}\"", TEST_OBJECT_NAME), "en")).await;
        assert_no_error(&events);
        assert_completed(&events);
        assert_description(&events);
    }
}

mod de {
    use super::super::common::*;
    use super::assert_description;

    #[tokio::test]
    async fn full_context() {
        let state = shared_state().await;
        let object_id = resolve_object_id(&state).await;
        let (current_id, previous_id) = resolve_report_ids(&state, &object_id).await;
        let events = send_and_collect(&state, &build_request_with_reports("Beschreibe den aktuellen Report", "de", &object_id, &current_id, &previous_id)).await;
        assert_no_error(&events);
        assert_completed(&events);
        assert_description(&events);
    }

    #[tokio::test]
    async fn auto_resolve_reports() {
        let state = shared_state().await;
        let object_id = resolve_object_id(&state).await;
        let events = send_and_collect(&state, &build_request_with_object("Beschreibe den neuesten Report", "de", &object_id)).await;
        assert_no_error(&events);
        assert_completed(&events);
        assert_description(&events);
    }

    #[tokio::test]
    async fn full_auto_resolve_period() {
        let state = shared_state().await;
        let events = send_and_collect(&state, &build_request(&format!("Beschreibe den Report vom letzten Monat für \"{}\"", TEST_OBJECT_NAME), "de")).await;
        assert_no_error(&events);
        assert_completed(&events);
        assert_description(&events);
    }

    #[tokio::test]
    async fn full_auto_resolve_last() {
        let state = shared_state().await;
        let events = send_and_collect(&state, &build_request(&format!("Zeige mir den neuesten Report für \"{}\"", TEST_OBJECT_NAME), "de")).await;
        assert_no_error(&events);
        assert_completed(&events);
        assert_description(&events);
    }
}
