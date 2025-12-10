-- ============================================================================
-- Tree Nodes Table
-- ============================================================================

CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "vector"; -- For pgvector embeddings

CREATE TYPE node_type_enum AS ENUM ('Root', 'Branch', 'ImageLeaf');

CREATE TABLE tree_nodes (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL,
    parent_id UUID REFERENCES tree_nodes(id) ON DELETE CASCADE,
    node_type node_type_enum NOT NULL,
    
    -- JSONB for flexible data storage
    data JSONB NOT NULL,
    
    -- Materialized path for efficient tree queries
    path TEXT NOT NULL,
    
    -- For ordering siblings
    sort_order INTEGER DEFAULT 0,
    
    -- Metadata
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Indexes
    CONSTRAINT tree_nodes_user_id_idx CHECK (user_id IS NOT NULL)
);

CREATE INDEX idx_tree_nodes_user_id ON tree_nodes(user_id);
CREATE INDEX idx_tree_nodes_parent_id ON tree_nodes(parent_id);
CREATE INDEX idx_tree_nodes_path ON tree_nodes USING BTREE(path);
CREATE INDEX idx_tree_nodes_type ON tree_nodes(node_type);
CREATE INDEX idx_tree_nodes_data_gin ON tree_nodes USING GIN(data);

-- ============================================================================
-- Image Embeddings (for Qdrant sync)
-- ============================================================================

CREATE TABLE image_embeddings (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    node_id UUID NOT NULL REFERENCES tree_nodes(id) ON DELETE CASCADE,
    embedding VECTOR(512), -- Adjust dimension based on your model
    model_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    UNIQUE(node_id, model_name)
);

CREATE INDEX idx_image_embeddings_node_id ON image_embeddings(node_id);

-- ============================================================================
-- Chat Messages
-- ============================================================================

CREATE TYPE message_role_enum AS ENUM ('User', 'Assistant', 'System');

CREATE TABLE chat_messages (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    chat_id UUID NOT NULL,
    user_id UUID NOT NULL,
    session_id TEXT NOT NULL,
    
    role message_role_enum NOT NULL,
    content TEXT NOT NULL,
    
    -- References to tree nodes
    tree_refs UUID[] DEFAULT '{}',
    
    -- Token usage tracking
    input_tokens INTEGER,
    output_tokens INTEGER,
    
    -- Metadata
    metadata JSONB DEFAULT '{}',
    
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_chat_messages_chat_id ON chat_messages(chat_id);
CREATE INDEX idx_chat_messages_user_id ON chat_messages(user_id);
CREATE INDEX idx_chat_messages_session_id ON chat_messages(session_id);
CREATE INDEX idx_chat_messages_created_at ON chat_messages(created_at DESC);

-- ============================================================================
-- Agent Tool Calls (for observability)
-- ============================================================================

CREATE TABLE agent_tool_calls (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    message_id UUID NOT NULL REFERENCES chat_messages(id) ON DELETE CASCADE,
    tool_name TEXT NOT NULL,
    tool_input JSONB NOT NULL,
    tool_output JSONB,
    status TEXT NOT NULL, -- 'started', 'completed', 'failed'
    error_message TEXT,
    duration_ms INTEGER,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_agent_tool_calls_message_id ON agent_tool_calls(message_id);
CREATE INDEX idx_agent_tool_calls_tool_name ON agent_tool_calls(tool_name);

-- ============================================================================
-- Image Descriptions Cache
-- ============================================================================

CREATE TABLE image_descriptions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    node_id UUID NOT NULL REFERENCES tree_nodes(id) ON DELETE CASCADE,
    model_name TEXT NOT NULL,
    prompt TEXT NOT NULL,
    description TEXT NOT NULL,
    confidence FLOAT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    UNIQUE(node_id, model_name, prompt)
);

CREATE INDEX idx_image_descriptions_node_id ON image_descriptions(node_id);

-- ============================================================================
-- Helper Functions
-- ============================================================================

-- Function to get full tree path
CREATE OR REPLACE FUNCTION get_tree_path(node_id UUID)
RETURNS TABLE(id UUID, parent_id UUID, node_type node_type_enum, data JSONB, depth INTEGER) AS $$
    WITH RECURSIVE tree_path AS (
        SELECT 
            t.id, 
            t.parent_id, 
            t.node_type, 
            t.data,
            0 as depth
        FROM tree_nodes t
        WHERE t.id = node_id
        
        UNION ALL
        
        SELECT 
            t.id, 
            t.parent_id, 
            t.node_type, 
            t.data,
            tp.depth + 1
        FROM tree_nodes t
        INNER JOIN tree_path tp ON t.parent_id = tp.id
    )
    SELECT * FROM tree_path ORDER BY depth;
$$ LANGUAGE SQL;

-- Function to get all children recursively
CREATE OR REPLACE FUNCTION get_tree_children(node_id UUID, max_depth INTEGER DEFAULT 10)
RETURNS TABLE(id UUID, parent_id UUID, node_type node_type_enum, data JSONB, depth INTEGER) AS $$
    WITH RECURSIVE tree_children AS (
        SELECT 
            t.id, 
            t.parent_id, 
            t.node_type, 
            t.data,
            0 as depth
        FROM tree_nodes t
        WHERE t.id = node_id
        
        UNION ALL
        
        SELECT 
            t.id, 
            t.parent_id, 
            t.node_type, 
            t.data,
            tc.depth + 1
        FROM tree_nodes t
        INNER JOIN tree_children tc ON t.parent_id = tc.id
        WHERE tc.depth < max_depth
    )
    SELECT * FROM tree_children;
$$ LANGUAGE SQL;

-- Function to update materialized path on insert/update
CREATE OR REPLACE FUNCTION update_tree_path()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.parent_id IS NULL THEN
        NEW.path := NEW.id::TEXT;
    ELSE
        SELECT path || '.' || NEW.id::TEXT INTO NEW.path
        FROM tree_nodes
        WHERE id = NEW.parent_id;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER tree_nodes_path_trigger
    BEFORE INSERT OR UPDATE ON tree_nodes
    FOR EACH ROW
    EXECUTE FUNCTION update_tree_path();

-- Function to prevent circular references
CREATE OR REPLACE FUNCTION prevent_circular_tree()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.parent_id IS NOT NULL THEN
        IF EXISTS (
            SELECT 1 FROM tree_nodes
            WHERE id = NEW.parent_id
            AND path LIKE NEW.id::TEXT || '%'
        ) THEN
            RAISE EXCEPTION 'Circular reference detected';
        END IF;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER tree_nodes_circular_check
    BEFORE INSERT OR UPDATE ON tree_nodes
    FOR EACH ROW
    EXECUTE FUNCTION prevent_circular_tree();

-- ============================================================================
-- Sample Data Insertion
-- ============================================================================

-- Insert sample user and root node
DO $$
DECLARE
    sample_user_id UUID := '00000000-0000-0000-0000-000000000001';
    root_id UUID := uuid_generate_v4();
    branch1_id UUID := uuid_generate_v4();
    branch2_id UUID := uuid_generate_v4();
BEGIN
    -- Root node
    INSERT INTO tree_nodes (id, user_id, parent_id, node_type, data, sort_order)
    VALUES (
        root_id,
        sample_user_id,
        NULL,
        'Root',
        '{"title": "My Project"}'::JSONB,
        0
    );

    -- Branch 1
    INSERT INTO tree_nodes (id, user_id, parent_id, node_type, data, sort_order)
    VALUES (
        branch1_id,
        sample_user_id,
        root_id,
        'Branch',
        '{"label": "Category A", "description": "First category"}'::JSONB,
        0
    );

    -- Branch 2
    INSERT INTO tree_nodes (id, user_id, parent_id, node_type, data, sort_order)
    VALUES (
        branch2_id,
        sample_user_id,
        root_id,
        'Branch',
        '{"label": "Category B", "description": "Second category"}'::JSONB,
        1
    );

    -- Image leaves
    INSERT INTO tree_nodes (user_id, parent_id, node_type, data, sort_order)
    VALUES 
        (sample_user_id, branch1_id, 'ImageLeaf', 
         '{"url": "https://example.com/image1.jpg", "description": "Sample image 1"}'::JSONB, 0),
        (sample_user_id, branch1_id, 'ImageLeaf',
         '{"url": "https://example.com/image2.jpg", "description": "Sample image 2"}'::JSONB, 1),
        (sample_user_id, branch2_id, 'ImageLeaf',
         '{"url": "https://example.com/image3.jpg", "description": "Sample image 3"}'::JSONB, 0);
END $$;

-- ============================================================================
-- Useful Queries
-- ============================================================================

-- Get full tree for a user
COMMENT ON FUNCTION get_tree_children IS 
'Example usage: SELECT * FROM get_tree_children(''root-uuid-here'') ORDER BY depth, sort_order;';

-- Get all image nodes for a user
COMMENT ON TABLE tree_nodes IS 
'Query images: SELECT * FROM tree_nodes WHERE user_id = $1 AND node_type = ''ImageLeaf'';';

-- Get chat history with tree references
COMMENT ON TABLE chat_messages IS 
'Query with refs: SELECT m.*, array_agg(t.data) as referenced_nodes 
FROM chat_messages m 
LEFT JOIN tree_nodes t ON t.id = ANY(m.tree_refs) 
WHERE m.chat_id = $1 
GROUP BY m.id 
ORDER BY m.created_at;';
