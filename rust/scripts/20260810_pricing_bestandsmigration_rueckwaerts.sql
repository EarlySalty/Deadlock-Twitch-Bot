-- Rueckwaerts-Skript zu migrations/20260810100000_pricing_bestandsmigration.sql.
--
-- Stellt manual_plan_id und plan_name aus der Sicherungstabelle wieder her.
-- Wird nicht vom Migrator ausgefuehrt, sondern von Hand:
--   psql "$DSN" -f rust/scripts/20260810_pricing_bestandsmigration_rueckwaerts.sql
--
-- Datumsfelder wurden von der Migration nicht angefasst und werden deshalb
-- auch hier nicht angefasst.
--
-- Achtung: das Skript dreht die Zuordnung fuer die Zeilen um, die zum
-- Migrationszeitpunkt existierten. Zeilen, die danach entstanden sind, stehen
-- nicht in der Sicherung und bleiben unveraendert — das ist gewollt, ein
-- frisch angelegter `premium`-Kunde soll bei einem Rollback nicht verschwinden.

BEGIN;

UPDATE public.streamer_plans AS p
   SET manual_plan_id = b.manual_plan_id,
       plan_name      = b.plan_name
  FROM public.streamer_plans_pricing_backup_20260810 AS b
 WHERE p.twitch_user_id = b.twitch_user_id
   AND (p.manual_plan_id IS DISTINCT FROM b.manual_plan_id
        OR p.plan_name IS DISTINCT FROM b.plan_name);

-- Gegenprobe vor dem COMMIT: muss 0 Zeilen liefern.
SELECT p.twitch_user_id, p.manual_plan_id AS jetzt, b.manual_plan_id AS soll
  FROM public.streamer_plans AS p
  JOIN public.streamer_plans_pricing_backup_20260810 AS b
    ON p.twitch_user_id = b.twitch_user_id
 WHERE p.manual_plan_id IS DISTINCT FROM b.manual_plan_id
    OR p.plan_name IS DISTINCT FROM b.plan_name;

COMMIT;
