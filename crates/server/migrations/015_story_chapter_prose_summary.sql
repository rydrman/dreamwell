ALTER TABLE story_chapters ADD COLUMN prose_summary TEXT NOT NULL DEFAULT '';
ALTER TABLE story_chapters ADD COLUMN prose_summary_valid INTEGER NOT NULL DEFAULT 0;
ALTER TABLE story_chapters ADD COLUMN prose_summary_at TEXT;
