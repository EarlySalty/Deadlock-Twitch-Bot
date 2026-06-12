-- Beschleunigt Stats-Leaderboard-Abfragen: LOWER(streamer)-Index erlaubt
-- effiziente IN/NOT IN-Filter gegen twitch_streamers_partner_state.
-- TimescaleDB propagiert den Index automatisch auf alle Chunks.
CREATE INDEX IF NOT EXISTS idx_twitch_stats_tracked_streamer_lower
    ON twitch_stats_tracked (LOWER(streamer));

CREATE INDEX IF NOT EXISTS idx_twitch_stats_category_streamer_lower
    ON twitch_stats_category (LOWER(streamer));
