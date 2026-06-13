-- Allow the same key at different anchors (message / beat position).
-- Manual panel entries use source_message_id = -1 or source_chapter_order = source_beat_order = -1.

PRAGMA foreign_keys = OFF;

CREATE TABLE chat_variables_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    chat_id INTEGER NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    value TEXT NOT NULL DEFAULT '',
    source_message_id INTEGER NOT NULL DEFAULT -1,
    updated_at TEXT NOT NULL,
    UNIQUE(chat_id, key, source_message_id)
);

INSERT INTO chat_variables_new (id, chat_id, key, value, source_message_id, updated_at)
SELECT id, chat_id, key, value, -1, updated_at
FROM chat_variables;

DROP TABLE chat_variables;
ALTER TABLE chat_variables_new RENAME TO chat_variables;

CREATE TABLE story_variables_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    story_id INTEGER NOT NULL REFERENCES stories(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    value TEXT NOT NULL DEFAULT '',
    source_chapter_order INTEGER NOT NULL DEFAULT -1,
    source_beat_order INTEGER NOT NULL DEFAULT -1,
    updated_at TEXT NOT NULL,
    UNIQUE(story_id, key, source_chapter_order, source_beat_order)
);

INSERT INTO story_variables_new (
    id,
    story_id,
    key,
    value,
    source_chapter_order,
    source_beat_order,
    updated_at
)
SELECT id, story_id, key, value, source_chapter_order, source_beat_order, updated_at
FROM story_variables;

DROP TABLE story_variables;
ALTER TABLE story_variables_new RENAME TO story_variables;

PRAGMA foreign_keys = ON;
