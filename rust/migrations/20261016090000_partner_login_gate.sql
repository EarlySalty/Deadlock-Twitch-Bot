CREATE FUNCTION public.guild_partner_login_allowed(uid text) RETURNS boolean LANGUAGE sql STABLE AS $$
    WITH current_partner AS (
        SELECT status, technical_pause_reason
        FROM public.twitch_partners
        WHERE twitch_user_id = uid
        ORDER BY CASE COALESCE(status, '') WHEN 'active' THEN 0 WHEN 'archived' THEN 1 WHEN 'departnered' THEN 2 ELSE 3 END,
            COALESCE(departnered_at, admin_archived_at, partnered_at) DESC NULLS LAST
        LIMIT 1
    )
    SELECT NOT EXISTS (
            SELECT 1 FROM public.twitch_partners p
            WHERE p.twitch_user_id = uid
              AND lower(btrim(COALESCE(p.technical_pause_reason, ''))) IN ('blocked', 'bot_banned')
        )
        AND (
            public.guild_partner_allowed(uid)
            OR (
                EXISTS (
                    SELECT 1 FROM current_partner c
                    WHERE COALESCE(c.status, '') IN ('departnered', 'archived')
                       OR (c.status = 'active' AND lower(btrim(COALESCE(c.technical_pause_reason, ''))) LIKE 'token_error%')
                )
                AND EXISTS (
                    SELECT 1 FROM public.streamer_guilds sg
                    JOIN public.discord_guilds g ON g.guild_id = sg.guild_id
                    WHERE sg.twitch_user_id = uid AND sg.guild_id = 1289721245281292288
                      AND COALESCE(sg.deactivated_by, '') <> 'admin'
                      AND g.status = 'active' AND g.removed_at IS NULL
                )
                AND NOT EXISTS (
                    SELECT 1 FROM public.twitch_exclusions e
                    WHERE e.twitch_user_id = uid AND e.reactivated_at IS NULL
                )
                AND NOT EXISTS (
                    SELECT 1 FROM public.twitch_partner_signup_denylist d
                    WHERE d.twitch_user_id = uid OR lower(d.twitch_login) IN (
                        SELECT lower(p.twitch_login) FROM public.twitch_partners p WHERE p.twitch_user_id = uid
                        UNION
                        SELECT lower(i.twitch_login) FROM public.twitch_streamer_identities i WHERE i.twitch_user_id = uid
                    )
                )
            )
        );
$$;
