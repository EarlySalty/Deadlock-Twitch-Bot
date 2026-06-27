# B8 Social-Media + Highlight-Clipper Audit

Datum: 2026-06-27
Rolle: frischer Python->Rust-Paritäts-Verifizierer, read-only mit einer Ergebnisdatei

## Scope

Geprüft wurden:

- Python-Referenz: `bot/social_media/*`, insbesondere Upload-Worker, Clip-Fetch/Register/Queue, Enrichment/Transcription/LLM, Approval, Token-Refresh, Retention, Analytics/Reports, Dashboard/OAuth; plus `bot/highlight_clipper/*`.
- Rust-Ziel: `rust/crates/tb-social-media/*`, `rust/crates/tb-highlight/*`, Wiring in `rust/bin/tb-bot/src/main.rs`, Dashboard-Routing in `rust/crates/tb-dashboard-api/src/*`.
- Vorab-Kontext: `rust/docs/audit/2026-06-27/00-baseline.md` Abschnitt 2 und `findings/B9-infra-schema.md` (B9-024).

Bewertungsskala: P0 kritisch/blockierend, P1 Hauptfunktion bricht im Opt-in-Betrieb, P2 nutzer-/admin-sichtbare Funktion bricht oder ist nicht aktivierbar, P3 Paritätsnotiz/Caveat.

## Gesamturteil

Die vollständige Aussage "alles ist portiert und nur über Flags/Env/Daten gegated" ist widerlegt.

Portiert sind die Kernpfade für Clip-Fetch/Register, Upload-Queue, TikTok/YouTube-Posting, LLM-Dispatch, Retention, Analytics/Reports, `/social-media`-Dashboard/API, Website `/streamer`, und der Highlight-Clipper inklusive Demo-first-Analyse, API-Fallback, VOD-Schnitt und lokalem Relay.

Nicht nur gated, sondern problematisch:

- **B8-009 P1 bug:** `TokenRefreshWorker` ist implementiert, wird in `tb-bot` aber nicht gespawnt. Proaktiver Plattform-Token-Refresh ist damit tot.
- **B8-019 P2 regression:** Die Social-Admin-SPA (`/social-media-admin`) ist implementiert, aber der Live-Router bindet stattdessen einen Legacy-Redirect nach `/twitch/dashboard` ein. Das widerspricht der Baseline-Aussage zu P2.66.
- **B8-011 P2 missing/deliberate-drop:** Konkrete Transcription-Engines sind nicht portiert. Rust hat nur Trait/Mock und startet den Enrichment-Worker ohne Transcriber; laut Kommentar ist das B15-OFF/OpenAI-Whisper-Drop.
- **B8-008 P2 deliberate:** Instagram-Graph-Logik existiert nur für öffentliche Video-URLs. Der normale Auto-Upload-Pfad liefert lokale Dateien und scheitert dort wie Python, weil temporäres Hosting bewusst fehlt.

Zähl-Summary aus der Tabelle: **30 Funktionen geprüft: 21 parity, 6 deliberate, 1 regression, 1 bug, 1 missing.** Die eine `missing`-Klasse ist kein stiller Plattformadapter, sondern die konkrete Transcription-Engine hinter dem vorhandenen Trait.

## Aktivierungs-Gates

| Bereich/Plattform | Gate | Ergebnis |
|---|---|---|
| Social Core Worker | `tb-bot` startet Retention, Approval-Queue, Reports, Enrichment immer; Upload/Insights nur mit `DB_MASTER_KEY_V1`/FieldCipher (`main.rs:1185-1245`). | Parity/opt-in über Daten, aber Token-Refresh fehlt im Wiring. |
| Clip-Fetch/Clip-Trigger | `TB_CLIP_FETCHER_ENABLED=1` plus HelixClient (`main.rs:1360-1371`); Task selbst 6h/60s initial (`clip/task.rs:6-63`). | Deliberate-off, Implementierung vorhanden. |
| Highlight-Clipper | `TB_HIGHLIGHT_CLIPPER_ENABLED=1` plus HelixClient, `tools/boon`, `.venv/bin/yt-dlp`, `/usr/bin/ffmpeg`, lokaler Relay `127.0.0.1:8899` (`main.rs:1153-1183`, `tb-highlight/config.rs:4-31`). | Deliberate-off, Pfad vorhanden. |
| TikTok | `DB_MASTER_KEY_V1`, `social_media_platform_auth.enabled=1`, decryptbares Access-Token, `client_id`, pending Queue, Approval (`upload_worker.rs:45-55,123-130`). | Plattformlogik vorhanden. |
| YouTube | wie TikTok; optional `refresh_token+client_id+client_secret` für inline 401-Refresh (`upload_worker.rs:73-91`, `youtube.rs:9-13,84-119`). | Plattformlogik vorhanden; proaktiver Refresh fehlt wegen B8-009. |
| Instagram | `DB_MASTER_KEY_V1`, Access-Token, `platform_user_id`; zusätzlich öffentliche Video-URL nötig (`instagram.rs:1-8,97-108,125-134`). | Adapter vorhanden, normaler Auto-Upload mit lokaler Datei ist nicht sauber aktivierbar. |
| LLM | Default Ollama (`OLLAMA_HOST`, `OLLAMA_MODEL`); externe Provider per `SOCIAL_MEDIA_LLM_PROVIDER`, Provider-Keys und DB-Setting `external_llm_consent` (`llm_dispatch.rs:321-428`). | Parity; B9-Zentralisierung bleibt separates Infra-Thema. |
| Transcription | Kein aktivierbares Gate; `main.rs` injiziert keinen Transcriber, Rust enthält keine konkrete Whisper/OpenAI/Faster-Whisper-Engine (`main.rs:1201-1204`, `enrich_pipeline.rs:44-52`). | Missing/deliberate drop, nicht "nur off". |
| Social-Admin-SPA | Kein Feature-Flag; realer Handler existiert (`spa.rs:214-267`), Live-Router nutzt Stub (`lib.rs:1134-1151`). | Regression/toter Pfad. |
| Website | `/streamer` und `/website` im Router (`lib.rs:1194-1216`), Dist per `WEBSITE_DIST_PATH` (`website.rs:1-14,81-124`). | Parity. |

## Funktionsmatrix

| ID | Funktion | Python-Referenz | Rust-Pendant / Evidenz | Gate | Klasse | Severity | Befund |
|---|---|---|---|---|---|---|---|
| B8-001 | Social-Worker-Wiring | Python-Cogs/Worker starten dauerhaft. | `tb-bot` startet Retention, Approval, Reports, Enrichment; Upload/Insights bei FieldCipher (`main.rs:1185-1245`). | `DB_MASTER_KEY_V1` nur für tokenabhängige Worker. | parity | P3 | Kernloops sind vorhanden; siehe B8-009 für den fehlenden TokenRefreshWorker. |
| B8-002 | Twitch-Clip-Fetch/Clip-Trigger | `clip_fetcher.py` 6h-Loop über aktive Partner. | `ClipFetchTask` 6h/60s, `ClipFetchService` über aktive Partner; `tb-bot` nur mit `TB_CLIP_FETCHER_ENABLED=1` (`clip/task.rs:6-72`, `main.rs:1360-1371`). | `TB_CLIP_FETCHER_ENABLED=1`, HelixClient. | deliberate | P3 | Deliberate-off aus Baseline; nicht still fehlend. |
| B8-003 | Clip-Register/Manual Upload/Layout/Templates | `clip_manager.py` registriert Twitch- und manuelle Clips, Layout/Template-Daten. | `clip_manager.rs`, `clip_queue.rs`, Dashboard-Handlers für Upload/Layout/Templates (`social_media.rs:29-65,737-924`). | Dashboard-Auth/Scope, DB. | parity | P3 | Vollständige Gegenstücke vorhanden. |
| B8-004 | Upload-Queue, Scheduling, Retry/Stale-Reclaim | `queue_upload`, `get_upload_queue`, `update_upload_status`; `scheduled_at` wird gespeichert, aber nicht als Due-Filter genutzt. | `queue_upload` dedupliziert/requeued, `get_upload_queue` reclaimt stale und sortiert nach Priority/created_at (`clip_queue.rs:51-130,133-192`). | Pending Queue, Approval später im Worker. | parity | P3 | Rust spiegelt auch die Scheduling-Caveat: `scheduled_at` ist Persistenzfeld, kein Due-Gate. |
| B8-005 | Upload-Worker Orchestrierung | Download, 9:16-Konvertierung, Approval-Gate, paralleler Batch. | `UploadWorker` löst Uploader auf, Approval-Gate, max_parallel=2, stale scan, `yt-dlp` und VideoProcessor (`upload_worker.rs:1-8,45-100,123-174,322-362`). | `DB_MASTER_KEY_V1`, Plattformcredentials, Queue. | parity | P3 | Kernlogik portiert. |
| B8-006 | TikTok Posting/Status/Analytics | TikTok Content Posting API init/chunks/publish/status/analytics. | `TikTokUploader` init/chunk/publish/status/analytics (`tiktok.rs:1-173`). | Access-Token, client_id im Credential-Resolver. | parity | P3 | Logik vorhanden. |
| B8-007 | YouTube Shorts Posting/Status/Analytics | Google API resumable Upload, Token-Refresh durch Client-Lib. | Rust-Roh-HTTP resumable Upload, Status/Analytics, inline 401-Refresh bei Refresh-Creds (`youtube.rs:1-13,84-119,275-360`). | Access-Token/client_id; optional Refresh-Creds. | parity | P3 | Funktional portiert; proaktiver Refresh siehe B8-009. |
| B8-008 | Instagram Reels Posting | Python kann Graph-Container/Publish nur mit öffentlicher URL; lokaler Host-Upload TODO/NotImplemented. | Rust gleich: Graph API + `upload_to_temporary_host` NotImplemented; lokale Datei validiert zu Fehler (`instagram.rs:1-8,97-108,125-134`). | Access-Token, business/platform_user_id, öffentliche URL. | deliberate | P2 | Nicht still fehlend, aber kein sauberer Auto-Upload-Gate: der normale Queue-Pfad liefert lokale Dateien und bricht für Instagram. |
| B8-009 | Proaktiver OAuth Token-Refresh | `SocialMediaTokenRefreshWorker` startet im Init, 60s initial, dann 5min, Threshold 1h (`token_refresh_worker.py:47-85,87-197`). | `TokenRefreshWorker` existiert und testet `run_once`, Kommentar sagt "noch nicht verdrahtet" (`refresh_worker.rs:1-8,35-64`); `rg` findet keinen Spawn in `tb-bot`. | Müsste `DB_MASTER_KEY_V1` + OAuthManager brauchen; aktuell kein Gate, weil nicht gestartet. | bug | P1 | Dead path. Nach Token-Ablauf scheitern Plattform-Operationen; YouTube heilt nur einzelne 401 im Upload, persistiert aber keinen globalen proaktiven Refresh. |
| B8-010 | Enrichment Statusmaschine/Korrektur/Approval | `enrichment.py` pending->transcribing->correcting->llm->done/failed, Vocab-Korrektur, Approval. | `ClipEnrichmentPipeline::run` lädt Kontext, speichert Transcript/Korrektur/LLM und markiert Approval (`enrich_pipeline.rs:144-233`). | EnrichmentWorker immer gestartet; LLM-Gate separat. | parity | P3 | Orchestrator portiert. |
| B8-011 | Transcription Engines | Python unterstützt `faster_whisper`, `openai_api`, `none` per `SOCIAL_MEDIA_TRANSCRIBER`. | Rust hat nur `Transcriber`-Trait; `main.rs` startet ohne Transcriber und kommentiert B15-OFF/OpenAI-Whisper-Drop (`enrich_pipeline.rs:44-52`, `main.rs:1201-1204`). | Kein aktivierbares Gate/keine konkrete Engine. | missing | P2 | Nicht "nur deaktiviert": konkrete Engine-Implementierungen fehlen. Laut Kommentar deliberate drop, aber Parität zur Python-Referenz ist nicht vollständig. |
| B8-012 | LLM Dispatch/Fallback/Consent | Python: Default Ollama, MiniMax/Claude nur mit DB-Consent + Env Provider. | Ollama, MiniMax, Claude-Haiku direkt implementiert; Consent-Gate/Fallback-Chain vorhanden (`llm_dispatch.rs:30-110,140-291,321-428`). | `SOCIAL_MEDIA_LLM_PROVIDER`, Provider-Keys, `external_llm_consent`; Ollama default. | parity | P3 | Social-Verhalten portiert. B9-024 bleibt nur Zentralisierungsbefund. |
| B8-013 | Approval Queue/Auto-Approve | Python ApprovalService + Worker queue approved uploads. | ApprovalWorker queued approved clips; settings für auto-approve (`approval_worker.rs:1-8,32-49`, `settings.rs:12-15`). | DB-Approval-State, auto_approve_* settings. | parity | P3 | Queue-Seite portiert. |
| B8-014 | Approval-DMs | Python verschickt Approval-DMs. | Rust-Kommentar: `_dispatch_pending_dms` ist B10 Discord-DMs und nicht portiert (`approval_worker.rs:1-8`). | Kein Gate; bewusst ausgeschlossen. | deliberate | P3 | Deliberate omission, nicht still. |
| B8-015 | Retention | Python löscht abgelaufene, verworfene oder voll publizierte Clips. | RetentionWorker gleiche Bedingungen und File-Deletion (`retention_worker.rs:1-7,29-72`). | Immer gestartet, Datenzustand. | parity | P3 | Portiert. |
| B8-016 | Insights/Analytics Pull | Python Plattformanalytics/Reports. | InsightsWorker löst Plattformclients und schreibt Snapshots; ReportDispatcher generiert Reports (`insights_worker.rs`, `report_dispatcher.rs:1-9,43-70`). | `DB_MASTER_KEY_V1` für Insights; Reports ohne Cipher. | parity | P3 | Portiert; Report-DM siehe B8-017. |
| B8-017 | Admin Report DM Versand | Python kann Admin-Report per Discord-DM senden. | Rust persistiert Report, lässt DM als B10 weg (`report_dispatcher.rs:1-9,53-58`). | Kein Gate; bewusst ausgeschlossen. | deliberate | P3 | Deliberate omission, Dashboard-Report bleibt vorhanden. |
| B8-018 | `/social-media` Dashboard + APIs | Python registriert `/social-media`, APIs, OAuth, Admin Clips/Approval/Reports/Vocab/Templates. | Rust routes + handlers decken dieselben API-Gruppen ab (`lib.rs:106-112,133-175`, `social_media.rs:1-14,178-221,737-782,790-1006,1031-1169,1200-1398,1586-2215`). | DashboardAuthLevel/Scope, DB, FieldCipher für Credentials. | parity | P3 | Hauptdashboard portiert. |
| B8-019 | Social-Admin-SPA `/social-media-admin` | Baseline sagt P2.66 behoben. Python hatte dedizierten Admin-SPA-Einstieg. | Realer Handler existiert (`spa.rs:214-267`), aber `build_social_media_admin_router` bindet Stub-Redirect (`lib.rs:1134-1151`, `obsolete_routes.rs:42-48`) und wird in `build_router` gemerged (`lib.rs:1231-1240`). | Kein Flag; live Route zeigt auf Stub. | regression | P2 | Toter Pfad. Implementierung ist nicht erreichbar; P2.66 faktisch nicht behoben. |
| B8-020 | Öffentliche Website `/streamer` + Legacy `/website` | Python `api_overview.py` Website-Dist/Redirects. | `build_website_router` und `website.rs` implementieren statische Auslieferung, Redirects, Pfadvalidierung, `WEBSITE_DIST_PATH` (`lib.rs:1194-1216`, `website.rs:1-14,81-124`). | Öffentlich; optional `WEBSITE_DIST_PATH`. | parity | P3 | P2.67 bestätigt. |
| B8-021 | OAuth/Platform Status | Python Dashboard OAuth start/callback/disconnect/status. | Rust Social handlers importieren OAuth/CredentialManager und registrieren Callback öffentlich + Start/Disconnect/Status authed (`social_media.rs:54-56,1374-1398,2288-2385`, `lib.rs:106-112,167-168`). | Provider env/config, FieldCipher, DashboardAuth. | parity | P3 | Gegenstücke vorhanden. |
| B8-022 | Highlight Worker Activation/Wiring | Python Worker loop im Bot, 600s. | Rust `tb-bot` startet nur mit `TB_HIGHLIGHT_CLIPPER_ENABLED=1`; Loop schläft `POLL_INTERVAL_SECONDS` (`main.rs:1153-1183`). | Env-Flag, HelixClient, `tools/boon`, `.venv/bin/yt-dlp`. | deliberate | P3 | Deliberate-off, nicht fehlend. |
| B8-023 | Highlight Partner/Steam-ID Discovery | Python Postgres aktive Partner + Discord->Steam SQLite + manuelle `steamids.json`. | Rust `partners.rs` portiert Postgres-Query, read-only SQLite, manual override, Combine-Logik (`partners.rs:1-13,21-32,82-149`). | DB `twitch_streamers_partner_state`, SQLite-Datei, manual JSON. | parity | P3 | Portiert. |
| B8-024 | Highlight Match History/Metadata + API-Fallback | Python deadlock-api Client + `detect_events`. | Rust `deadlock_client.rs` + `event_detector.rs` portieren History, Metadata, Multikill/Teamfight/Close-Fight (`deadlock_client.rs:1-60`, `event_detector.rs:1-127`). | deadlock-api erreichbar. | parity | P3 | Portiert. |
| B8-025 | Highlight Demo Download + boon Analyse | Python lädt salts/demo bz2, entpackt, boon abilities/events/entities. | Rust `demo_downloader.rs`, `boon.rs`, `demo_analyzer.rs` portieren Download, Parser, Health/Combo/KillMoment (`demo_downloader.rs:1-99`, `boon.rs:1-265`, `demo_analyzer.rs:94-179`). | deadlock-api salts/demo URL, `tools/boon`. | parity | P3 | Portiert. |
| B8-026 | Highlight VOD-Suche, Clip-Fenster, Schnitt | Python `find_vod_for_match`, `download_clip`, Pre/Post/Max. | Rust `worker.rs` process_match + `twitch_vod.rs` VOD-Auswahl, yt-dlp/ffmpeg, Größe (`worker.rs:266-322`, `twitch_vod.rs:51-172`). | Helix archive videos, yt-dlp, ffmpeg. | parity | P3 | Portiert. |
| B8-027 | Highlight Sender/Relay | Python postet an lokalen highlight-clips Relay. | Rust `highlight_sender.rs` postet gleiche Payload best-effort (`highlight_sender.rs:1-82`). | Lokaler Relay erreichbar. | parity | P3 | Portiert; kein B10-DM, sondern Channel-Relay. |
| B8-028 | Highlight State/Idempotenz | Python `state.json`, processed_matches. | Rust `state.rs` defensives Load/Save/mark/is_processed (`state.rs:1-93`). | `data/highlight_clipper/state.json`. | parity | P3 | Portiert. |
| B8-029 | Highlight Config/Konstanten | Python `config.py`. | Rust `config.rs` 1:1 Werte (`config.rs:1-31`). | Keine. | parity | P3 | Portiert. |
| B8-030 | Unbenutzte Highlight-Helfer | Python importiert/enthält teilweise caller-lose Helfer. | Rust-Kommentare markieren bewusst nicht portiertes `_parse_kills`/ungenutzten Pfad; `analyze_match` ist vorhanden (`boon.rs:4-9`, `demo_analyzer.rs:121-143`). | Kein Runtime-Pfad. | deliberate | P3 | Kein Runtime-Missing gefunden; bewusst ausgelassene ungenutzte Helfer. |

## Regression / Missing / Bug Liste

- **P1 bug, B8-009:** `TokenRefreshWorker` ist ein toter Pfad. Fix wäre Wiring in `tb-bot` neben Upload/Insights mit FieldCipher + `OAuthManager`.
- **P2 regression, B8-019:** `/social-media-admin` ist live ein Redirect-Stub. Fix wäre Router auf `spa::social_media_admin_handler` und `spa::social_media_admin_assets_handler` mit `PgPool`/Auth-State statt `obsolete_routes`.
- **P2 missing, B8-011:** Transcription-Engines fehlen konkret. Wenn das als bewusst "DROP/OFF" bleibt, sollte das Cutover-Dokument es als nicht portierte Funktion führen; wenn aktivierbar gewünscht, braucht Rust eine echte `Transcriber`-Implementierung und ein Env/Data-Gate.

## Deliberate Nicht-Parität

- Clip-Fetch und Highlight-Erstellung sind default-off, aber vollständig genug hinter `TB_CLIP_FETCHER_ENABLED` bzw. `TB_HIGHLIGHT_CLIPPER_ENABLED`.
- Instagram Auto-Upload ist nicht sauber aktivierbar, weil öffentliches Video-Hosting fehlt. Das ist Python-Parität, aber funktional trotzdem ein P2-Caveat.
- Approval-DMs und Admin-Report-DMs sind B10/Discord-DM-Ausschluss, nicht still vergessen.
- OpenAI/Faster-Whisper-Transcription ist laut Rust-Kommentar bewusst entfernt; der Trait ist nur ein zukünftiger Anker.

## Rest-Risiken

- Diese Prüfung war statisch/line-level; es wurden keine Cargo-Tests oder Live-Uploads ausgeführt.
- Mehrere Rust-Dateikommentare sagen noch "noch nicht gespawnt", obwohl `tb-bot` einzelne Worker inzwischen startet (`upload_worker.rs`, `approval_worker.rs`, `retention_worker.rs`, `report_dispatcher.rs`). Für die Bewertung zählt das tatsächliche `tb-bot`-Wiring.
- Instagram bleibt auch bei korrekten Credentials ohne Public-URL-Provider faktisch unbrauchbar im normalen Queue-Pfad.
- Token-Refresh-Ausfall wird mit der Zeit schlimmer, nicht sofort beim Start sichtbar; deshalb P1 trotz vorhandener Upload-Adapter.
