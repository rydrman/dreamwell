ALTER TABLE inference_connections ADD COLUMN model TEXT NOT NULL DEFAULT '';

UPDATE inference_connections
SET model = (SELECT model FROM app_settings WHERE id = 1);
