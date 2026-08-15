-- Bestandsmigration Free/Premium. NICHT auf Prod anwenden ohne Freigabe.
-- Vorher-Sicherung in streamer_plans_pricing_backup_20260816.
-- Stop-Regel: nur die Zuordnungstabelle der Spec. Andere IDs bleiben unangetastet.

CREATE TABLE IF NOT EXISTS streamer_plans_pricing_backup_20260816 AS
SELECT twitch_user_id,
       twitch_login,
       manual_plan_id,
       manual_plan_expires_at,
       plan_name,
       trial_ever_granted,
       trials_granted
  FROM streamer_plans;

-- Aktive Alt-Plaene und laufende Trials → premium, Ablaufdatum unveraendert.
UPDATE streamer_plans
   SET manual_plan_id = 'premium',
       plan_name = 'premium'
 WHERE TRIM(COALESCE(manual_plan_id, '')) IN (
           'chat_quiet',
           'raid_boost',
           'analysis_dashboard',
           'analytics_trial',
           'bundle_chat_quiet_raid_boost',
           'bundle_werbefrei_analyse',
           'bundle_komplett',
           'bundle_analysis_raid_boost'
       )
   AND (
           manual_plan_expires_at IS NULL
        OR BTRIM(manual_plan_expires_at) = ''
        OR manual_plan_expires_at::timestamptz >= NOW()
       );

-- Abgelaufene Trials und abgelaufenes analysis_dashboard → free.
UPDATE streamer_plans
   SET manual_plan_id = 'free',
       plan_name = 'free'
 WHERE TRIM(COALESCE(manual_plan_id, '')) IN (
           'analytics_trial',
           'analysis_dashboard'
       )
   AND manual_plan_expires_at IS NOT NULL
   AND BTRIM(manual_plan_expires_at) <> ''
   AND manual_plan_expires_at::timestamptz < NOW();

-- raid_free und leere Plaene → free.
UPDATE streamer_plans
   SET manual_plan_id = 'free',
       plan_name = 'free'
 WHERE TRIM(COALESCE(manual_plan_id, '')) IN ('', 'raid_free');
