ALTER TABLE scenarios ADD COLUMN game_elements_json TEXT NOT NULL DEFAULT '{}';

ALTER TABLE games ADD COLUMN engine_mode TEXT NOT NULL DEFAULT 'pipeline';
ALTER TABLE games ADD COLUMN game_elements_json TEXT NOT NULL DEFAULT '{}';
ALTER TABLE games ADD COLUMN element_instances_json TEXT NOT NULL DEFAULT '{}';

ALTER TABLE game_turns ADD COLUMN mechanical_results_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE game_turns ADD COLUMN observability_json TEXT NOT NULL DEFAULT '{}';
