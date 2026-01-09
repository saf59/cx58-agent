DO
$$
    DECLARE
        root_id      UUID;
        branch1_id   UUID := uuidv7();
        branch11_id  UUID := uuidv7();
        branch2_id   UUID := uuidv7();
        branch21_id  UUID := uuidv7();
        branch211_id UUID := uuidv7();
        branch3_id   UUID := uuidv7();
    BEGIN
        -- Get root_id from existing Root node
        SELECT id INTO root_id
        FROM tree_nodes
        WHERE node_type = 'Root'
        LIMIT 1;

        -- Check if root exists
        IF root_id IS NULL THEN
            RAISE EXCEPTION 'Root node not found in tree_nodes table';
        END IF;

        -- Branch 1
        INSERT INTO tree_nodes (id, parent_id, name, node_type, data)
        VALUES (branch1_id,
                root_id,
                'Object 1',
                'Branch',
                '{}'::JSONB);
        INSERT INTO tree_nodes (id, parent_id, name, node_type, data)
        VALUES (branch11_id,
                branch1_id,
                'Room 11',
                'Branch',
                '{}'::JSONB);

        -- Branch 2
        INSERT INTO tree_nodes (id, parent_id, name, node_type, data)
        VALUES (branch2_id,
                root_id,
                'Object 2',
                'Branch',
                '{}'::JSONB);
        INSERT INTO tree_nodes (id, parent_id, name, node_type, data)
        VALUES (branch21_id,
                branch2_id,
                'Floor 21',
                'Branch',
                '{}'::JSONB);
        INSERT INTO tree_nodes (id, parent_id, name, node_type, data)
        VALUES (branch211_id,
                branch21_id,
                'Room 211',
                'Branch',
                '{}'::JSONB);
        -- Branch 3
        INSERT INTO tree_nodes (id, parent_id, name, node_type, data)
        VALUES (branch3_id,
                root_id,
                'Object 3',
                'Branch',
                '{
                  "label": "Mock"
                }'::JSONB);

        RAISE NOTICE 'Base tree_nodes data created successfully';

        INSERT INTO node_access (user_id, node_id)
        VALUES ('shpirkov@gmail.com', branch11_id),
               ('shpirkov@gmail.com', branch211_id),
               ('alexandr.shpirkov@ispredict.com', branch211_id),
               ('mock', branch3_id);

        RAISE NOTICE 'Access data created successfully';
    END
$$;

-- SELECT * FROM get_tree('alexandr.shpirkov@ispredict.com')
-- SELECT * FROM get_tree('shpirkov@gmail.com')