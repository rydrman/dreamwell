ALTER TABLE games ADD COLUMN character_id INTEGER REFERENCES characters(id);
