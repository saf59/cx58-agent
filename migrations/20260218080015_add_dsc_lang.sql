-- Add migration script here
ALTER TABLE image_descriptions ADD COLUMN lang TEXT NOT NULL DEFAULT 'en';

ALTER TABLE image_descriptions ADD CONSTRAINT image_descriptions_node_id_lang_key UNIQUE(node_id, lang);

