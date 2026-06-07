CREATE TABLE IF NOT EXISTS characters (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    personality TEXT NOT NULL DEFAULT '',
    scenario TEXT NOT NULL DEFAULT '',
    first_message TEXT NOT NULL DEFAULT '',
    example_dialogue TEXT NOT NULL DEFAULT '',
    system_prompt TEXT NOT NULL DEFAULT '',
    avatar_url TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS chats (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL DEFAULT 'New Chat',
    character_id INTEGER REFERENCES characters(id),
    summary TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    chat_id INTEGER NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    content TEXT NOT NULL DEFAULT '',
    is_summary INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS facts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    chat_id INTEGER NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    value TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL,
    UNIQUE(chat_id, key)
);

CREATE TABLE IF NOT EXISTS generation_jobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    chat_id INTEGER NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'queued',
    error TEXT,
    position INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    started_at TEXT,
    completed_at TEXT
);

CREATE TABLE IF NOT EXISTS app_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    inference_url TEXT NOT NULL DEFAULT 'http://localhost:11434/v1',
    model TEXT NOT NULL DEFAULT '',
    temperature REAL NOT NULL DEFAULT 0.8,
    top_p REAL NOT NULL DEFAULT 0.9,
    max_tokens INTEGER NOT NULL DEFAULT 512,
    system_prompt_prefix TEXT NOT NULL DEFAULT '',
    system_prompt_suffix TEXT NOT NULL DEFAULT '',
    summarize_enabled INTEGER NOT NULL DEFAULT 1,
    summarize_after_messages INTEGER NOT NULL DEFAULT 20,
    summarize_keep_recent INTEGER NOT NULL DEFAULT 8,
    facts_enabled INTEGER NOT NULL DEFAULT 1,
    max_context_messages INTEGER NOT NULL DEFAULT 40
);
