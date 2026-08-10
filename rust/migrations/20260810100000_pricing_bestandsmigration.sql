-- Pricing-Umbau 2026-08-09: Bestand auf `free` und `premium` umschreiben.
--
-- Zuordnung laut Spec:
--   raid_free                          -> free
--   chat_quiet | raid_boost |
--   analysis_dashboard | bundle_*      -> premium, wenn laufend oder unbefristet
--                                      -> free, wenn abgelaufen
--   analytics_trial                    -> laufend: unveraendert (der Trial ist
--                                        Premium auf Zeit und traegt den
--                                        Namen, den die Trial-Ende-Anzeige
--                                        braucht)
--                                      -> abgelaufen: free
--   keine manual_plan_id               -> unveraendert (loest ohnehin auf free auf)
--
-- Datumsfelder werden NICHT angefasst. Timosius behaelt sein 2027-08-08, die
-- laufenden Trials behalten ihr Datum, und ein abgelaufenes Datum auf einer
-- `free`-Zeile bleibt als Spur stehen: die Plan-Aufloesung wertet einen
-- abgelaufenen manuellen Plan ohnehin als Default, das Ergebnis ist identisch.
--
-- `plan_name` (Legacy-Anzeigefeld) wird mitgezogen, damit nicht zwei
-- Wahrheiten in derselben Zeile stehen. Die Werte kommen aus
-- `tb_analytics::stripe::webhook_apply::plan_name_from_id`.

-- Sicherung der Ausgangswerte. Tabelle statt CSV-Datei: COPY TO braucht
-- Superuser und schreibt auf dem DB-Host, eine Tabelle faehrt in derselben
-- Transaktion mit. CSV daraus bei Bedarf:
--   \copy (SELECT * FROM public.streamer_plans_pricing_backup_20260810) TO 'bestand.csv' CSV HEADER
CREATE TABLE IF NOT EXISTS public.streamer_plans_pricing_backup_20260810 AS
SELECT twitch_user_id,
       twitch_login,
       manual_plan_id,
       manual_plan_expires_at,
       plan_name,
       NOW() AS gesichert_am
  FROM public.streamer_plans;

-- 1) Laufende oder unbefristete Bezahlplaene -> premium.
UPDATE public.streamer_plans
   SET manual_plan_id = 'premium',
       plan_name      = 'premium'
 WHERE TRIM(COALESCE(manual_plan_id, '')) IN (
           'chat_quiet', 'raid_boost', 'analysis_dashboard',
           'bundle_chat_quiet_raid_boost', 'bundle_werbefrei_analyse',
           'bundle_komplett', 'bundle_analysis_raid_boost'
       )
   AND (
           NULLIF(TRIM(COALESCE(manual_plan_expires_at, '')), '') IS NULL
           OR (
               TRIM(manual_plan_expires_at) ~ '^\d{4}-\d{2}-\d{2}'
               AND TRIM(manual_plan_expires_at)::timestamptz > NOW()
           )
       );

-- 2) Abgelaufene Bezahlplaene und abgelaufene Trials -> free.
--    Ein unparsebares Datum gilt als abgelaufen; es gibt keinen Fall, in dem
--    Muell in dieser Spalte einen laufenden Anspruch belegen soll.
UPDATE public.streamer_plans
   SET manual_plan_id = 'free',
       plan_name      = 'free'
 WHERE TRIM(COALESCE(manual_plan_id, '')) IN (
           'chat_quiet', 'raid_boost', 'analysis_dashboard', 'analytics_trial',
           'bundle_chat_quiet_raid_boost', 'bundle_werbefrei_analyse',
           'bundle_komplett', 'bundle_analysis_raid_boost'
       )
   AND NULLIF(TRIM(COALESCE(manual_plan_expires_at, '')), '') IS NOT NULL
   AND NOT (
           TRIM(manual_plan_expires_at) ~ '^\d{4}-\d{2}-\d{2}'
           AND TRIM(manual_plan_expires_at)::timestamptz > NOW()
       );

-- 3) raid_free ist der alte Gratisplan.
UPDATE public.streamer_plans
   SET manual_plan_id = 'free',
       plan_name      = 'free'
 WHERE TRIM(COALESCE(manual_plan_id, '')) = 'raid_free';
