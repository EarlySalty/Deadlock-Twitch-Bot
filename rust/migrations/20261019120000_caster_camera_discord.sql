ALTER TABLE twitch_caster_cameras
    ADD COLUMN IF NOT EXISTS discord_user_id TEXT;

ALTER TABLE twitch_caster_cameras
    ADD CONSTRAINT twitch_caster_cameras_discord_user_id_shape
    CHECK (discord_user_id IS NULL OR discord_user_id ~ '^[0-9]{5,25}$');

CREATE UNIQUE INDEX IF NOT EXISTS twitch_caster_cameras_discord_user_idx
    ON twitch_caster_cameras (discord_user_id)
    WHERE discord_user_id IS NOT NULL;
