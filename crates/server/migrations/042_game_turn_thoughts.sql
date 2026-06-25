ALTER TABLE game_turns ADD COLUMN thought_content TEXT NOT NULL DEFAULT '';
ALTER TABLE game_turns ADD COLUMN thought_duration_ms INTEGER;
ALTER TABLE game_turns ADD COLUMN thought_in_progress INTEGER NOT NULL DEFAULT 0;
