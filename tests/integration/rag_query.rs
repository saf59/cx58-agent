// tests/integration/rag_query.rs
use super::common::*;

fn assert_text_chunks(events: &[SseEvent]) {
    // TextChunk events have type "text_chunk" — just check the type field
    let count = events
        .iter()
        .filter(|e| {
            serde_json::from_str::<serde_json::Value>(&e.data)
                .ok()
                .and_then(|v| v["type"].as_str().map(|s| s.to_string()))
                .as_deref()
                == Some("text_chunk")
        })
        .count();
    assert!(
        count > 0,
        "Expected at least one TextChunk event.\nAll event types: {:?}",
        events
            .iter()
            .filter_map(|e| serde_json::from_str::<serde_json::Value>(&e.data).ok())
            .map(|v| v["type"].as_str().unwrap_or("?").to_string())
            .collect::<Vec<_>>()
    );
}

mod en {
    use super::super::common::*;
    use super::assert_text_chunks;

    #[tokio::test]
    async fn capabilities() {
        let state = shared_state().await;
        let events = send_and_collect(&state, &build_request("What can you do?", "en")).await;
        assert_no_error(&events);
        assert_completed(&events);
        assert_text_chunks(&events);
    }

    #[tokio::test]
    async fn project_knowledge() {
        let state = shared_state().await;
        let events = send_and_collect(
            &state,
            &build_request(
                "How does the monitoring system track construction progress?",
                "en",
            ),
        )
        .await;
        assert_no_error(&events);
        assert_completed(&events);
        assert_text_chunks(&events);
    }
}

mod de {
    use super::super::common::*;
    use super::assert_text_chunks;

    #[tokio::test]
    async fn capabilities() {
        let state = shared_state().await;
        let events = send_and_collect(&state, &build_request("Was kannst du tun?", "de")).await;
        assert_no_error(&events);
        assert_completed(&events);
        assert_text_chunks(&events);
    }

    #[tokio::test]
    async fn project_knowledge() {
        let state = shared_state().await;
        let events = send_and_collect(
            &state,
            &build_request(
                "Wie verfolgt das Überwachungssystem den Baufortschritt?",
                "de",
            ),
        )
        .await;
        assert_no_error(&events);
        assert_completed(&events);
        assert_text_chunks(&events);
    }
}
