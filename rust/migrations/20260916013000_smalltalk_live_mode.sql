ALTER TABLE twitch_engagement_settings
    DROP CONSTRAINT IF EXISTS twitch_engagement_settings_output_mode_chk;

ALTER TABLE twitch_engagement_settings
    ADD CONSTRAINT twitch_engagement_settings_output_mode_chk
    CHECK (output_mode IN ('off', 'shadow', 'live', 'test', 'smalltalk_live'));
