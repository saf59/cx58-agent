DO $$
DECLARE
    deleted_count INTEGER;
BEGIN
    -- Delete all nodes where node_type = 'ImageLeaf'
    DELETE FROM tree_nodes
    WHERE node_type = 'ImageLeaf';
    
    -- Get count of deleted rows
    GET DIAGNOSTICS deleted_count = ROW_COUNT;
    
    -- Output result
    RAISE NOTICE 'Deleted % ImageLeaf nodes', deleted_count;
END
$$;