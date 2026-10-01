CREATE TABLE IF NOT EXISTS twitch_caster_cameras (
    camera_id TEXT PRIMARY KEY,
    owner_login TEXT NOT NULL,
    label TEXT NOT NULL,
    publish_token_hash TEXT NOT NULL,
    consent_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_connected_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    CONSTRAINT twitch_caster_cameras_camera_id_shape CHECK (camera_id ~ '^[A-Za-z0-9_-]{1,80}$'),
    CONSTRAINT twitch_caster_cameras_owner_login_shape CHECK (owner_login ~ '^[a-z0-9_]{1,60}$'),
    CONSTRAINT twitch_caster_cameras_label_len CHECK (char_length(label) BETWEEN 1 AND 80),
    CONSTRAINT twitch_caster_cameras_token_hash_shape CHECK (publish_token_hash ~ '^[0-9a-f]{64}$')
);

CREATE INDEX IF NOT EXISTS twitch_caster_cameras_owner_idx
    ON twitch_caster_cameras (owner_login, created_at DESC);
