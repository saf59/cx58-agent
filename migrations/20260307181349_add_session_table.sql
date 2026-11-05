CREATE TABLE chat_session (
    user_id     TEXT        NOT NULL PRIMARY KEY,
    chat_id     TEXT        NOT NULL,
    object_id   TEXT        NULL,
    prev_leaf   TEXT        NULL,
    next_leaf   TEXT        NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);