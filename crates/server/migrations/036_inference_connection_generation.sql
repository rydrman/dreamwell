ALTER TABLE inference_connections ADD COLUMN temperature REAL NOT NULL DEFAULT 0.8;
ALTER TABLE inference_connections ADD COLUMN top_p REAL NOT NULL DEFAULT 0.9;
ALTER TABLE inference_connections ADD COLUMN max_tokens INTEGER NOT NULL DEFAULT 512;
ALTER TABLE inference_connections ADD COLUMN context_tokens INTEGER NOT NULL DEFAULT 8192;
ALTER TABLE inference_connections ADD COLUMN max_context_messages INTEGER NOT NULL DEFAULT 40;
ALTER TABLE inference_connections ADD COLUMN auto_context_on_model_change INTEGER NOT NULL DEFAULT 1;

UPDATE inference_connections
SET
    temperature = (SELECT temperature FROM app_settings WHERE id = 1),
    top_p = (SELECT top_p FROM app_settings WHERE id = 1),
    max_tokens = (SELECT max_tokens FROM app_settings WHERE id = 1),
    context_tokens = (SELECT context_tokens FROM app_settings WHERE id = 1),
    max_context_messages = (SELECT max_context_messages FROM app_settings WHERE id = 1),
    auto_context_on_model_change = (SELECT auto_context_on_model_change FROM app_settings WHERE id = 1);
