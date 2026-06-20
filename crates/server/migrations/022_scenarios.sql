CREATE TABLE IF NOT EXISTS scenarios (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    title           TEXT NOT NULL,
    premise         TEXT NOT NULL DEFAULT '',
    setting         TEXT NOT NULL DEFAULT '',
    gm_style        TEXT NOT NULL DEFAULT '',
    pc_name         TEXT NOT NULL DEFAULT '',
    pc_description  TEXT NOT NULL DEFAULT '',
    character_id    INTEGER REFERENCES characters(id),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

ALTER TABLE games ADD COLUMN scenario_id INTEGER REFERENCES scenarios(id);
