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

		RAISE NOTICE 'Leafs data added successfully';
        
    END
$$;

-- SELECT * FROM get_tree('alexandr.shpirkov@ispredict.com')
-- SELECT * FROM get_tree('shpirkov@gmail.com')
