ALTER TABLE messages ADD COLUMN thought_content TEXT NOT NULL DEFAULT '';
ALTER TABLE messages ADD COLUMN thought_duration_ms INTEGER;
ALTER TABLE messages ADD COLUMN thought_in_progress INTEGER NOT NULL DEFAULT 0;
