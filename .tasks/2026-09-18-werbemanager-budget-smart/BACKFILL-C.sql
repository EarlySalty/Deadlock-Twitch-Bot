-- Einmaliger Backfill des Werbe-Kontexts fuer bestehende twitch_ad_break_events.
-- Von Hand als postgres auf twitch_analytics ausfuehren, nach der Migration
-- 20260918140000_werbemanager_telemetrie.sql. Kein Migrationsfile, kein
-- wiederkehrender Job. Idempotent: fuellt nur Zeilen ohne bereits gesetzte Quelle.
--
-- Match-Zustand gibt es rueckwirkend nicht und bleibt leer (match_state,
-- seconds_since_match_*, in_window). Quelle folgt aus is_automatic, weil vor
-- dem Werbemanager keine Bot-Entscheidungen vorliegen.

UPDATE twitch_ad_break_events a
SET
    source = CASE WHEN COALESCE(a.is_automatic, false) THEN 'twitch_plan' ELSE 'manual' END,
    chat_msgs_last_min = (
        SELECT COUNT(*) FROM twitch_chat_messages m
         WHERE m.session_id = a.session_id
           AND m.message_ts >= a.started_at - INTERVAL '1 minute'
           AND m.message_ts <  a.started_at
    ),
    chat_msgs_last_5min = (
        SELECT COUNT(*) FROM twitch_chat_messages m
         WHERE m.session_id = a.session_id
           AND m.message_ts >= a.started_at - INTERVAL '5 minutes'
           AND m.message_ts <  a.started_at
    ),
    viewers_before = (
        SELECT v.viewer_count FROM twitch_session_viewers v
         WHERE v.session_id = a.session_id
           AND v.ts_utc <= a.started_at
         ORDER BY v.ts_utc DESC
         LIMIT 1
    ),
    raid_in_window = EXISTS (
        SELECT 1 FROM twitch_raid_arrival_tracking r
         WHERE r.to_broadcaster_id = a.twitch_user_id
           AND r.detected_at >= a.started_at - INTERVAL '10 minutes'
           AND r.detected_at <  a.started_at
    ),
    first_chatter_in_window = EXISTS (
        SELECT 1 FROM twitch_first_message_events f
         WHERE f.broadcaster_id = a.twitch_user_id
           AND f.event_ts >= a.started_at - INTERVAL '5 minutes'
           AND f.event_ts <  a.started_at
    )
WHERE a.source IS NULL
  AND a.session_id IS NOT NULL;
