CREATE TABLE IF NOT EXISTS inference_connections (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    inference_url TEXT NOT NULL,
    api_key TEXT NOT NULL DEFAULT ''
);

ALTER TABLE app_settings ADD COLUMN active_inference_connection_id INTEGER REFERENCES inference_connections(id);

INSERT INTO inference_connections (name, inference_url, api_key)
SELECT 'Default', inference_url, '' FROM app_settings WHERE id = 1;

UPDATE app_settings
SET active_inference_connection_id = (
    SELECT id FROM inference_connections ORDER BY id LIMIT 1
)
WHERE id = 1;
