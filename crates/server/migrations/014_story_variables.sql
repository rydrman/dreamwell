CREATE TABLE IF NOT EXISTS story_variables (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    story_id INTEGER NOT NULL REFERENCES stories(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    value TEXT NOT NULL DEFAULT '',
    source_chapter_order INTEGER NOT NULL DEFAULT 0,
    source_beat_order INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL,
    UNIQUE(story_id, key)
);

ALTER TABLE story_beats ADD COLUMN variable_updates TEXT NOT NULL DEFAULT '[]';
