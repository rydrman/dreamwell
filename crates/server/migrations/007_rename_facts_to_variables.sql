ALTER TABLE facts RENAME TO chat_variables;
ALTER TABLE app_settings RENAME COLUMN facts_enabled TO variables_enabled;
