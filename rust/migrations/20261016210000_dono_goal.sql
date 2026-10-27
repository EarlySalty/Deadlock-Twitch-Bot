CREATE TABLE IF NOT EXISTS twitch_dono_goal (
    twitch_login TEXT PRIMARY KEY,
    twitch_user_id TEXT NOT NULL DEFAULT '',
    title TEXT NOT NULL DEFAULT '',
    target_amount DOUBLE PRECISION NOT NULL DEFAULT 100,
    currency TEXT NOT NULL DEFAULT 'EUR',
    count_from TIMESTAMPTZ NOT NULL DEFAULT now(),
    se_channel_id TEXT NOT NULL DEFAULT '',
    se_jwt_enc BYTEA,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
