// tests/integration/main.rs
//
// Integration test suite entry point.
//
// Prerequisites:
//   - Running cx58-agent server at http://127.0.0.1:3000 (release build)
//   - .env with: DATABASE_URL, TEST_USER_ID, AGENT_SECRET,
//     OLLAMA_URL, TEXT_MODEL, VISION_MODEL, CHAT_MODEL
//   - Object "Room 11" in DB for TEST_USER_ID with at least 2 reports
//
// AppState is initialised once (OnceLock) — all tests share one DB pool.
//
// Run:
//   cargo test --test integration -- --test-threads=1

mod common;

mod get_object_tree;
mod get_report_list;
mod describe_report;
mod compare_reports;
mod rag_query;
mod context_request;
