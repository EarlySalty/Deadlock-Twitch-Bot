ALTER TABLE twitch_ad_break_events
    ADD COLUMN IF NOT EXISTS match_state TEXT
        CHECK (match_state IN ('queue', 'first_match_minute', 'in_match', 'post_match')),
    ADD COLUMN IF NOT EXISTS seconds_since_match_start INTEGER,
    ADD COLUMN IF NOT EXISTS seconds_since_match_end INTEGER,
    ADD COLUMN IF NOT EXISTS chat_msgs_last_min INTEGER,
    ADD COLUMN IF NOT EXISTS chat_msgs_last_5min INTEGER,
    ADD COLUMN IF NOT EXISTS viewers_before INTEGER,
    ADD COLUMN IF NOT EXISTS raid_in_window BOOLEAN,
    ADD COLUMN IF NOT EXISTS first_chatter_in_window BOOLEAN,
    ADD COLUMN IF NOT EXISTS source TEXT
        CHECK (source IN ('twitch_plan', 'bot_pulled_forward', 'bot_own_block', 'manual')),
    ADD COLUMN IF NOT EXISTS in_window BOOLEAN,
    ADD COLUMN IF NOT EXISTS decision_id BIGINT
        REFERENCES twitch_ad_manager_decisions(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS twitch_ad_break_events_session_started
    ON twitch_ad_break_events (session_id, started_at);
