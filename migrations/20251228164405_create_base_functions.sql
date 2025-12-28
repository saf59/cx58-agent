-- ============================================================================
-- Helper Functions
-- ============================================================================
CREATE OR REPLACE FUNCTION from_updated(
    updated_at TIMESTAMP,
    zone TEXT DEFAULT 'Europe/Berlin'
)
    RETURNS TEXT
    LANGUAGE plpgsql
    IMMUTABLE
AS $$
BEGIN
    -- Конвертируем timestamp в указанную timezone и форматируем
RETURN to_char(
        updated_at AT TIME ZONE 'UTC' AT TIME ZONE zone,
        'DD.MM.YYYY HH24:MI:SS'
       );
END;
$$;

---
CREATE OR REPLACE FUNCTION get_full_node_name(p_node_id UUID)
    RETURNS TEXT
    LANGUAGE plpgsql
    STABLE
AS $$
DECLARE
v_full_name TEXT;
BEGIN
WITH RECURSIVE node_path AS (
    SELECT
        id,
        parent_id,
        COALESCE(name, from_updated(updated_at)) AS name,
        0 AS level
    FROM tree_nodes
    WHERE id = p_node_id

    UNION ALL

    SELECT
        tn.id,
        tn.parent_id,
        COALESCE(tn.name, from_updated(tn.updated_at)) AS name,
        np.level + 1 AS level
    FROM tree_nodes tn
             INNER JOIN node_path np ON tn.id = np.parent_id
)
SELECT
    string_agg(name, '/' ORDER BY level DESC)
INTO v_full_name
FROM node_path;

RETURN v_full_name;
END;
$$;

