CREATE TABLE IF NOT EXISTS twitch_caster_overlay_assets (
    id UUID PRIMARY KEY,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    mime TEXT NOT NULL,
    bytes BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT twitch_caster_overlay_assets_kind CHECK (kind IN ('team_logo', 'event_logo', 'brand')),
    CONSTRAINT twitch_caster_overlay_assets_name_len CHECK (char_length(name) BETWEEN 1 AND 120),
    CONSTRAINT twitch_caster_overlay_assets_mime CHECK (mime IN ('image/png', 'image/webp', 'image/jpeg', 'image/svg+xml')),
    CONSTRAINT twitch_caster_overlay_assets_size CHECK (octet_length(bytes) BETWEEN 1 AND 2097152)
);

CREATE INDEX IF NOT EXISTS twitch_caster_overlay_assets_kind_idx
    ON twitch_caster_overlay_assets (kind, created_at DESC);
