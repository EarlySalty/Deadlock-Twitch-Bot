-- twitch_viewer_presence_ticks zur Hypertable mit Compression machen.
--
-- Die Tabelle ist die groesste der Datenbank (636 MB, 2,94 Mio Zeilen, 44 % des
-- Gesamtvolumens) und als einzige grosse Zeitreihe bisher eine normale Tabelle.
-- docs/VIEWER_PRESENCE_TIMELINE.md sah die Konvertierung von Anfang an vor, der
-- Deploy-Schritt wurde nur nie ausgefuehrt.
--
-- Warum sich Compression hier besonders lohnt: pro Viewer und Session stehen im
-- Schnitt 168 Ticks (Maximum 2684), jeder mit erneut ausgeschriebenem
-- streamer_login und viewer_login -- bei nur 53 verschiedenen Streamern und
-- 4929 verschiedenen Viewern auf 2,94 Mio Zeilen. Dictionary-Encoding faltet
-- das zusammen, tick_at wird als 30-Sekunden-Delta kodiert. Verlustfrei: jede
-- Zeile bleibt einzeln abrufbar, SELECT * liefert unveraendert dieselben Daten.
--
-- compress_segmentby ist session_id und NICHT streamer_login, weil jede
-- Produktivquery auf genau diese Spalte filtert:
--   tb-analytics/src/post_stream.rs:1065  WHERE session_id = $1
--   tb-dashboard-api/src/handlers/viewer_timeline.rs:187  WHERE session_id = $1
-- Damit liegt eine Session in einem Segment und wird gelesen, ohne fremde
-- Segmente auszupacken.
--
-- Voraussetzung: timescaledb-Extension ist im Schema installiert (siehe
-- 20260601000100_observability_hypertable.sql). Idempotent via if_not_exists.
--
-- Die Tabelle ist append-only (pg_stat_user_tables: n_tup_upd = 0,
-- n_tup_del = 0), damit gibt es keinen Konflikt mit komprimierten Chunks.
-- Der PK (session_id, viewer_login, tick_at) enthaelt die Partitionsspalte
-- tick_at und bleibt daher unveraendert gueltig; der FK auf
-- twitch_stream_sessions zeigt von der Hypertable weg und ist zulaessig.

SELECT create_hypertable(
    'public.twitch_viewer_presence_ticks',
    'tick_at',
    if_not_exists => TRUE,
    migrate_data => TRUE,
    chunk_time_interval => INTERVAL '7 days'
);

DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM timescaledb_information.hypertables
                 WHERE hypertable_schema = 'public'
                   AND hypertable_name = 'twitch_viewer_presence_ticks'
                   AND compression_enabled) THEN
    EXECUTE 'ALTER TABLE public.twitch_viewer_presence_ticks SET ('
         || 'timescaledb.compress, '
         || 'timescaledb.compress_segmentby = ''session_id'', '
         || 'timescaledb.compress_orderby = ''tick_at DESC'')';
  END IF;
END $$;

-- 7 Tage wie bei allen anderen Event-Hypertables. Frische Chunks bleiben
-- unkomprimiert, weil laufende Sessions am haeufigsten gelesen werden.
SELECT add_compression_policy(
    'public.twitch_viewer_presence_ticks',
    INTERVAL '7 days',
    if_not_exists => TRUE
);
