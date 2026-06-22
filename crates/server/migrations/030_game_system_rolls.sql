ALTER TABLE games ADD COLUMN rules_blocks_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE games ADD COLUMN state_schema_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE games ADD COLUMN win_condition_json TEXT;
ALTER TABLE games ADD COLUMN scenario_triggers_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE games ADD COLUMN trait_defs_json TEXT NOT NULL DEFAULT '[]';

ALTER TABLE game_turns ADD COLUMN plan_json TEXT;

CREATE TABLE IF NOT EXISTS game_turn_system_rolls (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    turn_id         INTEGER NOT NULL REFERENCES game_turns(id) ON DELETE CASCADE,
    label           TEXT NOT NULL,
    dice_expr       TEXT NOT NULL DEFAULT '1d6',
    rolls           TEXT NOT NULL DEFAULT '[]',
    outcome_key     TEXT NOT NULL DEFAULT '',
    outcome_summary TEXT NOT NULL DEFAULT '',
    sort_order      INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_game_turn_system_rolls_turn_id ON game_turn_system_rolls(turn_id);
