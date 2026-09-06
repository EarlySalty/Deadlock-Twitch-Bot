---
status: aktiv
datum: 2026-09-06
thema: Bestandsaufnahme Clip-Aufbereitung Social Media, Belege fuer den Plan
methode: graphify/grep Nachlese im Repo + SELECT-only twitch_analytics
---

# EVIDENCE: Clips fuer Social Media aufbereiten

Jede Zeile eine echte Fundstelle. DB-Belege stammen aus SELECT-only auf
`twitch_analytics` (2026-09-06). earlysalty = Twitch-User-ID 1186925760.

## REQ-06 kalter Betrieb (Ursachenkette)

- `twitch_partners`: earlysalty aufgeloest auf `twitch_user_id = 1186925760`; `social_media_streamer_settings.approval_mode = full_auto` fuer earlysalty (SELECT).
- DB `social_media_category`: `deadlock` hat `enrichment_enabled=true`, `other` hat `enrichment_enabled=false` (SELECT).
- DB `twitch_clips_social_media`: ALLE 126 Clips liegen in `category_key='other'` mit `game_name` NULL; Kategorie `deadlock` hat 0 Clips (SELECT). earlysalty: 25 Clips, alle `other`/NULL.
- `rust/crates/tb-social-media/src/enrichment.rs:327-336`: `iter_pending_enrichments` verlangt `k.enrichment_enabled AND COALESCE(c.upload_local_path, c.local_file_path) IS NOT NULL` -> bei `other`-Clips 0 Treffer, deshalb 0 Enrichment-Zeilen.
- DB `social_media_clip_enrichment`: 0 Zeilen; `social_media_settings`: nur Key `admin_weekly_report_last_sent_period_end`, KEIN Key `external_llm_consent` (SELECT).
- `rust/crates/tb-social-media/src/settings.rs:110`: `external_llm_consent` liest genau diesen fehlenden Key; ohne `true` -> false.
- `rust/crates/tb-social-media/src/llm_dispatch.rs:155-166`: `ensure_consent` bricht den LLM-Aufruf ohne Consent ab (zweite Bremse, greift erst nach Fix von Kategorie/Download).
- `rust/crates/tb-social-media/src/approval.rs:235-247`: `iter_clips_ohne_enrichment` verlangt `NOT k.enrichment_enabled AND a.clip_db_id IS NULL AND COALESCE(c.status,'pending')='pending'`; sobald ein Clip auf `awaiting_approval` steht, faellt er hier raus.
- `rust/crates/tb-social-media/src/approval.rs:430-438`: `auto_approve_if_allowed` bricht ab, wenn `auto_platforms` leer ist -> full_auto ohne eingeplante Plattform bewirkt nichts; DB: earlysalty 24 `awaiting_approval`, 1 `approved`, alle mit `local_file_path` NULL.
- `rust/bin/tb-bot/src/main.rs:1635-1647`: der Upload-Worker (einziger Aufrufer von `download_clip`) wird nur bei `FieldCipher::from_env()` Ok gespawnt; fehlt DB_MASTER_KEY_V1, laufen nur die cipher-freien Worker -> selbst der 1 freigegebene Clip wird nie geladen. DB `twitch_clips_upload_queue`: 1 Zeile `pending`.
- `rust/crates/tb-social-media/src/upload_worker.rs` (do_upload/download_clip): Download passiert erst NACH dem Approval-Gate `is_clip_approved_for`; Download haengt damit an der Freigabe.
- `rust/bin/tb-bot/src/main.rs:1819`: `build_clip_fetch_task` hinter `TB_CLIP_FETCHER_ENABLED`; die Klassifizierung setzt `game_name`/`category_key` nicht (Ergebnis: alle Clips `other`/NULL, Schreibweg-Fehler).

## REQ-01 Layout wirkt im Render

- `rust/crates/tb-social-media/src/upload_worker.rs:563-582`: `convert_to_vertical` ruft `convert_and_trim(..., TARGET_WIDTH, TARGET_HEIGHT)`, plattformabhaengig, ohne Layout.
- `rust/crates/tb-social-media/src/video_processor.rs:272`: `convert_and_trim` ruft `convert_to_vertical(..., "center")` hart -> Center-Crop.
- `rust/crates/tb-social-media/src/video_processor.rs:178`: `compose_vertical(layout, mode, cam_enabled)` ist layout-bewusst gebaut, hat aber keinen Aufrufer im Live-Pfad.
- `rust/crates/tb-social-media/src/layout.rs:385-404`: `get_streamer_layout(pool, login)` laedt `StreamerLayout` aus `social_media_streamer_layout`; DB: earlysalty hat eine Zeile `mode=pip, cam=true`.
- `rust/crates/tb-dashboard-api/src/handlers/social_media.rs:54-56`: `get_clip_effective_layout` + `set_clip_layout_override` bereits importiert -> effektives Layout (Streamer + `layout_override_json`) existiert, wird im Render nicht benutzt; das ist die fehlende Verdrahtung.
- `rust/crates/tb-social-media/src/layout.rs:202-209`: `StreamerLayout { game_crop, cam_crop, cam_position, mode }`; Facecam-Rechteck-Semantik: `cam_crop` = Ausschnitt aus dem Twitch-Bild, `cam_position` = Zielrechteck im 1080x1920-Frame (Modul-Doku layout.rs:6-23).

## REQ-02 Blur-Rand-Variante

- `rust/crates/tb-social-media/src/video_processor.rs:47-93`: `build_compose_filter` baut nur pip/stacked mit `overlay` (`video_processor.rs:92`), kein `boxblur`; crateweit kein `boxblur`/`subtitles`/`drawtext`.
- `rust/crates/tb-social-media/src/layout.rs:23`: Modus `mode ∈ {pip, stacked}`; `StreamerLayout.mode` ist String (layout.rs:209) -> dritte Variante `blur_pad` ergaenzbar.
- `bot/dashboard_v2/src/pages/SocialMedia.tsx:34,167,258-259,583`: `LayoutEditor` mit `mode`-State (Default `pip`); dritte Option gehoert hier hinein.

## REQ-03 Untertitel per STT

- `ops/stt-server/README.md`: STT-Server auf `127.0.0.1:8791`, `POST /v1/audio/transcriptions`, OpenAI-kompatibel, `verbose_json`, keine Auth, loopback-only.
- `rust/crates/tb-engagement/src/transcribe.rs:27,41-61,249-250`: `OpenAiTranscriber` zeigt per Default auf `http://127.0.0.1:8791/v1/audio/transcriptions`, fordert `response_format=verbose_json` + `timestamp_granularities[]=segment`, liefert `TranscriptionResult { text, duration_seconds, segments: Vec<TranscriptSegment { start_seconds, end_seconds, text }> }` -> wiederverwendbarer Client, Wort/Segment-Zeitstempel vorhanden.
- `rust/crates/tb-social-media/src/enrich_pipeline.rs`: `Transcriber`-Trait injizierbar, im Worker `transcriber=None` -> Stage uebersprungen; `correction.rs` laeuft dadurch leer.
- `rust/crates/tb-social-media/src/correction.rs:1`: Fuzzy-Korrektur eines Transkripts gegen das Deadlock-Vokabular -> an den STT-Ausgang zu haengen.
- DB `social_media_streamer_settings` Spalten: `streamer_login, approval_mode, timezone, updated_at, updated_by` -> KEIN Untertitel-Schalter; Migration noetig (neue Spalte `subtitles_enabled`, Default true).

## REQ-04 Vorschau-Render + Auslieferung

- `rust/crates/tb-dashboard-api/src/lib.rs:252-258`: `streamer-layout` GET/PUT und `clips/:clip_db_id/layout` PUT vorhanden; kein `preview`/`render`-Endpoint.
- `rust/crates/tb-dashboard-api/src/handlers/social_media.rs:765-804`: mp4-MIME-Erkennung existiert (Upload-Annahme), aber kein Datei-Serving fuer eine gerenderte Vorschau.
- `bot/dashboard_v2/src/pages/SocialMedia.tsx:212-222,1889-1917`: Vorschau im Editor nutzt `clip.thumbnail_url` (Standbild), kein gerendertes Video.
- `/etc/caddy/Caddyfile:616`: `@public_twitch` enthaelt `/social-media/api/*` -> neue `/social-media/api/...`-Subpfade sind ohne Caddy-Aenderung erreichbar; `:207` `@dashboard_paths` enthaelt `/social-media` (CSP) -> kein Caddy-Change noetig, solange die Vorschau unter `/social-media/api/` liegt.
- `rust/crates/tb-social-media/src/retention.rs:151-181`: `iter_expired_clips_for_retention` + `delete_clips_by_ids` raeumen ueber `retention_until`, `upload_local_path`, `local_file_path` -> die Vorschau-Datei muss an einem dieser Pfade/Ordner haengen, damit Retention sie loescht (INV-07).

## REQ-05 Batch-Kommando

- `rust/crates/tb-social-media/`: kein `[[bin]]` in `Cargo.toml`, kein `src/bin/` -> Batch als neues Bin dort anlegbar.
- `rust/bin/tb-bot/src/main.rs:552`: `async fn main()` ohne clap/Subcommand-Infrastruktur (env-flag-gesteuert) -> ein Subcommand im Riesen-Binary waere schwer; eigenes Bin in tb-social-media ist der saubere Weg.

## REQ-07 Doku

- `docs/funktionsweise/social-media-uploads.md:10,13`: verspricht Ton-Transkription und einblendbare Untertitel; beides bis REQ-03 nicht im Code.
- `docs/architecture/social-media.md` Punkte 2,3 (~51-52) und Abschnitt 6: nennt Whisper und Claude-Haiku/MiniMax/Ollama; Bahnen sind entfernt (`llm_dispatch.rs:1-14`, nur tb-llm/Fireworks).
- `rust/crates/tb-social-media/src/enrichment_worker.rs:80-81`: veralteter Kommentar "Noch nicht in tb-bot gespawnt", obwohl `main.rs:1627` ihn spawnt.

## Test-Infrastruktur

- `AGENTS.md`/Memory: tb-social-media-Tests mit `SQLX_OFFLINE=1 TB_TEST_DATABASE_URL=postgres:///tb_bb_test?host=/var/run/postgresql`; tb-db-Migrationstests gegen Docker-Container `tb-test-postgres` ueber `rust/scripts/test_db.sh`. Cargo immer `/home/nathanael/.cargo/bin/cargo`.
- `rust/crates/tb-social-media/src/video_processor.rs:301-393`: bestehende Filter-Tests fuer `build_compose_filter` (pip/stacked) -> Andockpunkt fuer den Blur-Rot-Test.
