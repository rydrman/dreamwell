CREATE TABLE IF NOT EXISTS games (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    title           TEXT NOT NULL DEFAULT 'Untitled Game',
    premise         TEXT NOT NULL DEFAULT '',
    setting         TEXT NOT NULL DEFAULT '',
    gm_style        TEXT NOT NULL DEFAULT '',
    resolution_system TEXT NOT NULL DEFAULT 'pbta_2d6',
    modifier_min    INTEGER NOT NULL DEFAULT -2,
    modifier_max    INTEGER NOT NULL DEFAULT 3,
    merge_resolve_scene INTEGER NOT NULL DEFAULT 1,
    step_mode       INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS game_actors (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id     INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    role        TEXT NOT NULL DEFAULT 'pc',
    name        TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    skills      TEXT NOT NULL DEFAULT '{}',
    sort_order  INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS game_state_entries (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id       INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    actor_id      INTEGER REFERENCES game_actors(id) ON DELETE CASCADE,
    kind          TEXT NOT NULL,
    key           TEXT NOT NULL,
    value         TEXT NOT NULL DEFAULT '',
    num_value     INTEGER,
    max_value     INTEGER,
    source_turn   INTEGER NOT NULL DEFAULT -1,
    updated_at    TEXT NOT NULL,
    UNIQUE(game_id, actor_id, kind, key)
);

CREATE TABLE IF NOT EXISTS game_turns (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id       INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    sort_order    INTEGER NOT NULL DEFAULT 0,
    player_action TEXT NOT NULL DEFAULT '',
    phase         TEXT NOT NULL DEFAULT 'pending',
    scene_beats   TEXT NOT NULL DEFAULT '[]',
    prose         TEXT NOT NULL DEFAULT '',
    state_changes TEXT NOT NULL DEFAULT '[]',
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS game_turn_checks (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    turn_id       INTEGER NOT NULL REFERENCES game_turns(id) ON DELETE CASCADE,
    label         TEXT NOT NULL DEFAULT '',
    skill         TEXT NOT NULL DEFAULT '',
    modifier      INTEGER NOT NULL DEFAULT 0,
    stakes        TEXT NOT NULL DEFAULT '',
    justification TEXT NOT NULL DEFAULT '',
    dice_expr     TEXT NOT NULL DEFAULT '2d6',
    seed          INTEGER NOT NULL DEFAULT 0,
    rolls         TEXT NOT NULL DEFAULT '[]',
    total         INTEGER NOT NULL DEFAULT 0,
    tier          TEXT NOT NULL DEFAULT '',
    margin        INTEGER NOT NULL DEFAULT 0,
    sort_order    INTEGER NOT NULL DEFAULT 0,
    created_at    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS game_scenes (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id         INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    title           TEXT NOT NULL DEFAULT '',
    summary         TEXT NOT NULL DEFAULT '',
    summary_valid   INTEGER NOT NULL DEFAULT 0,
    summary_at      TEXT,
    start_turn      INTEGER NOT NULL DEFAULT 0,
    sort_order      INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

ALTER TABLE generation_jobs ADD COLUMN game_id INTEGER REFERENCES games(id) ON DELETE CASCADE;
ALTER TABLE generation_jobs ADD COLUMN turn_id INTEGER REFERENCES game_turns(id) ON DELETE CASCADE;
