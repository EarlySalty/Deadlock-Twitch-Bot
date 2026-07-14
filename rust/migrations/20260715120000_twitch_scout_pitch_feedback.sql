ALTER TABLE twitch_scout_pitch_ledger
    ADD COLUMN IF NOT EXISTS feedback_up INTEGER,
    ADD COLUMN IF NOT EXISTS feedback_down INTEGER,
    ADD COLUMN IF NOT EXISTS feedback_synced_at TIMESTAMPTZ;
