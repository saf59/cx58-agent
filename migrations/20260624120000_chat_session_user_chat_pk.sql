WITH ranked AS (
    SELECT
        ctid,
        row_number() OVER (
            PARTITION BY user_id, chat_id
            ORDER BY updated_at DESC
        ) AS rn
    FROM chat_session
)
DELETE FROM chat_session
WHERE ctid IN (
    SELECT ctid
    FROM ranked
    WHERE rn > 1
);

ALTER TABLE chat_session
    DROP CONSTRAINT IF EXISTS chat_session_pkey;

ALTER TABLE chat_session
    ADD CONSTRAINT chat_session_pkey PRIMARY KEY (user_id, chat_id);
