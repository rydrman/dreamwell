ALTER TABLE app_settings ADD COLUMN user_name TEXT NOT NULL DEFAULT 'User';

UPDATE app_settings
SET system_prompt_prefix = 'Write {{char}}''s next reply in a fictional chat between {{char}} and {{user}}.'
WHERE system_prompt_prefix = '';

UPDATE app_settings
SET user_name = 'User'
WHERE user_name = '';

UPDATE chats
SET character_id = (SELECT id FROM characters ORDER BY id LIMIT 1)
WHERE character_id IS NULL
  AND EXISTS (SELECT 1 FROM characters);

DELETE FROM chats WHERE character_id IS NULL;
