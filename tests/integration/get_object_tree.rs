// tests/integration/get_object_tree.rs

mod en {
    use super::super::common::*;

    #[tokio::test]
    async fn no_context() {
        let state = shared_state().await;
        let events = send_and_collect(&state, &build_request("Show me all objects", "en")).await;
        assert_no_error(&events);
        assert_completed(&events);
        let _ = assert_event(&events, "object_tree");
    }

    #[tokio::test]
    async fn with_object_id() {
        let state = shared_state().await;
        let object_id = resolve_object_id(&state).await;
        let events = send_and_collect(
            &state,
            &build_request_with_object("List the hierarchy", "en", &object_id),
        )
        .await;
        assert_no_error(&events);
        assert_completed(&events);
        let _ = assert_event(&events, "object_tree");
    }
}

mod de {
    use super::super::common::*;

    #[tokio::test]
    async fn no_context() {
        let state = shared_state().await;
        let events = send_and_collect(&state, &build_request("Zeige mir alle Objekte", "de")).await;
        assert_no_error(&events);
        assert_completed(&events);
        let _ = assert_event(&events, "object_tree");
    }

    #[tokio::test]
    async fn with_object_id() {
        let state = shared_state().await;
        let object_id = resolve_object_id(&state).await;
        let events = send_and_collect(
            &state,
            &build_request_with_object("Zeige die Hierarchie", "de", &object_id),
        )
        .await;
        assert_no_error(&events);
        assert_completed(&events);
        let _ = assert_event(&events, "object_tree");
    }
}
