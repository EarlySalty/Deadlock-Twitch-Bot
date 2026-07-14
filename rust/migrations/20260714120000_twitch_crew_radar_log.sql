CREATE TABLE IF NOT EXISTS twitch_crew_radar_log (
  id BIGSERIAL PRIMARY KEY,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  channel_login TEXT NOT NULL,
  chatter_login TEXT NOT NULL,
  chatter_id TEXT,
  account_age_days BIGINT,
  style_score SMALLINT NOT NULL,
  style_breakdown JSONB NOT NULL,
  time_window_match BOOLEAN NOT NULL,
  messages JSONB NOT NULL,
  llm_verdict TEXT NOT NULL,
  llm_confidence REAL,
  llm_reasoning TEXT,
  action_taken TEXT NOT NULL DEFAULT 'none',
  source TEXT NOT NULL DEFAULT 'network'
);

CREATE INDEX IF NOT EXISTS twitch_crew_radar_log_created_idx ON twitch_crew_radar_log (created_at DESC);
CREATE INDEX IF NOT EXISTS twitch_crew_radar_log_chatter_idx ON twitch_crew_radar_log (lower(chatter_login));
