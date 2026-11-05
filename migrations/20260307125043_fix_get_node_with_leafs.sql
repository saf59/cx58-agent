CREATE OR REPLACE FUNCTION public.get_node_with_leafs(
    p_node_id uuid,
    p_limit integer DEFAULT 1,
    p_from_timestamp timestamp without time zone DEFAULT NULL,
    p_to_timestamp timestamp without time zone DEFAULT NULL
)
RETURNS TABLE(id uuid, parent_id uuid, node_type node_type_enum, name text,
              data jsonb, path text, updated_at timestamp without time zone, full_name text)
LANGUAGE plpgsql COST 100 STABLE PARALLEL UNSAFE ROWS 1000
AS $BODY$
BEGIN
RETURN QUERY
    WITH target_node AS (
        SELECT
            tn.id, tn.parent_id, tn.node_type,
            COALESCE(tn.name, from_updated(tn.updated_at)) AS name,
            tn.data, tn.path, tn.updated_at,
            get_full_node_name(tn.id) AS full_name
        FROM tree_nodes tn
        WHERE tn.id = p_node_id
    ),
    child_leafs AS (
        SELECT
            tn.id, tn.parent_id, tn.node_type,
            COALESCE(tn.name, from_updated(tn.updated_at)) AS name,
            tn.data, tn.path, tn.updated_at,
            get_full_node_name(tn.id) AS full_name
        FROM tree_nodes tn
        WHERE tn.parent_id = p_node_id
          AND tn.node_type = 'ImageLeaf'
    ),
    selected_leafs AS (
        SELECT * FROM child_leafs cl
        WHERE (p_from_timestamp IS NULL OR cl.updated_at >= p_from_timestamp)
          AND (p_to_timestamp   IS NULL OR cl.updated_at <= p_to_timestamp)
        ORDER BY cl.updated_at DESC
        LIMIT p_limit
    )
SELECT tn.id, tn.parent_id, tn.node_type, tn.name,
       tn.data, tn.path, tn.updated_at, tn.full_name
FROM target_node tn
UNION ALL
SELECT cl.id, cl.parent_id, cl.node_type, cl.name,
       cl.data, cl.path, cl.updated_at, cl.full_name
FROM selected_leafs cl
ORDER BY updated_at DESC;
END;
$BODY$;
