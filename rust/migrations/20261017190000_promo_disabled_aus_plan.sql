UPDATE public.streamer_plans AS sp
   SET promo_disabled = 1
 WHERE COALESCE(sp.promo_disabled, 0) <> 0
    OR (
        btrim(COALESCE(sp.manual_plan_id, '')) IN (
            'plus', 'pro', 'studio', 'studio_ki', 'studio_cut',
            'chat_quiet', 'bundle_chat_quiet_raid_boost',
            'bundle_analysis_raid_boost', 'bundle_werbefrei_analyse',
            'bundle_komplett'
        )
        AND NOT (
            CASE
                WHEN btrim(COALESCE(sp.manual_plan_expires_at, '')) ~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'
                    THEN ((btrim(sp.manual_plan_expires_at)::date + TIME '23:59:59') AT TIME ZONE 'UTC') <= NOW()
                WHEN btrim(COALESCE(sp.manual_plan_expires_at, '')) ~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}[ T][0-9]{2}:[0-9]{2}:[0-9]{2}'
                    THEN btrim(sp.manual_plan_expires_at)::timestamptz <= NOW()
                ELSE FALSE
            END
        )
    )
    OR (
        NOT (
            btrim(COALESCE(sp.manual_plan_id, '')) IN (
                'free', 'plus', 'studio', 'studio_ki', 'studio_cut', 'pro',
                'raid_free', 'chat_quiet', 'raid_boost',
                'bundle_chat_quiet_raid_boost', 'analysis_dashboard',
                'bundle_analysis_raid_boost', 'bundle_werbefrei_analyse',
                'bundle_komplett', 'analytics_trial'
            )
            AND NOT (
                CASE
                    WHEN btrim(COALESCE(sp.manual_plan_expires_at, '')) ~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'
                        THEN ((btrim(sp.manual_plan_expires_at)::date + TIME '23:59:59') AT TIME ZONE 'UTC') <= NOW()
                    WHEN btrim(COALESCE(sp.manual_plan_expires_at, '')) ~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}[ T][0-9]{2}:[0-9]{2}:[0-9]{2}'
                        THEN btrim(sp.manual_plan_expires_at)::timestamptz <= NOW()
                    ELSE FALSE
                END
            )
        )
        AND EXISTS (
            SELECT 1
              FROM public.twitch_billing_subscriptions AS b
             WHERE b.status IN ('active', 'trialing', 'past_due')
               AND b.plan_id IN (
                    'plus', 'pro', 'studio', 'studio_ki', 'studio_cut',
                    'chat_quiet', 'bundle_chat_quiet_raid_boost',
                    'bundle_analysis_raid_boost', 'bundle_werbefrei_analyse',
                    'bundle_komplett'
               )
               AND (
                    b.customer_reference = sp.twitch_user_id
                    OR (
                        COALESCE(sp.twitch_login, '') <> ''
                        AND LOWER(b.customer_reference) = LOWER(sp.twitch_login)
                    )
               )
        )
    );
