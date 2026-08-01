CREATE TABLE twitch_login_aliases (
    twitch_user_id TEXT NOT NULL,
    login TEXT NOT NULL,
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    is_current BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (twitch_user_id, login)
);

CREATE UNIQUE INDEX twitch_login_aliases_current_user_idx
    ON twitch_login_aliases (twitch_user_id)
    WHERE is_current;

CREATE INDEX twitch_login_aliases_login_idx
    ON twitch_login_aliases (login);
