-- Typed session state for chat and story (mirrors game_actors / game_state_entries).

CREATE TABLE IF NOT EXISTS chat_actors (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    chat_id     INTEGER NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    role        TEXT NOT NULL DEFAULT 'pc',
    name        TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    skills      TEXT NOT NULL DEFAULT '{}',
    sort_order  INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS chat_state_entries (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    chat_id           INTEGER NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    actor_id          INTEGER REFERENCES chat_actors(id) ON DELETE CASCADE,
    kind              TEXT NOT NULL,
    key               TEXT NOT NULL,
    value             TEXT NOT NULL DEFAULT '',
    num_value         INTEGER,
    max_value         INTEGER,
    source_message_id INTEGER NOT NULL DEFAULT -1,
    updated_at        TEXT NOT NULL,
    UNIQUE(chat_id, actor_id, kind, key)
);

CREATE TABLE IF NOT EXISTS story_actors (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    story_id    INTEGER NOT NULL REFERENCES stories(id) ON DELETE CASCADE,
    role        TEXT NOT NULL DEFAULT 'pc',
    name        TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    skills      TEXT NOT NULL DEFAULT '{}',
    sort_order  INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS story_state_entries (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    story_id       INTEGER NOT NULL REFERENCES stories(id) ON DELETE CASCADE,
    actor_id       INTEGER REFERENCES story_actors(id) ON DELETE CASCADE,
    kind           TEXT NOT NULL,
    key            TEXT NOT NULL,
    value          TEXT NOT NULL DEFAULT '',
    num_value      INTEGER,
    max_value      INTEGER,
    source_beat_id INTEGER NOT NULL DEFAULT -1,
    updated_at     TEXT NOT NULL,
    UNIQUE(story_id, actor_id, kind, key)
);

-- Migrate chat_variables → chat_state_entries (latest value per key wins).
INSERT INTO chat_state_entries (chat_id, actor_id, kind, key, value, source_message_id, updated_at)
SELECT cv.chat_id, NULL, 'fact', cv.key, cv.value, cv.source_message_id, cv.updated_at
FROM chat_variables cv
INNER JOIN (
    SELECT chat_id, key, MAX(updated_at) AS max_updated
    FROM chat_variables
    GROUP BY chat_id, key
) latest
    ON cv.chat_id = latest.chat_id
   AND cv.key = latest.key
   AND cv.updated_at = latest.max_updated;

DROP TABLE chat_variables;

-- Migrate story_variables → story_state_entries (latest value per key wins).
INSERT INTO story_state_entries (story_id, actor_id, kind, key, value, source_beat_id, updated_at)
SELECT sv.story_id, NULL, 'fact', sv.key, sv.value, -1, sv.updated_at
FROM story_variables sv
INNER JOIN (
    SELECT story_id, key, MAX(updated_at) AS max_updated
    FROM story_variables
    GROUP BY story_id, key
) latest
    ON sv.story_id = latest.story_id
   AND sv.key = latest.key
   AND sv.updated_at = latest.max_updated;

DROP TABLE story_variables;

ALTER TABLE messages ADD COLUMN reply_beats TEXT NOT NULL DEFAULT '[]';
ALTER TABLE messages ADD COLUMN state_changes TEXT NOT NULL DEFAULT '[]';
ALTER TABLE messages ADD COLUMN generation_phase TEXT NOT NULL DEFAULT '';

ALTER TABLE story_beats ADD COLUMN plan_beats TEXT NOT NULL DEFAULT '[]';
ALTER TABLE story_beats ADD COLUMN state_changes TEXT NOT NULL DEFAULT '[]';
