use sqlx::PgPool;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ChatSession {
    pub user_id:   String,
    pub chat_id:   String,
    pub object_id: Option<String>,
    pub prev_leaf:  Option<String>,
    pub next_leaf:  Option<String>,
}

/// Load the session for the given user_id.
/// Returns None if no session exists, if the stored chat_id differs from the
/// current one (stale session), or on DB error (non-fatal).
pub async fn load_session(
    db: &PgPool,
    user_id: &str,
    chat_id: &str,
) -> Option<ChatSession> {
    let session = sqlx::query_as!(
        ChatSession,
        r#"SELECT user_id, chat_id, object_id, prev_leaf, next_leaf
           FROM chat_session
           WHERE user_id = $1"#,
        user_id,
    )
    .fetch_optional(db)
    .await
    .ok()
    .flatten()?;

    // Discard session if it belongs to a different chat
    if session.chat_id != chat_id {
        return None;
    }

    Some(session)
}

/// Persist resolved IDs for the given user_id.
///
/// One row per user — if chat_id changed all previous IDs are discarded and
/// replaced with the values from the current request.
/// If chat_id is the same, COALESCE ensures that NULL arguments never overwrite
/// an already-resolved value.
/// Errors are logged as warnings and swallowed so they never abort the stream.
pub async fn save_session(
    db: &PgPool,
    user_id: &str,
    chat_id: &str,
    object_id: Option<&str>,
    prev_leaf:  Option<&str>,
    next_leaf:  Option<&str>,
) {
    let result = sqlx::query!(
        r#"INSERT INTO chat_session (user_id, chat_id, object_id, prev_leaf, next_leaf, updated_at)
           VALUES ($1, $2, $3, $4, $5, NOW())
           ON CONFLICT (user_id) DO UPDATE SET
               object_id  = CASE WHEN chat_session.chat_id = $2
                                 THEN COALESCE($3, chat_session.object_id)
                                 ELSE $3 END,
               prev_leaf  = CASE WHEN chat_session.chat_id = $2
                                 THEN COALESCE($4, chat_session.prev_leaf)
                                 ELSE $4 END,
               next_leaf  = CASE WHEN chat_session.chat_id = $2
                                 THEN COALESCE($5, chat_session.next_leaf)
                                 ELSE $5 END,
               chat_id    = $2,
               updated_at = NOW()"#,
        user_id,
        chat_id,
        object_id,
        prev_leaf,
        next_leaf,
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
