CREATE TABLE user_model_settings (
    user_id      TEXT        NOT NULL PRIMARY KEY,
    vision_model TEXT        NOT NULL,
    text_model   TEXT        NOT NULL,
    chat_model   TEXT        NOT NULL,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
