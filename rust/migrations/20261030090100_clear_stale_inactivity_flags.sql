UPDATE twitch_partners p
   SET inactivity_flagged_at = NULL
 WHERE p.inactivity_flagged_at IS NOT NULL
   AND NULLIF(p.inactivity_flagged_at::text, '')::timestamptz < TIMESTAMPTZ '2026-10-17 15:36:42+00'
   AND (
       SELECT COUNT(DISTINCT (s.started_at AT TIME ZONE 'Europe/Berlin')::date)
         FROM twitch_stream_sessions s
        WHERE LOWER(s.streamer_login) = LOWER(p.twitch_login)
          AND COALESCE(s.had_deadlock_in_session, FALSE)
          AND s.started_at >= NOW() - INTERVAL '60 days'
   ) >= 4;
