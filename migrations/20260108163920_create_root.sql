DO $$
DECLARE
    root_id UUID := uuidv7();
BEGIN
    -- Root node
    INSERT INTO tree_nodes (id, parent_id, name, node_type, data)
    VALUES (root_id,
            NULL,
            'Root',
            'Root',
            '{
              "title": "CX-5.8"
            }'::JSONB);
END
$$;