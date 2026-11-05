DO
$$
    DECLARE
        root_id  UUID;
        small_id UUID;
    BEGIN
        SELECT id INTO root_id
        FROM tree_nodes
        WHERE node_type = 'Root'
        LIMIT 1;

        IF root_id IS NULL THEN
            RAISE EXCEPTION 'Root node not found in tree_nodes table';
        END IF;

        SELECT id INTO small_id
        FROM tree_nodes
        WHERE parent_id = root_id
          AND name = 'Small Objects'
          AND node_type = 'Branch'
        LIMIT 1;

        IF small_id IS NULL THEN
            RAISE EXCEPTION 'Small Objects node not found in tree_nodes table';
        END IF;

        INSERT INTO node_access (user_id, node_id)
        SELECT v.user_id, small_id
        FROM (
                 VALUES ('shpirkov@gmail.com'),
                        ('britta.hilt@ispredict.com')
             ) AS v(user_id)
        WHERE NOT EXISTS (
            SELECT 1
            FROM node_access na
            WHERE na.user_id = v.user_id
              AND na.node_id = small_id
        );

        RAISE NOTICE 'Access to Small Objects granted successfully';
    END
$$;
