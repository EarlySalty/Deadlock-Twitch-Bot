CREATE TABLE IF NOT EXISTS twitch_stream_sessions_duration_repair_backup AS
SELECT id, duration_seconds
FROM twitch_stream_sessions
WHERE ended_at IS NOT NULL
  AND duration_seconds > 172800
  AND ABS(duration_seconds - EXTRACT(EPOCH FROM ended_at)) < 864000;

UPDATE twitch_stream_sessions
SET duration_seconds = GREATEST(0, EXTRACT(EPOCH FROM (ended_at - started_at)))::int
WHERE ended_at IS NOT NULL
  AND duration_seconds > 172800
  AND ABS(duration_seconds - EXTRACT(EPOCH FROM ended_at)) < 864000;
