CREATE TABLE IF NOT EXISTS twitch_caster_teams (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    short TEXT,
    logo_asset_id UUID REFERENCES twitch_caster_overlay_assets (id) ON DELETE SET NULL,
    players JSONB NOT NULL DEFAULT '[]'::jsonb,
    coach_name TEXT,
    coach_discord_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT twitch_caster_teams_name_len CHECK (char_length(name) BETWEEN 1 AND 80),
    CONSTRAINT twitch_caster_teams_short_len CHECK (short IS NULL OR char_length(short) BETWEEN 1 AND 8),
    CONSTRAINT twitch_caster_teams_players_array CHECK (jsonb_typeof(players) = 'array'),
    CONSTRAINT twitch_caster_teams_coach_name_len CHECK (coach_name IS NULL OR char_length(coach_name) BETWEEN 1 AND 80),
    CONSTRAINT twitch_caster_teams_coach_discord_shape CHECK (coach_discord_id IS NULL OR coach_discord_id ~ '^[0-9]{5,25}$')
);

CREATE INDEX IF NOT EXISTS twitch_caster_teams_name_idx
    ON twitch_caster_teams (lower(name));
