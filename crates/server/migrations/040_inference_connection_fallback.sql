ALTER TABLE inference_connections ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1;
ALTER TABLE inference_connections ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;
ALTER TABLE messages ADD COLUMN generation_notice TEXT NOT NULL DEFAULT '';

UPDATE inference_connections SET sort_order = id;
