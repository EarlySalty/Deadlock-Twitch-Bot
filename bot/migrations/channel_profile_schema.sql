-- Per-Streamer-Background: ein wachsendes Profil pro Channel (Mains, Spielstil,
-- Community-Vibe, Running-Gags), das ein Reflexions-Job aus den Chats des jeweiligen
-- Channels destilliert. Soziales Kontext-Wissen, KEINE harten Spielfakten. Die
-- Pipeline injiziert nur das Profil des gerade behandelten Channels.
-- Idempotent: CREATE ... IF NOT EXISTS.
CREATE TABLE IF NOT EXISTS twitch_engagement_channel_profile (
    channel_login  TEXT PRIMARY KEY,
    profile_text   TEXT NOT NULL,
    msg_count      INT NOT NULL DEFAULT 0,
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
