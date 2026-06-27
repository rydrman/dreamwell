-- Measurement + sequence state kinds (replace resource/clock).
ALTER TABLE game_state_entries ADD COLUMN float_value REAL;
ALTER TABLE game_state_entries ADD COLUMN float_min REAL;
ALTER TABLE game_state_entries ADD COLUMN float_max REAL;
ALTER TABLE game_state_entries ADD COLUMN unit TEXT;

ALTER TABLE chat_state_entries ADD COLUMN float_value REAL;
ALTER TABLE chat_state_entries ADD COLUMN float_min REAL;
ALTER TABLE chat_state_entries ADD COLUMN float_max REAL;
ALTER TABLE chat_state_entries ADD COLUMN unit TEXT;

ALTER TABLE story_state_entries ADD COLUMN float_value REAL;
ALTER TABLE story_state_entries ADD COLUMN float_min REAL;
ALTER TABLE story_state_entries ADD COLUMN float_max REAL;
ALTER TABLE story_state_entries ADD COLUMN unit TEXT;

-- resource/gauge → measurement (float value, optional max bound)
UPDATE game_state_entries
SET
    kind = 'measurement',
    float_value = CAST(COALESCE(num_value, 0) AS REAL),
    float_min = NULL,
    float_max = CASE
        WHEN max_value IS NOT NULL THEN CAST(max_value AS REAL)
        ELSE NULL
    END,
    num_value = NULL,
    max_value = NULL
WHERE kind IN ('resource', 'gauge');

UPDATE chat_state_entries
SET
    kind = 'measurement',
    float_value = CAST(COALESCE(num_value, 0) AS REAL),
    float_min = NULL,
    float_max = CASE
        WHEN max_value IS NOT NULL THEN CAST(max_value AS REAL)
        ELSE NULL
    END,
    num_value = NULL,
    max_value = NULL
WHERE kind IN ('resource', 'gauge');

UPDATE story_state_entries
SET
    kind = 'measurement',
    float_value = CAST(COALESCE(num_value, 0) AS REAL),
    float_min = NULL,
    float_max = CASE
        WHEN max_value IS NOT NULL THEN CAST(max_value AS REAL)
        ELSE NULL
    END,
    num_value = NULL,
    max_value = NULL
WHERE kind IN ('resource', 'gauge');

-- clock → sequence (segment labels 1..max, position from num_value)
UPDATE game_state_entries
SET
    kind = 'sequence',
    value = (
        SELECT printf(
            '{"items":%s,"position":%d,"loop":false}',
            json_group_array(CAST(n AS TEXT)),
            MIN(COALESCE(game_state_entries.num_value, 0), COALESCE(game_state_entries.max_value, 4) - 1)
        )
        FROM (
            WITH RECURSIVE cnt(n) AS (
                SELECT 1
                UNION ALL
                SELECT n + 1 FROM cnt
                WHERE n < COALESCE(game_state_entries.max_value, 4)
            )
            SELECT n FROM cnt
        )
    ),
    num_value = NULL,
    max_value = NULL
WHERE kind = 'clock';

UPDATE chat_state_entries
SET
    kind = 'sequence',
    value = (
        SELECT printf(
            '{"items":%s,"position":%d,"loop":false}',
            json_group_array(CAST(n AS TEXT)),
            MIN(COALESCE(chat_state_entries.num_value, 0), COALESCE(chat_state_entries.max_value, 4) - 1)
        )
        FROM (
            WITH RECURSIVE cnt(n) AS (
                SELECT 1
                UNION ALL
                SELECT n + 1 FROM cnt
                WHERE n < COALESCE(chat_state_entries.max_value, 4)
            )
            SELECT n FROM cnt
        )
    ),
    num_value = NULL,
    max_value = NULL
WHERE kind = 'clock';

UPDATE story_state_entries
SET
    kind = 'sequence',
    value = (
        SELECT printf(
            '{"items":%s,"position":%d,"loop":false}',
            json_group_array(CAST(n AS TEXT)),
            MIN(COALESCE(story_state_entries.num_value, 0), COALESCE(story_state_entries.max_value, 4) - 1)
        )
        FROM (
            WITH RECURSIVE cnt(n) AS (
                SELECT 1
                UNION ALL
                SELECT n + 1 FROM cnt
                WHERE n < COALESCE(story_state_entries.max_value, 4)
            )
            SELECT n FROM cnt
        )
    ),
    num_value = NULL,
    max_value = NULL
WHERE kind = 'clock';
