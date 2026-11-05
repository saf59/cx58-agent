-- Add migration script here
ALTER TABLE chat_session
    ADD COLUMN IF NOT EXISTS history JSONB NOT NULL DEFAULT '[]'::jsonb;

COMMENT ON COLUMN chat_session.history IS
    'Last N user+assistant message pairs as JSON array of {"role":"user"|"assistant","text":"..."}.
     Cleared when chat_id changes. Capped at 10 entries by application logic.';

