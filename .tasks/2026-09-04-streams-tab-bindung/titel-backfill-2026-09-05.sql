-- Titel-Backfill historischer Sessions, 2026-09-05 (Protokoll des tatsächlichen Ablaufs, kein ausführbares Skript).
-- Beide Schritte teilen sich die untenstehende CTE-Kette (leer, aus_samples, aus_updates, wahl).
-- Ausgeführte Abfolge:
--   Schritt 1 (vor dem UPDATE): SELECT der Rücknahme-Liste nach titel-backfill-2026-09-05.tsv
--   Schritt 2: das UPDATE, Ergebnis UPDATE 956

WITH leer AS (
  SELECT s.id, s.streamer_login, s.twitch_user_id, s.started_at, s.ended_at
  FROM twitch_stream_sessions s
  WHERE s.ended_at IS NOT NULL AND (s.stream_title IS NULL OR s.stream_title='')
),
aus_samples AS (
  SELECT DISTINCT ON (l.id) l.id, st.stream_title AS titel, count(*) AS n
  FROM leer l JOIN twitch_stats_tracked st
    ON st.streamer=l.streamer_login AND st.ts_utc BETWEEN l.started_at AND l.ended_at AND st.stream_title<>''
  GROUP BY l.id, st.stream_title
  ORDER BY l.id, n DESC
),
aus_updates AS (
  SELECT DISTINCT ON (l.id) l.id, u.title AS titel
  FROM leer l JOIN twitch_channel_updates u
    ON u.twitch_user_id=l.twitch_user_id AND u.title<>''
   AND u.recorded_at BETWEEN l.started_at - interval '24 hours' AND l.ended_at
  ORDER BY l.id, u.recorded_at DESC
),
wahl AS (
  SELECT l.id, l.streamer_login, l.started_at, COALESCE(a.titel, b.titel) AS titel,
         CASE WHEN a.titel IS NOT NULL THEN 'samples' WHEN b.titel IS NOT NULL THEN 'updates' END AS quelle
  FROM leer l LEFT JOIN aus_samples a ON a.id=l.id LEFT JOIN aus_updates b ON b.id=l.id
)

-- Schritt 1, vor dem UPDATE, Ausgabe nach titel-backfill-2026-09-05.tsv:
-- SELECT id||E'\t'||quelle||E'\t'||replace(titel,E'\t',' ') FROM wahl WHERE titel IS NOT NULL ORDER BY id;

-- Schritt 2, das UPDATE (Ergebnis: UPDATE 956):
-- UPDATE twitch_stream_sessions s SET stream_title = w.titel FROM wahl w
--   WHERE w.id = s.id AND w.titel IS NOT NULL AND (s.stream_title IS NULL OR s.stream_title = '');
