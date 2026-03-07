// tests/integration/get_report_list.rs

mod en {
    use super::super::common::*;

    #[tokio::test]
    async fn with_object_id() {
        let state = shared_state().await;
        let object_id = resolve_object_id(&state).await;
        let events = send_and_collect(&state, &build_request_with_object("Show all reports for this object", "en", &object_id)).await;
        assert_no_error(&events);
        assert_completed(&events);
        let _ = assert_event(&events, "report_list");
    }

    #[tokio::test]
    async fn auto_resolve_object() {
        let state = shared_state().await;
        let events = send_and_collect(&state, &build_request(&format!("Show all reports for \"{}\"", TEST_OBJECT_NAME), "en")).await;
        assert_no_error(&events);
        assert_completed(&events);
        let _ = assert_event(&events, "report_list");
    }

    #[tokio::test]
    async fn auto_resolve_with_period() {
        let state = shared_state().await;
        let events = send_and_collect(&state, &build_request(&format!("Show reports for \"{}\" from the last month", TEST_OBJECT_NAME), "en")).await;
        assert_no_error(&events);
        assert_completed(&events);
        let _ = assert_event(&events, "report_list");
    }

    #[tokio::test]
    async fn auto_resolve_last() {
        let state = shared_state().await;
        let events = send_and_collect(&state, &build_request(&format!("List all reports for \"{}\"", TEST_OBJECT_NAME), "en")).await;
        assert_no_error(&events);
        assert_completed(&events);
        let _ = assert_event(&events, "report_list");
    }
}

mod de {
    use super::super::common::*;

    #[tokio::test]
    async fn with_object_id() {
        let state = shared_state().await;
        let object_id = resolve_object_id(&state).await;
        let events = send_and_collect(&state, &build_request_with_object("Zeige alle Reports für dieses Objekt", "de", &object_id)).await;
        assert_no_error(&events);
        assert_completed(&events);
        let _ = assert_event(&events, "report_list");
    }

    #[tokio::test]
    async fn auto_resolve_object() {
        let state = shared_state().await;
        let events = send_and_collect(&state, &build_request(&format!("Zeige alle Reports für \"{}\"", TEST_OBJECT_NAME), "de")).await;
        assert_no_error(&events);
        assert_completed(&events);
        let _ = assert_event(&events, "report_list");
    }

    #[tokio::test]
    async fn auto_resolve_with_period() {
        let state = shared_state().await;
        let events = send_and_collect(&state, &build_request(&format!("Zeige Reports für \"{}\" vom letzten Monat", TEST_OBJECT_NAME), "de")).await;
        assert_no_error(&events);
        assert_completed(&events);
        let _ = assert_event(&events, "report_list");
    }

    #[tokio::test]
    async fn auto_resolve_last() {
        let state = shared_state().await;
        let events = send_and_collect(&state, &build_request(&format!("Zeige alle Reports für \"{}\"", TEST_OBJECT_NAME), "de")).await;
        assert_no_error(&events);
        assert_completed(&events);
        let _ = assert_event(&events, "report_list");
    }
}
