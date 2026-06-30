ALTER TABLE app_settings ADD COLUMN model_profiles_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE app_settings ADD COLUMN chat_model_plan TEXT NOT NULL DEFAULT '';
ALTER TABLE app_settings ADD COLUMN chat_model_prose TEXT NOT NULL DEFAULT '';
ALTER TABLE app_settings ADD COLUMN chat_temperature_plan REAL;
ALTER TABLE app_settings ADD COLUMN chat_top_p_plan REAL;
ALTER TABLE app_settings ADD COLUMN chat_temperature_prose REAL;
ALTER TABLE app_settings ADD COLUMN chat_top_p_prose REAL;

ALTER TABLE games ADD COLUMN temperature_checks REAL;
ALTER TABLE games ADD COLUMN top_p_checks REAL;
ALTER TABLE games ADD COLUMN temperature_resolve REAL;
ALTER TABLE games ADD COLUMN top_p_resolve REAL;
ALTER TABLE games ADD COLUMN temperature_prose REAL;
ALTER TABLE games ADD COLUMN top_p_prose REAL;
