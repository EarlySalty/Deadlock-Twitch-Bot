-- Speicher-Cleanup: Indexe entfernen, die seit DB-Erstellung null Scans haben
-- (pg_stat_user_indexes.idx_scan = 0 bei pg_stat_database.stats_reset IS NULL,
-- die Zaehler laufen also ueber die volle Lebensdauer der Datenbank).
--
-- Der mit Abstand groesste Posten ist idx_viewer_presence_ticks_session mit
-- 219 MB. Dessen Redundanz ist nicht nur statistisch, sondern strukturell:
--
--   twitch_viewer_presence_ticks_pkey  UNIQUE btree (session_id, viewer_login, tick_at)
--   idx_viewer_presence_ticks_session         btree (session_id, viewer_login, tick_at)
--
-- Gleiche Spalten, gleiche Reihenfolge, gleiche Opclass. Der PK-Index bedient
-- jede Query, die der Zweitindex bedienen koennte; er ist zusaetzlich UNIQUE.
--
-- Bewusst ohne CONCURRENTLY: DROP INDEX ist eine reine Katalog-Operation und
-- braucht den ACCESS-EXCLUSIVE-Lock nur fuer Millisekunden -- es wird nichts
-- umgeschrieben. Die laengste Produktivquery auf der groessten betroffenen
-- Tabelle laeuft 58 ms (viewer_timeline auf der groessten Session), der Lock
-- wartet also allenfalls so lange. CONCURRENTLY waere hier zusaetzlich gar
-- nicht moeglich: sqlx schickt eine Migrationsdatei als einen Query-String,
-- und PostgreSQL fasst mehrere Statements darin implizit zu einer Transaktion
-- zusammen -- CONCURRENTLY ist im Transaktionsblock verboten (Fehler 25001).
--
-- Rollback: die vollstaendigen Definitionen stehen unten als Kommentar, ein
-- Wiederherstellen ist Copy-Paste.

DROP INDEX IF EXISTS public.idx_viewer_presence_ticks_session;
DROP INDEX IF EXISTS public.idx_exp_snapshots_session;
DROP INDEX IF EXISTS public.idx_exp_sessions_streamer;
DROP INDEX IF EXISTS public.idx_stream_ai_reports_streamer;
DROP INDEX IF EXISTS public.idx_stream_ai_reports_session_variant;
DROP INDEX IF EXISTS public.idx_stream_ai_reports_session;
DROP INDEX IF EXISTS public.idx_exp_transitions_streamer;
DROP INDEX IF EXISTS public.idx_eng_log_channel_ts;
DROP INDEX IF EXISTS public.idx_tgk_keywords;
DROP INDEX IF EXISTS public.idx_twitch_clips_social_media_streamer;
DROP INDEX IF EXISTS public.idx_mmu_ts;

-- idx_clip_fetch_history_streamer wird bewusst NICHT gedroppt. Er hat zwar
-- ebenfalls 0 Scans, aber aus einem behebbaren Grund: scout.rs filterte mit
-- LOWER(streamer_login), was den Index auf der rohen Spalte ausschliesst. Der
-- Query-Fix in dieser Aenderung macht ihn nutzbar.

-- Rollback (Definitionen live aus pg_indexes gezogen, Stand 2026-07-28):
--   CREATE INDEX idx_viewer_presence_ticks_session ON public.twitch_viewer_presence_ticks USING btree (session_id, viewer_login, tick_at);
--   CREATE INDEX idx_exp_snapshots_session ON public.exp_snapshots USING btree (exp_session_id);
--   CREATE INDEX idx_exp_sessions_streamer ON public.exp_sessions USING btree (streamer, started_at);
--   CREATE INDEX idx_stream_ai_reports_streamer ON public.twitch_stream_ai_reports USING btree (streamer_login, generated_at DESC);
--   CREATE INDEX idx_stream_ai_reports_session_variant ON public.twitch_stream_ai_reports USING btree (session_id, report_variant, generated_at DESC);
--   CREATE INDEX idx_stream_ai_reports_session ON public.twitch_stream_ai_reports USING btree (session_id);
--   CREATE INDEX idx_exp_transitions_streamer ON public.exp_game_transitions USING btree (streamer, ts_utc);
--   CREATE INDEX idx_eng_log_channel_ts ON public.twitch_engagement_log USING btree (channel_login, ts DESC);
--   CREATE INDEX idx_tgk_keywords ON public.title_generator_knowledge USING gin (keywords);
--   CREATE INDEX idx_twitch_clips_social_media_streamer ON public.twitch_clips_social_media USING btree (streamer_login, created_at);
--   CREATE INDEX idx_mmu_ts ON public.minimax_usage USING btree (ts);
