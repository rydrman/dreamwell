CREATE TABLE IF NOT EXISTS stories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL DEFAULT 'Untitled Story',
    premise TEXT NOT NULL DEFAULT '',
    tone TEXT NOT NULL DEFAULT '',
    genre TEXT NOT NULL DEFAULT '',
    pov TEXT NOT NULL DEFAULT '',
    length_preset TEXT NOT NULL DEFAULT 'short',
    notes TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS story_chapters (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    story_id INTEGER NOT NULL REFERENCES stories(id) ON DELETE CASCADE,
    title TEXT NOT NULL DEFAULT '',
    synopsis TEXT NOT NULL DEFAULT '',
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS story_beats (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    chapter_id INTEGER NOT NULL REFERENCES story_chapters(id) ON DELETE CASCADE,
    title TEXT NOT NULL DEFAULT '',
    synopsis TEXT NOT NULL DEFAULT '',
    content TEXT NOT NULL DEFAULT '',
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE generation_jobs_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_type TEXT NOT NULL DEFAULT 'chat_message',
    chat_id INTEGER REFERENCES chats(id) ON DELETE CASCADE,
    message_id INTEGER REFERENCES messages(id) ON DELETE CASCADE,
    story_id INTEGER REFERENCES stories(id) ON DELETE CASCADE,
    chapter_id INTEGER REFERENCES story_chapters(id) ON DELETE CASCADE,
    beat_id INTEGER REFERENCES story_beats(id) ON DELETE CASCADE,
    guidance_notes TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'queued',
    error TEXT,
    position INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    started_at TEXT,
    completed_at TEXT
);

INSERT INTO generation_jobs_new (
    id, job_type, chat_id, message_id, status, error, position, created_at, started_at, completed_at
)
SELECT id, 'chat_message', chat_id, message_id, status, error, position, created_at, started_at, completed_at
FROM generation_jobs;

DROP TABLE generation_jobs;
ALTER TABLE generation_jobs_new RENAME TO generation_jobs;
