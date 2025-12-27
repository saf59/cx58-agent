DO
$$
    DECLARE
        root_id      UUID := uuidv7();
        branch1_id   UUID := uuidv7();
        branch11_id  UUID := uuidv7();
        branch2_id   UUID := uuidv7();
        branch21_id  UUID := uuidv7();
        branch211_id UUID := uuidv7();
        branch3_id   UUID := uuidv7();
    BEGIN
        -- Root node
        INSERT INTO tree_nodes (id, parent_id, name, node_type, data)
        VALUES (root_id,
                   -- sample_user_id,
                NULL,
                'Root',
                'Root',
                '{
                  "title": "CX-5.8"
                }'::JSONB);

        -- Branch 1
        INSERT INTO tree_nodes (id, parent_id, name, node_type, data)
        VALUES (branch1_id,
                root_id,
                'Oblect 1',
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
                'Oblect 2',
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
                'Oblect 3',
                'Branch',
                '{
                  "label": "Mock"
                }'::JSONB);

		RAISE NOTICE 'Base tree_nodes data created successfully';


        PERFORM  insert_image_leaf(branch11_id, '4к_1.jpg', '27.11.2025 17:00:00');
        PERFORM  insert_image_leaf(branch11_id, '4к_2.jpg', '01.12.2025 17:00:00');
        PERFORM  insert_image_leaf(branch11_id, '4к_3.jpg', '15.12.2025 17:00:00');
        PERFORM  insert_image_leaf(branch11_id, '4к_4.jpg', '27.12.2025 17:00:00');

        PERFORM  insert_image_leaf(branch211_id, '3w_1.jpg', '27.11.2025 17:00:00');
        PERFORM  insert_image_leaf(branch211_id, '3w_2.jpg', '01.12.2025 17:00:00');
        PERFORM  insert_image_leaf(branch211_id, '3w_3.jpg', '05.12.2025 17:00:00');
        PERFORM  insert_image_leaf(branch211_id, '3w_4.jpg', '15.12.2025 17:00:00');
        PERFORM  insert_image_leaf(branch211_id, '3w_5.jpg', '27.11.2025 17:00:00');

        PERFORM  insert_image_leaf(branch3_id, 'noise_1.jpg', '27.11.2025 17:00:00');
        PERFORM  insert_image_leaf(branch3_id, 'noise_2.jpg', '27.12.2025 17:00:00');

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
