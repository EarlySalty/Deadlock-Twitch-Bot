CREATE OR REPLACE FUNCTION public.twitch_streamer_ai_allowed(
    p_twitch_user_id text,
    p_channel_login text DEFAULT NULL
) RETURNS boolean
LANGUAGE sql
STABLE
AS $$
    WITH identity AS (
        SELECT i.twitch_user_id, i.twitch_login
        FROM public.twitch_streamer_identities i
        WHERE i.twitch_user_id ~ '^[0-9]{1,20}$'
          AND NULLIF(BTRIM(i.twitch_login), '') IS NOT NULL
          AND (
              (NULLIF(BTRIM(p_twitch_user_id), '') IS NOT NULL
               AND i.twitch_user_id = BTRIM(p_twitch_user_id)
               AND (NULLIF(BTRIM(p_channel_login), '') IS NULL
                    OR LOWER(i.twitch_login) = LOWER(BTRIM(p_channel_login))))
              OR (NULLIF(BTRIM(p_twitch_user_id), '') IS NULL
                  AND LOWER(i.twitch_login) = LOWER(BTRIM(p_channel_login)))
          )
    ), memberships AS (
        SELECT g.ai_features
        FROM identity i
        JOIN public.streamer_guilds sg ON sg.twitch_user_id = i.twitch_user_id
        JOIN public.discord_guilds g ON g.guild_id = sg.guild_id
        WHERE sg.active AND g.status = 'active' AND g.removed_at IS NULL
    )
    SELECT (SELECT COUNT(*) FROM identity) = 1
       AND EXISTS (SELECT 1 FROM memberships)
       AND NOT EXISTS (SELECT 1 FROM memberships WHERE NOT ai_features)
       AND EXISTS (
           SELECT 1 FROM identity i
           JOIN public.twitch_partners_all_state s ON s.twitch_user_id = i.twitch_user_id
           WHERE s.is_partner_active = 1
       )
       AND NOT EXISTS (
           SELECT 1 FROM identity i
           JOIN public.twitch_exclusions e ON e.twitch_user_id = i.twitch_user_id
           WHERE e.reactivated_at IS NULL
       )
       AND NOT EXISTS (
           SELECT 1 FROM identity i
           JOIN public.twitch_partner_signup_denylist d
             ON d.twitch_user_id = i.twitch_user_id OR LOWER(d.twitch_login) = LOWER(i.twitch_login)
       )
       AND NOT EXISTS (
           SELECT 1 FROM identity i
           JOIN public.twitch_partners p ON p.twitch_user_id = i.twitch_user_id
           WHERE LOWER(TRIM(COALESCE(p.technical_pause_reason, ''))) IN ('blocked', 'bot_banned')
       );
$$;
