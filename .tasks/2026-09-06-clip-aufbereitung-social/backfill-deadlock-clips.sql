-- Einmaliger Backfill fuer die Altzeilen in twitch_clips_social_media, die vor
-- dem Setzen von social_media_category.twitch_game_id angelegt wurden.
--
-- Ursache: Helix GET /clips liefert game_id, aber kein game_name; da die
-- Deadlock-Kategorie keine twitch_game_id trug, landeten alle Clips in 'other'.
-- Die Migration 20260906120000 setzt die twitch_game_id; neue Clips werden damit
-- beim Anlegen korrekt klassifiziert. Diese Datei raeumt einmalig die Altdaten.
--
-- Anwenden gegen die twitch_analytics-Datenbank (Schema public). Kein
-- Dauer-Nachtrag: einmal laufen lassen, danach uebernimmt der Schreibweg.
-- 2132205352 = Twitch-Kategorie-ID von Deadlock.

BEGIN;

UPDATE public.twitch_clips_social_media
   SET category_key = 'deadlock'
 WHERE game_id = '2132205352'
   AND category_key IS DISTINCT FROM 'deadlock';

UPDATE public.twitch_clips_social_media
   SET game_name = 'Deadlock'
 WHERE game_id = '2132205352'
   AND (game_name IS NULL OR game_name = '');

COMMIT;
