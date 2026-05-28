use sqlx::PgPool;
use serde_json::Value as JsonValue;

/// Maximum number of message pairs kept in history.
/// Older entries are dropped when the limit is reached.
const MAX_HISTORY_ENTRIES: usize = 10;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ChatSession {
    pub user_id:   String,
    pub chat_id:   String,
    pub object_id: Option<String>,
    pub prev_leaf:  Option<String>,
    pub next_leaf:  Option<String>,
    /// Last N message pairs as JSON array: [{"role":"user","text":"..."},{"role":"assistant","text":"..."},...]
    pub history:   JsonValue,
}

impl ChatSession {
    /// Returns history as a flat list of formatted strings for IntentRouter.
    /// Format: "user: <message>" / "assistant: <message>"
    pub fn history_strings(&self) -> Vec<String> {
        self.history
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|entry| {
                        let role = entry["role"].as_str()?;
                        let text = entry["text"].as_str()?;
                        Some(format!("{}: {}", role, text))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Appends a new user+assistant exchange to the history array.
/// Caps the total at MAX_HISTORY_ENTRIES pairs, dropping the oldest.
pub fn append_history(
    existing: &JsonValue,
    user_msg: &str,
    assistant_msg: &str,
) -> JsonValue {
    let mut entries: Vec<JsonValue> = existing
        .as_array()
        .cloned()
        .unwrap_or_default();

    entries.push(serde_json::json!({"role": "user",      "text": user_msg}));
    entries.push(serde_json::json!({"role": "assistant", "text": assistant_msg}));

    // Keep only the last MAX_HISTORY_ENTRIES * 2 individual entries (pairs × 2).
    let max = MAX_HISTORY_ENTRIES * 2;
    if entries.len() > max {
        entries = entries.into_iter().rev().take(max).rev().collect();
    }

    JsonValue::Array(entries)
}

/// Load the session for the given user_id + chat_id.
/// Returns None if no session exists or on DB error (non-fatal).
pub async fn load_session(
    db: &PgPool,
    user_id: &str,
    chat_id: &str,
) -> Option<ChatSession> {
    sqlx::query_as!(
        ChatSession,
        r#"SELECT user_id, chat_id, object_id, prev_leaf, next_leaf,
                  history as "history: JsonValue"
           FROM chat_session
           WHERE user_id = $1 AND chat_id = $2"#,
        user_id,
        chat_id,
    )
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
}

/// Persist resolved IDs and conversation history for the given user_id.
///
/// One row per user — if chat_id changes, all previous IDs and history are
/// discarded and replaced with values from the current request.
/// Errors are logged as warnings and swallowed so they never abort the stream.
pub async fn save_session(
    db: &PgPool,
    user_id: &str,
    chat_id: &str,
    object_id: Option<&str>,
    prev_leaf:  Option<&str>,
    next_leaf:  Option<&str>,
) {
    save_session_with_history(db, user_id, chat_id, object_id, prev_leaf, next_leaf, None).await;
}

/// Like save_session but also updates the conversation history column.
pub async fn save_session_with_history(
    db: &PgPool,
    user_id: &str,
    chat_id: &str,
    object_id: Option<&str>,
    prev_leaf:  Option<&str>,
    next_leaf:  Option<&str>,
    history: Option<&JsonValue>,
) {
    let history_value = history
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));

    let result = sqlx::query!(
        r#"INSERT INTO chat_session (user_id, chat_id, object_id, prev_leaf, next_leaf, history, updated_at)
           VALUES ($1, $2, $3, $4, $5, $6, NOW())
           ON CONFLICT (user_id) DO UPDATE SET
               chat_id    = $2,
               object_id  = $3,
               prev_leaf  = $4,
               next_leaf  = $5,
               history    = CASE
                                WHEN chat_session.chat_id = $2 THEN $6
                                ELSE '[]'::jsonb
                            END,
               updated_at = NOW()"#,
        user_id,
        chat_id,
        object_id,
        prev_leaf,
        next_leaf,
        history_value,
    )
    .execute(db)
    .await;

    if let Err(e) = result {
        tracing::warn!(
            user_id, chat_id,
            "Failed to save chat_session: {}", e
        );
    }
}
