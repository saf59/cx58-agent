// tests/integration/context_request.rs
use super::common::*;
use serde_json::Value;

fn assert_context_request(events: &[SseEvent]) {
    let data = assert_event(events, "context_request");
    let parsed: Value = serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("context_request is not JSON: {}\ndata: {}", e, data));
    assert!(
        parsed.get("prompt").is_some(),
        "context_request must contain 'prompt', got: {}", data
    );
}

mod en {
    use super::super::common::*;
    use super::assert_context_request;

    #[tokio::test]
    async fn missing_object() {
        let state = shared_state().await;
        let events = send_and_collect(&state, &build_request("Show me the reports", "en")).await;
        assert_completed(&events);
        assert_context_request(&events);
    }

    #[tokio::test]
    async fn missing_report_id_describe() {
        let state = shared_state().await;
        let object_id = resolve_object_id(&state).await;
        let events = send_and_collect(&state, &build_request_with_object("Describe the report", "en", &object_id)).await;
        assert_completed(&events);
        assert_context_request(&events);
    }

    #[tokio::test]
    async fn missing_report_ids_compare() {
        let state = shared_state().await;
        let object_id = resolve_object_id(&state).await;
        let events = send_and_collect(&state, &build_request_with_object("Compare the reports", "en", &object_id)).await;
        assert_completed(&events);
        assert_context_request(&events);
    }
}

mod de {
    use super::super::common::*;
    use super::assert_context_request;

    #[tokio::test]
    async fn missing_object() {
        let state = shared_state().await;
        let events = send_and_collect(&state, &build_request("Zeige mir die Reports", "de")).await;
        assert_completed(&events);
        assert_context_request(&events);
    }

    #[tokio::test]
    async fn missing_report_id_describe() {
        let state = shared_state().await;
        let object_id = resolve_object_id(&state).await;
        let events = send_and_collect(&state, &build_request_with_object("Beschreibe den Report", "de", &object_id)).await;
        assert_completed(&events);
        assert_context_request(&events);
    }

    #[tokio::test]
    async fn missing_report_ids_compare() {
        let state = shared_state().await;
        let object_id = resolve_object_id(&state).await;
        let events = send_and_collect(&state, &build_request_with_object("Vergleiche die Reports", "de", &object_id)).await;
        assert_completed(&events);
        assert_context_request(&events);
    }
}
