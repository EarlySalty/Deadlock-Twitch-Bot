ALTER TABLE social_production_jobs DROP CONSTRAINT IF EXISTS social_production_jobs_source_check;
ALTER TABLE social_production_jobs ADD CONSTRAINT social_production_jobs_source_check
    CHECK (source IN ('autopilot', 'manual', 'test'));
