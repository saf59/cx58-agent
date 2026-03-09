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
    sqlx::query_as!(
        ChatSession,
        r#"SELECT user_id, chat_id, object_id, prev_leaf, next_leaf
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
               chat_id    = $2,
               object_id  = $3,
               prev_leaf  = $4,
               next_leaf  = $5,
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
