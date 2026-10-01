SELECT LOWER(BTRIM(ls.streamer_login)) AS channel_login,
       ls.twitch_user_id AS streamer_user_id,
       (COALESCE(ls.is_live, 0) = 1
        AND COALESCE(c.live_deadlock, FALSE)
        AND COALESCE(c.live_checked_at BETWEEN $1 - INTERVAL '90 seconds' AND $1, FALSE)
        AND c.channel_login = LOWER(BTRIM(ls.streamer_login))) AS is_live,
       ls.last_game AS game,
       ls.last_viewer_count AS viewer_count,
       CASE WHEN c.source = 'helix'
                 AND c.checked_at BETWEEN $1 - INTERVAL '12 hours' AND $1
                 AND c.channel_login = LOWER(BTRIM(ls.streamer_login))
            THEN c.follower_count END AS follower_count,
       COALESCE(
           CASE WHEN c.cooldown_until > $1 THEN c.cooldown_until::text END,
           (SELECT MAX(NULLIF(BTRIM(o.cooldown_until), '')::timestamptz)::text
            FROM twitch_partner_outreach o
            WHERE o.streamer_user_id = ls.twitch_user_id
               OR LOWER(BTRIM(o.streamer_login)) = LOWER(BTRIM(ls.streamer_login)))) AS cooldown_until,
       EXISTS (SELECT 1 FROM twitch_partners p WHERE p.status = 'active'
               AND (p.twitch_user_id = ls.twitch_user_id
                    OR LOWER(p.twitch_login) = LOWER(BTRIM(ls.streamer_login)))) AS partner_table,
       EXISTS (SELECT 1 FROM twitch_streamers_partner_state ps
               WHERE COALESCE(ps.is_partner_active, 0) <> 0
                 AND (ps.twitch_user_id = ls.twitch_user_id
                      OR LOWER(ps.twitch_login) = LOWER(BTRIM(ls.streamer_login)))) AS partner_state,
       EXISTS (SELECT 1 FROM twitch_raid_blacklist b
               WHERE b.target_id = ls.twitch_user_id
                  OR LOWER(b.target_login) = LOWER(BTRIM(ls.streamer_login))) AS blacklisted,
       (SELECT MAX(s.ended_at) FROM twitch_smalltalk_sessions s
        WHERE s.streamer_user_id = ls.twitch_user_id
           OR LOWER(s.channel_login) = LOWER(BTRIM(ls.streamer_login))) AS last_observed_at,
       ls.twitch_user_id AS detected_at
FROM twitch_live_state ls
LEFT JOIN twitch_smalltalk_candidate_state c ON c.twitch_user_id = ls.twitch_user_id
WHERE ls.twitch_user_id ~ '^[0-9]+$'
  AND LOWER(BTRIM(ls.streamer_login)) ~ '^[a-z0-9_]+$'
  AND ($2::text IS NULL OR ls.twitch_user_id = $2)
