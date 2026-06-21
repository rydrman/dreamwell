ALTER TABLE game_turns ADD COLUMN is_opening INTEGER NOT NULL DEFAULT 0;

INSERT INTO game_turns (
    game_id,
    sort_order,
    player_action,
    phase,
    scene_beats,
    prose,
    state_changes,
    is_opening,
    created_at,
    updated_at
)
SELECT
    g.id,
    -1,
    '',
    'done',
    '[]',
    trim(g.opening_message),
    '[]',
    1,
    g.updated_at,
    g.updated_at
FROM games g
WHERE trim(g.opening_message) != ''
  AND NOT EXISTS (
      SELECT 1 FROM game_turns t WHERE t.game_id = g.id AND t.is_opening = 1
  );
