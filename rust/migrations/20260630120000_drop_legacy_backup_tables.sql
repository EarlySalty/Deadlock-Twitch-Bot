-- Schema-Reconcile: tote Einmal-Backup-Tabellen aus frueheren
-- Datenkorrekturen sowie die nur in der Baseline vorhandene,
-- ungenutzte twitch_auto_raid_pause entfernen.
--
-- Prod ist der Zielzustand: die Backup-Tabellen werden dort entfernt,
-- twitch_auto_raid_pause fehlt dort bereits. Auf frischen Datenbanken
-- entfernt IF EXISTS nur die jeweils vorhandenen Alt-Tabellen.

DROP TABLE IF EXISTS public.twitch_partner_raid_score_tracking_raid_identity_fix_backup;
DROP TABLE IF EXISTS public.twitch_raid_history_raid_identity_fix_backup;
DROP TABLE IF EXISTS public.twitch_raid_retention_raid_identity_fix_backup;
DROP TABLE IF EXISTS public.twitch_streamers_backup_preconsolidation;
DROP TABLE IF EXISTS public.twitch_auto_raid_pause;
