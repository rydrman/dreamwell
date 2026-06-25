ALTER TABLE generation_jobs ADD COLUMN generation_provider TEXT NOT NULL DEFAULT '';
ALTER TABLE generation_jobs ADD COLUMN generation_model TEXT NOT NULL DEFAULT '';
ALTER TABLE generation_jobs ADD COLUMN generation_notice TEXT NOT NULL DEFAULT '';
