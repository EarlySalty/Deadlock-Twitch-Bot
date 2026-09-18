ALTER TABLE twitch_ad_break_events
    ADD COLUMN IF NOT EXISTS match_state TEXT,
    ADD COLUMN IF NOT EXISTS seconds_since_match_start INTEGER,
    ADD COLUMN IF NOT EXISTS seconds_since_match_end INTEGER,
    ADD COLUMN IF NOT EXISTS chat_msgs_last_min INTEGER,
    ADD COLUMN IF NOT EXISTS chat_msgs_last_5min INTEGER,
    ADD COLUMN IF NOT EXISTS viewers_before INTEGER,
    ADD COLUMN IF NOT EXISTS raid_in_window BOOLEAN,
    ADD COLUMN IF NOT EXISTS first_chatter_in_window BOOLEAN,
    ADD COLUMN IF NOT EXISTS source TEXT,
    ADD COLUMN IF NOT EXISTS in_window BOOLEAN,
    ADD COLUMN IF NOT EXISTS decision_id BIGINT;

CREATE INDEX IF NOT EXISTS twitch_ad_break_events_session_started
    ON twitch_ad_break_events (session_id, started_at);
