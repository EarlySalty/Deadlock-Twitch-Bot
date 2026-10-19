CREATE FUNCTION social_job_releases_clip(job BIGINT) RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT social_video_discarded(job) OR EXISTS(SELECT 1 FROM social_production_jobs j
        WHERE j.id = job AND j.source = 'autopilot' AND j.status = 'cancelled' AND j.stage = 'cleared_no_video')
$$;
