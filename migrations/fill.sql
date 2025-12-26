-- ============================================================================
-- Sample Data Insertion
-- ============================================================================

-- Insert sample user and root node
DO $$
DECLARE
    --sample_user_id UUID := '00000000-0000-0000-0000-000000000001';
    root_id UUID := uuid_generate_v4();
    branch1_id UUID := uuid_generate_v4();
    branch2_id UUID := uuid_generate_v4();
BEGIN
    -- Root node
    INSERT INTO tree_nodes (id, user_id, parent_id, node_type, data, sort_order)
    VALUES (
        root_id,
        -- sample_user_id,
        NULL,
        'Root',
        '{"title": "My Project"}'::JSONB,
        0
    );

    -- Branch 1
    INSERT INTO tree_nodes (id, user_id, parent_id, node_type, data, sort_order)
    VALUES (
        branch1_id,
        -- sample_user_id,
        root_id,
        'Branch',
        '{"label": "Category A", "description": "First category"}'::JSONB,
        0
    );

    -- Branch 2
    INSERT INTO tree_nodes (id, user_id, parent_id, node_type, data, sort_order)
    VALUES (
        branch2_id,
        -- sample_user_id,
        root_id,
        'Branch',
        '{"label": "Category B", "description": "Second category"}'::JSONB,
        1
    );

    -- Image leaves
    INSERT INTO tree_nodes (user_id, parent_id, node_type, data, sort_order)
    VALUES 
        ( branch1_id, 'ImageLeaf', '{"url": "https://example.com/image1.jpg", "description": "Sample image 1"}'::JSONB, 0),
        ( branch1_id, 'ImageLeaf', '{"url": "https://example.com/image2.jpg", "description": "Sample image 2"}'::JSONB, 1),
        ( branch2_id, 'ImageLeaf', '{"url": "https://example.com/image3.jpg", "description": "Sample image 3"}'::JSONB, 0);
END $$;

-- ============================================================================
-- Useful Queries
-- ============================================================================

COMMENT ON FUNCTION get_tree_children IS 
'Example: SELECT * FROM get_tree_children(''root-uuid-here'') ORDER BY depth, sort_order;';

COMMENT ON TABLE tree_nodes IS 
'Query images: SELECT * FROM tree_nodes WHERE user_id = $1 AND node_type = ''ImageLeaf'';';

COMMENT ON TABLE chat_messages IS 
'Query with refs: SELECT m.*, array_agg(t.data) as referenced_nodes 
FROM chat_messages m 
LEFT JOIN tree_nodes t ON t.id = ANY(m.tree_refs) 
WHERE m.chat_id = $1 
GROUP BY m.id 
ORDER BY m.id;';