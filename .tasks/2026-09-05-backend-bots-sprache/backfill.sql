-- REQ-04: einmaliges Nachfuellen leerer twitch_stream_sessions.language.
-- Quelle: der juengste twitch_channel_updates-Wert derselben twitch_user_id mit
-- gesetzter Sprache, dessen recorded_at nicht nach dem Session-Ende liegt.
-- Nur Sessions mit leerer oder NULL Sprache werden angefasst. Kein Schemawechsel.
-- Nicht vom Implementierer ausfuehren; der Orchestrator fuehrt es als postgres aus.

-- 1) Zaehl-SELECT: wie viele Sessions bekommen einen Wert, bevor das UPDATE laeuft.
SELECT COUNT(*) AS betroffene_sessions
FROM twitch_stream_sessions s
WHERE COALESCE(s.language, '') = ''
  AND s.twitch_user_id IS NOT NULL
  AND s.ended_at IS NOT NULL
  AND EXISTS (
      SELECT 1
        FROM twitch_channel_updates u
       WHERE u.twitch_user_id = s.twitch_user_id
         AND COALESCE(u.language, '') <> ''
         AND u.recorded_at <= s.ended_at
  );

-- 2) UPDATE: je Session der juengste passende Kanalwert.
UPDATE twitch_stream_sessions s
   SET language = quelle.language
  FROM (
      SELECT DISTINCT ON (s2.id) s2.id AS session_id, u.language AS language
        FROM twitch_stream_sessions s2
        JOIN twitch_channel_updates u
          ON u.twitch_user_id = s2.twitch_user_id
         AND COALESCE(u.language, '') <> ''
         AND u.recorded_at <= s2.ended_at
       WHERE COALESCE(s2.language, '') = ''
         AND s2.twitch_user_id IS NOT NULL
         AND s2.ended_at IS NOT NULL
       ORDER BY s2.id, u.recorded_at DESC
  ) AS quelle
 WHERE s.id = quelle.session_id
   AND COALESCE(s.language, '') = '';
