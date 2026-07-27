-- Nachtrag zu 20260728120100: viewer_login vor tick_at in compress_orderby.
--
-- Beim Aktivieren der Compression mahnt Timescale die Spalte selbst an
-- ("column viewer_login should be used for segmenting or ordering"), und beide
-- Produktivquerys gruppieren danach:
--   tb-analytics/src/post_stream.rs:1065          GROUP BY viewer_login
--   tb-dashboard-api/.../viewer_timeline.rs:187   PARTITION BY LOWER(viewer_login)
--
-- Auf Prod gemessen (2,94 Mio Zeilen, 17 Chunks):
--   compress_orderby = 'tick_at DESC'                32,4x  ->  84 MB
--   compress_orderby = 'viewer_login, tick_at DESC'  94,1x  ->  75 MB
--
-- Bewusst eine eigene Migration statt einer Aenderung an 20260728120100:
-- eine bereits angewendete Migration nachtraeglich zu editieren bricht die
-- Pruefsumme in _sqlx_migrations, und der compression_enabled-Guard dort
-- greift beim zweiten Lauf nicht als Reparatur.
--
-- Das ALTER wirkt nur auf kuenftige Kompressionen. Bereits komprimierte Chunks
-- behalten ihre alte Sortierung, bis sie neu komprimiert werden -- korrekt
-- lesbar sind sie in beiden Faellen. Auf Prod wurden sie einmalig per
-- decompress_chunk/compress_chunk angeglichen (knapp 10 s fuer alle 17); das
-- gehoert nicht in die Migration, weil es auf grossen Bestaenden beliebig lange
-- laufen kann. Frische Datenbanken sind davon ohnehin nicht betroffen.

DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM timescaledb_information.hypertables
             WHERE hypertable_schema = 'public'
               AND hypertable_name = 'twitch_viewer_presence_ticks'
               AND compression_enabled) THEN
    EXECUTE 'ALTER TABLE public.twitch_viewer_presence_ticks SET ('
         || 'timescaledb.compress, '
         || 'timescaledb.compress_segmentby = ''session_id'', '
         || 'timescaledb.compress_orderby = ''viewer_login, tick_at DESC'')';
  END IF;
END $$;
