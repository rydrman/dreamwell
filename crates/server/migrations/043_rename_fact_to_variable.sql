-- Rename typed state kind 'fact' → 'variable' (aligns with chat/story variable terminology).
UPDATE game_state_entries SET kind = 'variable' WHERE kind = 'fact';
UPDATE chat_state_entries SET kind = 'variable' WHERE kind = 'fact';
UPDATE story_state_entries SET kind = 'variable' WHERE kind = 'fact';
