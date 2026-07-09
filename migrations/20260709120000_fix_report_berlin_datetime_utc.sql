CREATE OR REPLACE FUNCTION insert_image_leaf(
    p_parent_id UUID,
    p_url TEXT,
    p_berlin_datetime TEXT
)
    RETURNS UUID
    LANGUAGE plpgsql
AS $$
DECLARE
    v_new_id UUID;
    v_parent_path TEXT;
    v_updated_at TIMESTAMP;
BEGIN
    v_new_id := uuidv7();

    SELECT path INTO v_parent_path
    FROM tree_nodes
    WHERE id = p_parent_id;

    IF v_parent_path IS NULL THEN
        RAISE EXCEPTION 'Parent node not found: %', p_parent_id;
    END IF;

    v_updated_at := (
        to_timestamp(p_berlin_datetime, 'DD.MM.YYYY HH24:MI:SS')::timestamp
            AT TIME ZONE 'Europe/Berlin'
        ) AT TIME ZONE 'UTC';

    INSERT INTO tree_nodes (
        id,
        parent_id,
        node_type,
        name,
        data,
        path,
        updated_at
    ) VALUES (
        v_new_id,
        p_parent_id,
        'ImageLeaf'::node_type_enum,
        NULL,
        jsonb_build_object('url', p_url),
        v_parent_path || v_new_id || '/',
        v_updated_at
    );

    RETURN v_new_id;
END;
$$;
