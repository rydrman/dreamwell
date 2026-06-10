ALTER TABLE app_settings ADD COLUMN context_tokens INTEGER NOT NULL DEFAULT 8192;
ALTER TABLE app_settings ADD COLUMN auto_context_on_model_change INTEGER NOT NULL DEFAULT 1;
