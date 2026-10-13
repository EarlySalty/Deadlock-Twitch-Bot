ALTER TABLE streamer_plans
    ADD COLUMN clip_command_duration_seconds DOUBLE PRECISION;

ALTER TABLE streamer_plans
    ADD CONSTRAINT streamer_plans_clip_duration_valid
    CHECK (
        clip_command_duration_seconds IS NULL OR (
            clip_command_duration_seconds BETWEEN 5 AND 60
            AND ABS(clip_command_duration_seconds * 10 - ROUND(clip_command_duration_seconds * 10)) < 0.0000001
        )
    );
