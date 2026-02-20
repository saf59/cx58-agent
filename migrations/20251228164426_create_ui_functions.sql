CREATE OR REPLACE FUNCTION get_tree(
    req_user_id TEXT,
    with_leafs BOOLEAN DEFAULT true
)
RETURNS TABLE(
    id UUID,
    parent_id UUID,
    node_type node_type_enum,
    name TEXT,
    data JSONB,
    path TEXT,
    updated_at TIMESTAMP,
    depth INTEGER,
    own BOOLEAN
) AS $$
WITH RECURSIVE user_nodes AS (
    -- Все узлы с доступом
    SELECT node_id
    FROM node_access
    WHERE user_id = req_user_id
),
accessible_tree AS (
    -- Начинаем с Root
    SELECT
        tn.id,
        tn.parent_id,
        tn.node_type,
        COALESCE(tn.name, from_updated(tn.updated_at)) as name,
        tn.data,
        tn.path,
        tn.updated_at,
        0 AS depth,
        false AS own  -- Root сам по себе не "свой"
    FROM tree_nodes tn
    WHERE tn.node_type = 'Root'

    UNION ALL

    -- Рекурсивно получаем дочерние
    SELECT
        tn.id,
        tn.parent_id,
        tn.node_type,
        COALESCE(tn.name, from_updated(tn.updated_at)) as name,
        tn.data,
        tn.path,
        tn.updated_at,
        at.depth + 1 AS depth,
        -- own = true если узел в user_nodes или родитель уже own
        (EXISTS(SELECT 1 FROM user_nodes un WHERE un.node_id = tn.id) OR at.own) AS own
    FROM tree_nodes tn
    INNER JOIN accessible_tree at ON tn.parent_id = at.id
)
-- Финальная выборка: показываем только те узлы, которые либо own, либо на пути к own
SELECT DISTINCT
    at.id,
    at.parent_id,
    at.node_type,
    at.name,
    at.data,
    at.path,
    at.updated_at,
    at.depth,
    at.own
FROM accessible_tree at
WHERE (at.own  -- Показываем own узлы и их потомков
   OR EXISTS(  -- И узлы на пути к own узлам
    SELECT 1
    FROM accessible_tree at2
    WHERE at2.own = true
  AND at2.path LIKE at.path || '%'
    ))
  AND (with_leafs = true OR at.node_type != 'ImageLeaf')  -- Фильтруем ImageLeaf если нужно
ORDER BY at.path, at.depth;
$$ LANGUAGE SQL;

-- examples:
-- SELECT * FROM get_tree('alexandr.shpirkov@ispredict.com')
-- SELECT * FROM get_tree('shpirkov@gmail.com')
-- SELECT * FROM get_tree('mock')
-- SELECT * FROM get_tree('none')

--
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
    -- Генерируем новый ID
    v_new_id := uuidv7();

    -- Получаем path родителя
SELECT path INTO v_parent_path
FROM tree_nodes
WHERE id = p_parent_id;

IF v_parent_path IS NULL THEN
        RAISE EXCEPTION 'Parent node not found: %', p_parent_id;
END IF;

    -- Конвертируем берлинское время в UTC timestamp
    v_updated_at := (
        to_timestamp(p_berlin_datetime, 'DD.MM.YYYY HH24:MI:SS')
            AT TIME ZONE 'Europe/Berlin'
        )::TIMESTAMP;

    -- Вставляем новый узел
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
-- SELECT get_full_node_name('e839dfae-2cf6-4ac4-a1c5-6213739bfaa9'); -- is Root/B/12.12.2025 19:38:11
-- Функция получения узла и его дочерних ImageLeaf с полным именем
CREATE OR REPLACE FUNCTION get_node_with_leafs(
    p_node_id UUID,
    p_limit INTEGER DEFAULT 1,
    p_from_timestamp TIMESTAMP DEFAULT NULL,
    p_to_timestamp TIMESTAMP DEFAULT NULL
)
    RETURNS TABLE(
                     id UUID,
                     parent_id UUID,
                     node_type node_type_enum,
                     name TEXT,
                     data JSONB,
                     path TEXT,
                     updated_at TIMESTAMP,
                     full_name TEXT
                 )
    LANGUAGE plpgsql
    STABLE
AS $$
BEGIN
RETURN QUERY
    WITH target_node AS (
            -- Получаем целевой узел
            SELECT
                tn.id,
                tn.parent_id,
                tn.node_type,
                COALESCE(tn.name, from_updated(tn.updated_at)) as name,
                tn.data,
                tn.path,
                tn.updated_at,
                get_full_node_name(tn.id) as full_name
            FROM tree_nodes tn
            WHERE tn.id = p_node_id
        ),
             child_leafs AS (
                 -- Получаем все дочерние ImageLeaf
                 SELECT
                     tn.id,
                     tn.parent_id,
                     tn.node_type,
                     COALESCE(tn.name, from_updated(tn.updated_at)) as name,
                     tn.data,
                     tn.path,
                     tn.updated_at,
                     get_full_node_name(tn.id) as full_name
                 FROM tree_nodes tn
                 WHERE tn.parent_id = p_node_id
                   AND tn.node_type = 'ImageLeaf'
             ),
             time_based_selection AS (
                 -- Top 1 после to_timestamp
                 (
                     SELECT * FROM child_leafs cl
                     WHERE p_to_timestamp IS NOT NULL
                       AND cl.updated_at > p_to_timestamp
                     ORDER BY cl.updated_at
                     LIMIT 1
                 )
                 UNION
                 -- Top 1 после from_timestamp
                 (
                     SELECT * FROM child_leafs cl
                     WHERE p_from_timestamp IS NOT NULL
                       AND cl.updated_at > p_from_timestamp
                     ORDER BY cl.updated_at
                     LIMIT 1
                 )
             ),
             regular_selection AS (
                 -- Обычная выборка с лимитом (когда нет временных параметров)
                 SELECT * FROM child_leafs cl
                 WHERE p_from_timestamp IS NULL AND p_to_timestamp IS NULL
                 ORDER BY cl.updated_at DESC
                 LIMIT p_limit
             ),
             combined_leafs AS (
                 -- Объединяем результаты в зависимости от наличия временных параметров
                 SELECT * FROM time_based_selection
                 WHERE p_from_timestamp IS NOT NULL OR p_to_timestamp IS NOT NULL

                 UNION

                 SELECT * FROM regular_selection
                 WHERE p_from_timestamp IS NULL AND p_to_timestamp IS NULL
             )
-- Финальная выборка: узел + отфильтрованные leafs
SELECT
    tn.id,
    tn.parent_id,
    tn.node_type,
    tn.name,
    tn.data,
    tn.path,
    tn.updated_at,
    tn.full_name
FROM target_node tn

UNION ALL

SELECT DISTINCT
    cl.id,
    cl.parent_id,
    cl.node_type,
    cl.name,
    cl.data,
    cl.path,
    cl.updated_at,
    cl.full_name
FROM combined_leafs cl

ORDER BY updated_at DESC;
END;
$$;

-- Примеры использования:

-- 1. Получить узел и 1 последний ImageLeaf (по умолчанию)
-- SELECT * FROM get_node_with_leafs('019b61a9-f8e0-7374-971f-62ebf1d591ff');

-- 2. Получить узел и 5 последних ImageLeaf
-- SELECT * FROM get_node_with_leafs('019b61a9-f8e0-7374-971f-62ebf1d591ff', 5);

-- 3. Получить узел и ImageLeaf с временными рамками
-- SELECT * FROM get_node_with_leafs('019b61a9-f8e0-7374-971f-62ebf1d591ff',1,'2024-01-01 00:00:00'::TIMESTAMP,'2024-12-31 23:59:59'::TIMESTAMP);

-- 4. Получить узел и Top 1 после определенной даты
-- SELECT * FROM get_node_with_leafs('019b61a9-f8e0-7374-971f-62ebf1d591ff',1,'2025-12-20 00:00:00'::TIMESTAMP,NULL);

