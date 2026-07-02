# Workflow

## 2026-07-02 — Fix security.rs Test-Schema-Isolation

- Start: delegierter GPT-Implementierungsworker fuer gezielten `security.rs`-Test-Harness-Fix im laufenden Merge-Worktree; kein Commit/Push/Stash/Checkout/Reset, kein repo-weites `cargo fmt`.
- Implementiert: `auth::security::integration_tests::maybe_pool` nutzt jetzt wie `auth_login`/`auth_session` pro Test-Pool `test_schema_name(...)`, erstellt das Schema ueber Admin-Pool und verbindet den Test-Pool mit `search_path` auf dieses Schema.
- Verifikation: Timescale-Container `tb-secfix` auf `127.0.0.1:55041`, `timescaledb`-Extension vorhanden; `TB_TEST_DATABASE_URL=... TB_TEST_REQUIRE_DB=1 SQLX_OFFLINE=true cargo test -p tb-dashboard-api --no-fail-fast` dreimal hintereinander gruen mit je 703 passed / 0 failed / 1 ignored plus Doc-tests 0 failed.

## 2026-07-02 — Merge parity into main sqlx-Konflikte

- Start: delegierter GPT-Implementierungsworker fuer Merge `fix/py-rust-parity-obvious-bugs` nach `merge/parity-into-main` im isolierten Worktree `/home/naniadm/.worktrees/twitch-merge`; Ausgangs-HEAD `1a81da7d`, Feature `98cfaf14`, Arbeitsbaum vor Merge sauber. Auftrag enthaelt widerspruechliche Commit-Regeln; ich loese/verifiziere und lasse den Merge uncommitted.
- Merge gestartet mit `git merge --no-ff fix/py-rust-parity-obvious-bugs`; erwartete 10 Konflikte erhalten. Konfliktmarker in `CHANGELOG.md`, `tb-analytics` und `tb-dashboard-api` entfernt; Branch-Parity-Logik mit main-`sqlx`-Makroform zusammengefuehrt. Zusaetzlich `viewer_exclusion.rs` auf `query_scalar!` nachgezogen, weil die Branch-Logik den vorher lokalen Helper ausgelagert hat.
- sqlx-Prepare: frischer Timescale-Container `tb-merge` auf `127.0.0.1:55021`, Extension + Migrationen erfolgreich, `DATABASE_URL=... cargo sqlx prepare --workspace` gruen; Cache bereinigt 21 stale `rust/.sqlx/query-*.json`-Dateien.
- Verifikation final: `SQLX_OFFLINE=true cargo build --workspace` gruen; `SQLX_OFFLINE=true cargo clippy -p tb-dashboard-api -p tb-analytics --all-targets` exit 0 mit bestehenden Warnungen. Finaler Vier-Crate-Test gegen frisch neu gestarteten/migrierten `tb-merge`: `tb-analytics` 363/0, `tb-dashboard-api` 703/0 (1 ignored), `tb-internal-api` 283/0, `tb-social-media` 107/0; Doc-tests ebenfalls 0 failed.
- Kein Merge-Commit erstellt wegen widerspruechlicher Auftragsspezifikation (Task verlangt Commit, verbindliche Regeln verbieten Commit/Push und verlangen uncommitted Working Tree). Merge bleibt resolved/staged fuer Review.

## 2026-07-02 — Merge-Prep auth_login + tb-social-media

- Start: delegierter GPT-Implementierungsworker fuer Merge-Prep `auth_login`-Concurrent-DDL-Flake plus 14 `tb-social-media`-Lib-Fails; vorhandene uncommitted Merge-Prep-Fixes bleiben unangetastet, kein Commit/Push/Stash/Checkout/Reset, kein repo-weites `cargo fmt`.
- Teil 1 Recon/Implementierung: Dashboard-DB-Tests nutzen pro Test ein eigenes Schema mit `search_path` auf dem Pool; `handlers/auth_login.rs` hing noch direkt an der Basis-DB. `maybe_pool` dort auf schema-isolierten Pool nach bestehendem Muster umgestellt, damit `CREATE TABLE ... BIGSERIAL` nicht parallel im Shared-Schema raced.
- Setup: Timescale-Container `tb-mprep` auf `127.0.0.1:55499`, `CREATE EXTENSION timescaledb` ausgefuehrt; weitere Tests mit `TB_TEST_DATABASE_URL=postgres://postgres:tbtest@127.0.0.1:55499/postgres`, `TB_TEST_REQUIRE_DB=1`, `SQLX_OFFLINE=true`.
- Teil 2 Diagnose/Fix: 14 `tb-social-media`-Fails einzeln mit `--nocapture` reproduziert. Alle als stale Test-Fixtures kategorisiert: Clip-/Queue-/Template-/Analytics-Fixtures lagen noch auf `SERIAL`/`INTEGER` bzw. teils `TEXT`/nullable, waehrend `fresh_schema_snapshot.txt` fuer die betroffenen Prod-Spalten `BIGINT`, `TIMESTAMPTZ`, `BOOLEAN` und nicht-nullbare Clip-Felder vorgibt; `social_media_clip_approval.clip_db_id` bleibt bewusst `INTEGER` gemaess Snapshot.
- Teil 2 Verifikation bisher: die 14 Einzelfaelle laufen gruen; `TB_TEST_DATABASE_URL=... TB_TEST_REQUIRE_DB=1 SQLX_OFFLINE=true cargo test -p tb-social-media --lib --no-fail-fast` gruen mit 107 passed / 0 failed.
- Zusatz-Merge-Prep-Fix: bekannter Dashboard-Rate-Limit-Middleware-Test verwendete einen festen Shared-Bucket und konnte nach roten/parallel laufenden Tests aktive Hit-Rows hinterlassen; Test nutzt jetzt einen zufaelligen XFF-Bucket und raeumt seinen Prefix auf. Das war noetig, damit der beauftragte Dreier-Gesamtlauf reproduzierbar 0 failed erreicht.
- Final verifiziert: `cargo build -p tb-dashboard-api -p tb-social-media` gruen; `cargo clippy -p tb-dashboard-api -p tb-social-media --all-targets` exit 0 mit bestehenden Warnungen; Zieltest `handlers::auth_login::tests::callback*` 8 passed / 0 failed.
- Finaler Dreier-Gesamtlauf zweimal gruen: `cargo test -p tb-dashboard-api -p tb-internal-api -p tb-social-media --no-fail-fast` exit 0 in zwei aufeinanderfolgenden Laeufen. Kompakte Summaries: `tb-dashboard-api` 703 passed / 0 failed / 1 ignored, `tb-internal-api` 283 passed / 0 failed, `tb-social-media` 107 passed / 0 failed; Doc-tests ok.
- Abschluss: Report geschrieben und per `jq empty` validiert: `/home/naniadm/.claude/projects/-home-naniadm-Claude-Native-Workspace/5b3905c3-f094-4325-b670-f7f72c2d4352/scratchpad/triage/fix/mergeprep2_report.json`.

## 2026-07-02 — Fix C social_media BIGINT/BOOL Decode

- Start: delegierter GPT-Implementierungsworker fuer echten Decode-Bug in `tb-dashboard-api` Social-Media-Handler; Scope Prod-Code plus betroffener Test/Report, kein Commit/Push/Stash/Checkout/Reset, kein repo-weites `cargo fmt`.
- Recon: frischer Snapshot bestaetigt `twitch_clips_social_media.id` als BIGINT und `uploaded_tiktok/uploaded_youtube/uploaded_instagram` als BOOLEAN; Python-Referenz serialisiert `clip_db_id` direkt als int und `platform_status` per `bool(row.get(...))`.
- Recon: Dashboard-Handler dekodiert `id`/Upload-Flags aktuell als `i32`/`Option<i32>` und maskiert `try_get`-Fehler; weitere Social-Media-Helfer nutzen teils `i32` wegen `social_media_clip_approval`/`social_media_clip_enrichment.clip_db_id` weiterhin INTEGER.
- Implementiert bisher: `normalize_id`, Clip-Ownership/Existenz, Admin-Clip-Row und manuelles Upload-Markieren auf `i64`; Clip-Row-Decode auf `Result` ohne `unwrap_or`-Masking; Upload-Flags als `Option<bool>`; `created_at` im Clip-SELECT explizit `::text`; Admin-Clip-Handler propagieren Query-/Decode-Fehler als 500 mit `tracing::error!`.
- Test nachgeschaerft: `admin_clips_list_detail_discard` prueft echte `clip_db_id != 0`, Detail-ID und boolsche `platform_status`-Flags.
- Verifikation: Timescale-Container `tb-cfix` auf `127.0.0.1:55497`; `cargo build -p tb-dashboard-api` gruen; `cargo clippy -p tb-dashboard-api --all-targets` exit 0 mit bestehenden Warnungen; `cargo test -p tb-dashboard-api --no-fail-fast` gruen: 703 passed / 0 failed / 1 ignored, Doc-tests 0/0/2 ignored; Zieltest `handlers::social_media::tests::admin_clips_list_detail_discard` gruen.
- Abschluss: Report geschrieben und per `jq empty` validiert: `/home/naniadm/.claude/projects/-home-naniadm-Claude-Native-Workspace/5b3905c3-f094-4325-b670-f7f72c2d4352/scratchpad/triage/fix/cfix_report.json`; `git diff --check` gruen; Container `tb-cfix` entfernt.

## 2026-07-02 — DB19 Diagnose Merge-Prep

- Start: delegierter GPT-Implementierungsworker fuer Diagnose der 19 bekannten DB-Env-Lib-Fails in `tb-internal-api` und `tb-dashboard-api`; kein Commit/Push/Stash/Checkout/Reset, kein repo-weites `cargo fmt`, keine Prod-Migrations-Edits.
- Setup: Wegwerf-Timescale-Container `tb-diag` auf `127.0.0.1:55496`, `TB_TEST_REQUIRE_DB=1`, `SQLX_OFFLINE=true`; Einzeltest-Diagnose mit `-- --nocapture` laeuft.
- Zwischenstand: 18/19 Zieltests durch reine Test-DDL-/Seed-/Assertion-Fixes gruen; verbleibend `handlers::social_media::tests::admin_clips_list_detail_discard` als C-Befund, weil frisches Schema `twitch_clips_social_media.id` BIGINT und `uploaded_*` BOOLEAN fuehrt, Handler aber `id`/Upload-Flags als `i32` liest.
- Verifikation: Einzeltest-Recheck 18 gruen, 1 C rot. Gesamt `cargo test -p tb-internal-api -p tb-dashboard-api --no-fail-fast -- --nocapture`: `tb-internal-api` 283/0 gruen; `tb-dashboard-api` 701 passed / 2 failed / 1 ignored, davon bekannter Timing-Flake `auth::security::integration_tests::rate_limit_middleware_429_nach_limit` plus C-Befund `admin_clips_list_detail_discard`.
- Abschluss: Report geschrieben und per `jq empty` validiert: `/home/naniadm/.claude/projects/-home-naniadm-Claude-Native-Workspace/5b3905c3-f094-4325-b670-f7f72c2d4352/scratchpad/triage/critic/db19_diag.json`; `git diff --check` gruen; Container `tb-diag` entfernt.

## 2026-07-02 — W10 Rework Auth-Split + Announcement-Redaction

- Start: delegierter GPT-Implementierungsworker fuer W10-Rework; Scope Admin-Auth 401/403-Split und Announcement-Detail-Redaktion; kein Commit/Push/Stash/Checkout/Reset, kein repo-weites `cargo fmt`.
- Recon bisher: Python `api_admin.py` nutzt fuer die gelisteten Admin-API-Routen durchgehend `_admin_auth_error(... _require_v2_admin_api ...)`; Rust-Handler in Admin-/System-Dateien haengen noch am gebrueckten `AuthLevel` und verlieren Partner-vs-None.
- Implementiert bisher: zentraler `require_admin(DashboardAuthLevel)` mit 401 `auth_required` fuer None und 403 `admin_required` fuer Partner; Admin-/System-Handler auf `DashboardAuthLevel` umgestellt; AnnouncementOutcome redigiert tokenartige Detail-Bodies vor Trim/Kappung.
- Verifikation bisher: `rust/target` war voll und wurde per `cargo clean` als Build-Artefakt bereinigt; danach `SQLX_OFFLINE=true cargo check -p tb-http-core -p tb-transport-twitch -p tb-dashboard-api -p tb-internal-api` gruen.
- Abschluss: `ApiError::unauthorized_with_body`, `auth_required_error`/`require_admin`, Admin-Handler-Umstellung und Announcement-Detail-Redactor implementiert; `admin_streamers`-Mutationshandler haben keinen direkten `api_admin.py`-Gegenpart, bleiben aber Admin-only mit demselben Split.
- Verifikation final: Timescale `tb-w10rw` auf `127.0.0.1:55495`; `cargo build` und `cargo clippy --all-targets` fuer `tb-http-core`, `tb-transport-twitch`, `tb-dashboard-api`, `tb-internal-api` gruen. Fokustests gruen: `unauth_auth_required_401` 6/0, `partner_admin_required_403` 1/0, `announcement_detail_redigiert_tokenartige_werte` 1/0.
- Gesamt-Tests: `tb-http-core` 15/0, `tb-transport-twitch` 77/0, `tb-dashboard-api` 688/15 (bekannte DB-Fixture-Fails), `tb-internal-api` 279/4 (bekannte DB-Fixture-Fails). Report geschrieben und validiert: `/home/naniadm/.claude/projects/-home-naniadm-Claude-Native-Workspace/5b3905c3-f094-4325-b670-f7f72c2d4352/scratchpad/triage/fix/wave10_rework_report.json`.

## 2026-07-02 — Wave10 adversarial critic

- Start: READ-ONLY adversariale Pruefung der uncommitteten W10-Aenderungen auf Branch `fix/py-rust-parity-obvious-bugs`; Fokus Admin-Auth/CSRF-Entscheidungen, Actor-Threading, Announcement-Details, Title-Ledger und `/twitch/market`; keine Code-Fixes, kein Commit/Push/Stash/Checkout/Reset.
- Recon laeuft: W10-Diff gegen HEAD, Python-Referenz `bot/analytics/api_admin.py` und betroffene Rust-Handler werden zeilenbasiert verglichen; Reportziel `scratchpad/triage/critic/wave10_crit.json`.
- Befund bisher: W10 dreht echte `AuthLevel::None`-Admin-API-Faelle pauschal auf 403 `admin_required`, obwohl Python `_require_v2_admin_api` fuer `auth_level == "none"` 401 `auth_required` liefert; Partner/Admin-Entscheidung selbst bleibt nicht aufgeweicht.
- Befund bisher: Announcement-Detailpfad uebernimmt rohen Twitch-HTTP-Body in `AnnouncementOutcome.detail`; das Detail wird spaeter als interne API-JSON-`detail` weitergereicht bzw. in Dashboard-Bridge-Logs verwendet, ohne Secret-Redaktion.
- Verifikation bisher: Timescale-Container `tb-w10crit` auf `127.0.0.1:55494`; gezielte Tests fuer Admin-CSRF/Auth-Wiring, Market-HTML, Title-Ledger-Fail-Best-Effort, Announcement-Detail und Admin-Actor-Fallback laufen gruen.
- Abschluss: Report geschrieben und per `jq empty` validiert: `/home/naniadm/.claude/projects/-home-naniadm-Claude-Native-Workspace/5b3905c3-f094-4325-b670-f7f72c2d4352/scratchpad/triage/critic/wave10_crit.json`; Container `tb-w10crit` entfernt.

## 2026-07-02 — Wave10 misc parity fixes

- Start: delegierter GPT-Implementierungsworker fuer W10 misc parity fixes; Scope nur belegte Python/Rust-Paritaetsluecken, kein Commit/Push/Stash/Checkout, kein repo-weites `cargo fmt`.
- Recon laeuft: Python-Referenzen und aktuelle Rust-Luecken fuer Title-LLM-Ledger, stale Title-Test/Kommentar, ChatApi-Announcement-Details, Admin-Promo-Actor, `/twitch/market` und Admin-Auth/CSRF-Error-Shapes werden vor Code-Aenderungen belegt.
- Implementiert bisher: Title-MiniMax-Usage wird ins gemeinsame Ledger geschrieben; stale `!title`-Nicht-portiert-Kommentare/Test entfernt; Announcement-Ergebnisvertrag additiv um Status-/Detaildaten erweitert; Admin-Promo/Announcements schreiben echten Discord-Actor aus der Admin-Session; Admin-Auth/CSRF-Shapes auf Python-Body umgestellt; `/twitch/market` rendert die vorhandene Market-Data-Payload mit Platzhalter-Labels.
- Verifikation: Timescale-Container `tb-w10` auf `127.0.0.1:55492`; `cargo build -p tb-http-core -p tb-transport-twitch -p tb-chat -p tb-internal-api -p tb-dashboard-api -p tb-bot` gruen; `cargo clippy --all-targets` fuer dieselben Crates gruen. `cargo test --no-fail-fast` fuer dieselben Crates bleibt rot in bestehenden umfangsfremden Lib-Tests von `tb-dashboard-api` (686 passed / 16 failed / 1 ignored) und `tb-internal-api` (279 passed / 4 failed); `tb-http-core`, `tb-transport-twitch`, `tb-chat`, `tb-bot` und alle W10-Fokustests gruen. Kein stash/HEAD-Vergleich wegen ausdruecklichem No-Stash/No-Checkout-Auftrag. Report: `scratchpad/triage/fix/wave10_report.json`.

## 2026-07-02 — W9 Ownership-Marker Concurrency Fix

- Start: delegierter GPT-Implementierungsworker fuer `Fix W9 ownership-marker concurrency`; Scope Code nur `rust/crates/tb-db/src/migrate.rs`, kein Commit/Push/Stash/Checkout, kein cargo fmt.
- Implementiert: `ensure_schema_owner_marker` in `tb-db::migrate` ohne Runtime-`CREATE TABLE` und ohne per-Startup-`UPDATE`; nur idempotentes `INSERT ... ON CONFLICT DO NOTHING` plus Owner-/Versionspruefung gegen `public.tb_schema_ownership`.
- Verifikation: Timescale-Postgres `w9fix-pg` auf `127.0.0.1:55490`; `cargo build -p tb-db` gruen; `cargo clippy --all-targets -p tb-db` gruen; paralleles `cargo test -p tb-db --no-fail-fast` gruen mit 17 passed / 0 failed; Container entfernt.

## 2026-07-02 — Wave9 Startup/Schema/Config-Ops

- Start: delegierter GPT-Implementierungsworker fuer Wave9 Startup/Schema/Config-Ops; Scope Rust unter `rust/`, Python nur lesend, kein Commit/Push/Stash/Checkout, kein repo-weites `cargo fmt`.
- Recon belegt: Rust-Binaries liefen bei Migrationsfehlern weiter; `tb-config` war bei ungueltigen optionalen Zahlen fatal; Python-Referenz degradiert optionale numerische Env-Werte mit Default/Clamp; Dashboard-Python erzwingt Role/Port plus PID-Lock; EventSub-Capacity-Retention nutzt 45 Tage, Clamp 7..365.
- Implementiert: Migrationsfehler in `tb-bot`/`tb-dashboard` sind fail-fast; Rust-Schema-Ownership-Marker als Migration + `tb-db`-Pruefung; optionale Config-/Retry-/Startup-Env-Parser warnen und defaulten/clampen; Dashboard Role-/Port-Guard + PID-Lock; zentraler `tb-bot`-TaskSupervisor fuer zentrale Dauerlaeufer; Observability-Event-Retention in `tb-monitoring` und stuendlicher Cleanup im bestehenden Retention-Loop.
- STOR-SCHEMA-005 untersucht: mehrere destruktive/lockende Migrationen identifiziert (`DROP COLUMN`, Constraint-Rebuilds, Hypertable `migrate_data => TRUE`, Typ-ALTERs); kein sauberer Einzelfix ohne Migrationsstrategie, als offener Punkt reportet.
- Verifikation: `wave9-pg` auf `127.0.0.1:55463` mit Timescale Community im vorgegebenen `postgres:16`-Container; `cargo build`, `cargo clippy --all-targets` und `cargo test --no-fail-fast` fuer `tb-config`, `tb-db`, `tb-monitoring`, `tb-bot`, `tb-dashboard` gruen. Regressionen gruen: Migrationsfehler stoppt Startup mit Exit 1; ungueltige optionale Env warnt/defaultet und startet weiter; Dashboard Role-Guard/PID-Lock blockieren falsche Rolle/Doppelstart; Observability-Retention entfernt alte Zeilen.

## 2026-07-02 — Wave8 internal-api + raid/requirements

- Start: delegierter GPT-Implementierungsworker fuer Wave8 `tb-internal-api`, `tb-dashboard-api`, `tb-raid`, `tb-bot` Route-Wiring; Scope Rust unter `rust/` plus Workflow/Report, Python nur lesend, kein Commit/Push/Stash/Checkout, kein repo-weites `cargo fmt`.
- Recon laeuft: Python-Referenz fuer `raid/requirements`, Rust-interne Raid-Routen, manuelle Raid-Route und diagnose/scam-guard-Fehlerformen werden belegt, bevor Fixes umgesetzt werden.
- Implementiert bisher: `POST /raid/requirements` im internen Router registriert und an den Idempotency-Layer gehaengt; tb-bot sendet Requirements-DM ueber Broker mit persistentem Dedupe-Marker pro `twitch_user_id`/Zweck; `raid/manual` validiert Auth/Input fail-closed vor Port-Aufruf; Scam-Guard-Fehlerformen in internal/dashboard auf `{error,message}` mit snake_case-Codes umgestellt.
- Verifikation: Wegwerf-Postgres `wave8-pg` auf `127.0.0.1:55460`; `cargo build -p tb-internal-api -p tb-dashboard-api -p tb-raid -p tb-bot` gruen; `cargo clippy --all-targets -p tb-internal-api -p tb-dashboard-api -p tb-raid -p tb-bot` exit 0 mit bestehenden Warnungen ausserhalb der Wave8-Aenderungen; `git diff --check` gruen.
- Fokussierte Regressionen gruen: `tb-bot requirements_dm_wird_persistent_deduped`, `tb-internal-api raid::tests`, `tb-internal-api requirements`, `tb-internal-api scam_guard::tests`, `tb-dashboard-api scam_guard`.
- Gesamt-Testlauf laut Auftrag bleibt rot: `cargo test -p tb-internal-api -p tb-dashboard-api -p tb-raid -p tb-bot --no-fail-fast` exit 101; rot sind bestehende/umfangsfremde Lib-Tests in `tb-dashboard-api --lib` (15) und `tb-internal-api --lib` (4), waehrend `tb-bot`, `tb-raid` und Wave8-Fokustests gruen sind. Report: `/home/naniadm/.claude/projects/-home-naniadm-Claude-Native-Workspace/5b3905c3-f094-4325-b670-f7f72c2d4352/scratchpad/triage/fix/wave8_report.json`.

## 2026-07-02 — Wave7 Monitoring Inbox/Poll

- Start: delegierter GPT-Implementierungsworker fuer Wave7 `tb-monitoring` Inbox/Poll + `tb-bot` Wiring; Scope Python nur lesen, Rust minimal, kein Commit/Push/Stash/Checkout und kein repo-weites `cargo fmt`.
- Recon bisher: Inbox-Requeue-Wakeup in Rust bestaetigt fehlend (Python weckt Runtime nach Requeue); Python-Inbox kennt zusaetzlich `stream.online.followups`; Dashboard-EventSub-Bridge-Outbox ist in Rust architektonisch durch nativen WebhookReceiver/EventSubDispatcher/InboxRuntime ersetzt; Poll-ScoreRefresh ist in `SubscriptionPollHooks::after_tick` bereits verdrahtet; ReAuth-Dedupe braucht aktuellen Stream-Kontext statt stale/fehlendem `stream_id`.
- Implementiert: Requeue weckt aktive Inbox-Runtimes; `stream.online.followups` wird als Inbox-Worktype verarbeitet; EventSub-/Poll-Go-Live-Hooks transportieren optional den aktuellen `stream_id`; ReAuth-Reminder dedupt mit aktuellem Stream-Kontext und Poller nutzt den Tick-Stream vor dem DB-Fallback.
- Verifikation: Wegwerf-Postgres `wave7-pg` auf `127.0.0.1:55458`; gezielte Regressionstests fuer Requeue-Wakeup, Followup-Worktype und ReAuth-Stream-Dedupe gruen; `SQLX_OFFLINE=true cargo build -p tb-monitoring -p tb-bot` gruen; `cargo clippy --all-targets -p tb-monitoring -p tb-bot` exit 0 mit bestehenden Warnungen ausserhalb der Wave7-Aenderung; `cargo test -p tb-monitoring -p tb-bot --no-fail-fast` gruen mit 287 passed / 0 failed. Report geschrieben nach `scratchpad/triage/fix/wave7_report.json`.

## 2026-07-01 — Reauth Opt-out AuthWriter Fix

- Start: delegierter GPT-Implementierungsworker fuer `Fix RAID-REAUTH-OPTOUT auth_writer`; Diagnose `reauth_optout_diag.json` vollstaendig gelesen. Scope minimal auf `tb-raid` AuthWriter/Test plus Workflow/Report, kein Commit/Push/Stash, kein cargo fmt.
- Implementiert: `AuthWriter::store_new_auth` heilt jetzt `token_error*`-Pausen, setzt dabei nur fuer diese technischen Pausegruende `manual_partner_opt_out=0`, und haelt die allgemeine `raid_bot_enabled`-Aktivierung an `activate_raid_features`, `manual_partner_opt_out=0` und nicht-`blocked`/`bot_banned` gebunden.
- Tests ergaenzt/angepasst: `auth_writer`-Fixture um `manual_partner_opt_out`; Regression fuer echte manuelle Opt-outs ohne token_error-Pause sowie `blocked`/`bot_banned`; bestehender Suffix-Test auf gewollte `token_error_expired`-Heilung angepasst.
- Verifikation: Wegwerf-Postgres `reauthfix-pg` auf `127.0.0.1:55456`; roter Callback-Test gruen; `cargo test -p tb-raid --test auth_writer` gruen; `cargo test -p tb-raid -p tb-bot --no-fail-fast` gruen mit 476 passed / 0 failed; `SQLX_OFFLINE=true cargo build -p tb-raid -p tb-bot` gruen; `SQLX_OFFLINE=true cargo clippy --all-targets -p tb-raid -p tb-bot` exit 0 mit bestehenden Warnungen ausserhalb der geaenderten AuthWriter-Stellen; `git diff --check` gruen. Report geschrieben nach `scratchpad/triage/fix/reauth_optout_fix_report.json`; Container entfernt.

## 2026-07-01 — Reauth Opt-out Diagnose

- Start: READ-ONLY Diagnose fuer roten tb-bot-Test `raid_oauth_impl::callback_tests::reauth_ohne_discord_state_fuehrt_partner_sync_aus`; Scope nur temp-Worktrees, Wegwerf-Postgres, Workflow/Report. Keine Source-Edits, kein Commit/Push, kein cargo fmt, Haupt-Worktree-HEAD bleibt unveraendert.
- Ergebnis: Branch-Base `0ca35c30` ist bereits rot; HEAD `89248195` reproduziert `(Some(1), Some(1), None)`. Relevante OAuth/AuthWriter/PartnerSetup-Dateien sind zwischen Base und HEAD unveraendert; nur `token_lifecycle.rs` aendert sich in W1-W6 und zeigt bereits den token_error*-Reconcile mit `manual_partner_opt_out=0`.
- Entscheidung: Hypothese A. Python `save_auth(... activate_raid_features=True)` setzt `manual_partner_opt_out=0` und `raid_bot_enabled=1`; Rust-AuthWriter heilt Pause/Raid, laesst Opt-out aber stehen. Empfehlung: enger Code-Fix in `rust/crates/tb-raid/src/auth_writer.rs`, Opt-out nur fuer altes `technical_pause_reason LIKE 'token_error%'` resetten, harte Pausen und echte manuelle Opt-outs erhalten.
- Verifikation: Wegwerf-Postgres `reauthdiag-pg` auf `127.0.0.1:55455`; `TB_TEST_DATABASE_URL=... TB_TEST_REQUIRE_DB=1 SQLX_OFFLINE=true cargo test -p tb-bot reauth_ohne_discord_state_fuehrt_partner_sync_aus` fuer `89248195` und mit eigenem Target fuer `0ca35c30` jeweils rot wie erwartet; Report JSON per `jq empty` validiert: `/home/naniadm/.claude/projects/-home-naniadm-Claude-Native-Workspace/5b3905c3-f094-4325-b670-f7f72c2d4352/scratchpad/triage/critic/reauth_optout_diag.json`.

## 2026-07-01 — Wave6 Rework tracked.rs schema-robust

- Start: delegierter GPT-Implementierungsworker fuer gezielten Rework in `rust/crates/tb-monitoring/src/poller/tracked.rs`; Scope nur Archiv-Kandidaten-Query plus Workflow/Report, kein cargo fmt, kein Commit/Push/Stash.
- Implementiert: `archive_candidates` rendert `sess.ended_at` und `sess.started_at` vor `NULLIF(..., '')` explizit als `text`, damit die Query sowohl auf TEXT-Fixtures als auch auf TIMESTAMPTZ-Prod-Schema laeuft.
- Verifikation: Wegwerf-Postgres `wave6rw-pg` auf `127.0.0.1:55450`; `TB_TEST_DATABASE_URL=... TB_TEST_REQUIRE_DB=1 SQLX_OFFLINE=true cargo test -p tb-monitoring` gruen; manuelle TIMESTAMPTZ-Prod-Schema-Repro liefert 1 Zeile ohne `invalid input syntax`; `SQLX_OFFLINE=true cargo build -p tb-monitoring` gruen; `SQLX_OFFLINE=true cargo clippy --all-targets -p tb-monitoring` exit 0 mit bestehenden Warnungen; Container entfernt.

## 2026-07-01 — Wave 6 Raid-Enforcement Kritiker

- Start: READ-ONLY adversariale Pruefung der uncommitteten Wave6-Aenderungen auf Branch `fix/py-rust-parity-obvious-bugs`; Scope Diff gegen HEAD, keine Source-Fixes, kein Commit/Push/Stash/Checkout/cargo fmt.
- Gelesen: Implementierer-Report `wave6_report.json`, aktueller Worktree/Diff und bestehender Workflow. Fokus jetzt auf RAID-RECRUIT-009/MON-RAID-009 fail-closed Enforcement, Guard-Reihenfolge und Paritaetsabweichungen.
- Ergebnis: 1 MED-Befund in `rust/crates/tb-monitoring/src/poller/tracked.rs`, weil `NULLIF(COALESCE(sess.ended_at, sess.started_at), '')::timestamptz` auf migriertem TIMESTAMPTZ-Schema mit `invalid input syntax for type timestamp with time zone: ""` scheitert; Test-Fixtures mit TEXT-Spalten verdecken das.
- Enforcement-Pruefung: Recruitment-Queue/Delivery, Auto/Manual-Pipeline, EventSub-Guard und Executor nutzen ID-ODER-Login bzw. fail-closed; manuelle `!raid` ignoriert weiche Raid-Blacklist und blockt Global-Ban. Restrisiko: Readiness-TOCTOU nach initialem Set-Load, aber kein bestaetigter Fail-Open fuer bereits geladene Bans.
- Verifikation: Wegwerf-Postgres `wave6crit-pg` auf `127.0.0.1:55449`; SQL-Repros fuer Hard-/Soft-Sets und Tracked-TIMESTAMPTZ-Bug; `TB_TEST_DATABASE_URL=... TB_TEST_REQUIRE_DB=1 SQLX_OFFLINE=true cargo test -p tb-raid -p tb-monitoring` gruen; `SQLX_OFFLINE=true cargo build -p tb-raid -p tb-monitoring -p tb-bot -p tb-transport-discord` gruen; `SQLX_OFFLINE=true cargo clippy --all-targets -p tb-raid -p tb-monitoring -p tb-bot` exit 0 mit bestehenden Warnungen; zusaetzlich `cargo test -p tb-transport-discord` gruen; Container entfernt. Report: `scratchpad/triage/critic/wave6_crit.json`.

## 2026-07-01 — Welle 6 Raid-Enforcement + Robustheit

- Start: delegierter GPT-Implementierungsworker fuer Wave6 Raid-Enforcement+Robustheit; Scope Rust-Crates `tb-raid`, `tb-monitoring`, `tb-bot`-Binary plus Workflow/Report, kein Commit/Push, kein cargo fmt.
- Recon: Rust-Hotspots gelesen: `auto_raid_pipeline.rs`, `raid_executor.rs`, `raid_blacklist.rs`, `outreach_boost.rs`, `signal_correlation.rs`, `pending_raids.rs`, `score_tracking_store.rs`, `partner_raid_delivery.rs`, `auto_raid.rs`, `eventsub_hooks.rs`, `raid_arrival_wiring.rs`, `partner_recruit.rs`, `confirm_resolver.rs`, `oauth_followups.rs`, `wiring.rs`; Python-Referenzen: `raid/services/raid_blacklist.py`, `raid/raid_pipeline.py`, `raid/services/outreach_boost_targets.py`, `raid/services/followers.py`, `raid/runtime_factories.py`, `raid/signal_correlation.py`, `raid/partner_raid_score_tracking.py`, `monitoring/eventsub_mixin.py`, `discord_role_sync.py`.
- Befund bisher: Auto-Raid filtert Blacklist/global gemeinsam; manueller Pfad nutzt dieselbe Pipeline und der `channel.moderate`-Guard cancelt weiche Blacklist; Recruitment-Follower bleibt `None`; Partner-/Recruitment-Send-Tasks haben keinen JoinHandle-Watcher; Outreach-Boost-Query nimmt `queued`/`detected_at` und schliesst aktive Partner aus, abweichend von Python.
- Implementiert bisher: harte globale Bans separat in `RaidBlacklistStore`; Executor/Pipeline/Manual-Guard fail-closed; Recruitment-Erkennung/Queue mit global-ban ID/Login-Ausschluss; Recruitment-Follower-Fallback und Send-Task-Watcher im Arrival-Sink; Outreach-Boost-Query auf Python-`sent`/`contacted_at`; Rollen-Sync-Fallback ueber Broker-Guilds; Confirm nutzt Pending-Score-Snapshot; Manual-Suppression nach Arrival-Insert.
- Verifikation: `SQLX_OFFLINE=true cargo build -p tb-raid -p tb-monitoring -p tb-bot` gruen; `SQLX_OFFLINE=true cargo clippy --all-targets -p tb-raid -p tb-monitoring -p tb-bot` exit 0 mit bestehenden Warnungen ausserhalb der geaenderten Wave6-Zeilen; Wegwerf-Postgres `wave6-raid-pg` auf `127.0.0.1:55447` fuer `TB_TEST_DATABASE_URL=... TB_TEST_REQUIRE_DB=1 SQLX_OFFLINE=true cargo test -p tb-raid -p tb-monitoring` gruen; Container entfernt.

## 2026-07-01 — Welle 5 Chat/IRC-Robustheit

- Start: delegierter GPT-Implementierungsworker fuer 7 Findings CHAT-API-014, CHAT-IRC-015/009/010/003/014 und CHAT-PIPE-006; Scope strikt `tb-monitoring`, `tb-bot`, `tb-chat`, `tb-engagement`, kein Commit/Push.
- Recon: Rust-Hotspots und Python-Referenzen gelesen: `irc_lurker.rs`/`irc_lurker_tracker.py`, `irc_lurker_wiring.rs`/`base.py`, `chatters_wiring.rs`/Chatters-Pythonpfade, `pipeline.rs`/`bot.py`, `irc_reader.rs`/`bot/engagement/irc_reader.py`.
- Implementiert bisher: IRC-Lurker-Locks poison-tolerant + Stop-Signal, tb-bot IRC-Task-JoinHandle-Logging, Chatters-Poll Noop-Fallbacks bei fehlendem TokenProvider, IRC-Partner/Category-Klassifizierung, Engagement-IRC sequenziell, Chat-Pipeline-Step-Isolation. `SQLX_OFFLINE=true cargo check -p tb-monitoring -p tb-bot -p tb-chat -p tb-engagement` gruen.
- Verifiziert final: `SQLX_OFFLINE=true cargo build -p tb-bot -p tb-monitoring -p tb-chat -p tb-engagement` gruen; `SQLX_OFFLINE=true cargo test -p tb-monitoring -p tb-chat -p tb-engagement` gruen (tb-chat 431 unit + 30 integration + 4 ignored doctests, tb-engagement 129 unit, tb-monitoring 76 unit + 90 integration); `SQLX_OFFLINE=true cargo clippy --message-format=short -p tb-bot -p tb-monitoring -p tb-chat -p tb-engagement --all-targets` exit 0 mit bestehenden Warnungen ausserhalb der geaenderten Stellen. Report: `/home/naniadm/.claude/projects/-home-naniadm-Claude-Native-Workspace/5b3905c3-f094-4325-b670-f7f72c2d4352/scratchpad/triage/fix/wave5_report.json`.

## 2026-07-01 — Welle 4 tb-dashboard-api Vertraege

- Start: delegierter GPT-Implementierungsworker fuer DASH-GATE-011/DASH-LIVEANN-005, DASH-GATE-014, DASH-BILL-002/OPS-RUNTIME-001; Scope strikt `tb-dashboard-api`, kein Commit/Push.
- Recon: Python-Referenzen gelesen: Live-Announcement-Routen in `bot/dashboard/live/live_announcement_mixin.py`/`routes_mixin.py`, Dashboard-Readiness in `bot/dashboard_service/app.py`; Rust-Router/Handler in `rust/crates/tb-dashboard-api`.
- Befund bisher: Demo-Router ist statisch/DB-frei; Live-Announcement-API-Tombstones fehlen; Readiness ist DB-only und muss auf Upstream/OAuth/Fingerprint-Paritaet gebracht werden.
- Implementiert: native JSON-410-Tombstones fuer `/twitch/api/live-announcement/config` (GET/POST), `/preview` (GET), `/test` (POST); `/readyz`/`/health` pruefen jetzt Internal-API-Health, OAuth-Konfiguration und Analytics-DB-Fingerprint-Mismatch mit 503 bei Nicht-Bereitschaft.
- Verifiziert: Demo-API in `handlers/demo.rs` bleibt statisch/DB-frei, kein echter Streamer-/DB-Zugriff. `SQLX_OFFLINE=true cargo build -p tb-dashboard-api`, `cargo test -p tb-dashboard-api` (700 passed, 1 ignored, Doc-Tests 0/2 ignored) und `cargo clippy -p tb-dashboard-api` gruen; Clippy meldet nur bestehende Warnungen ausserhalb der geaenderten Dateien. Report: `/home/naniadm/.claude/projects/-home-naniadm-Claude-Native-Workspace/5b3905c3-f094-4325-b670-f7f72c2d4352/scratchpad/triage/fix/wave4_report.json`.
- Kritiker-Review: 1 HIGH-Befund dokumentiert, weil Rust den Analytics-Fingerprint-Mismatch live in `/readyz`/`/health` berechnet, waehrend Python nur den Startup-Cache als 503-Grund nutzt. Tombstones/Demo/Text/Scope/unwrap unauffaellig. Verifikation aus `rust/`: Build gruen, Tests 700 passed/1 ignored plus Doc-Tests 0 passed/2 ignored, Clippy exit 0 mit bestehenden Warnungen. Report: `scratchpad/triage/critic/wave4_crit.json`.

## 2026-07-01 — Welle 3 tb-raid Kritiker Token/Grace-Lifecycle

- Start: READ-ONLY adversariale Pruefung der uncommitteten Aenderungen in `rust/crates/tb-raid/src/token_lifecycle.rs` und `rust/bin/tb-bot/src/token_lifecycle_wiring.rs`; kein Source-Fix, kein Commit/Push/Stash.
- Geprueft: Implementierer-Report `scratchpad/triage/fix/wave3_report.json`, `git diff`, Fail-Closed-Guards fuer Token-Error-Reactivation, Bot-Ban-Restore und Grace-Expiry; zusaetzliche SQL-Repros in isolierten Wegwerf-Containern.
- Ergebnis: 3 High-Befunde dokumentiert: Login-OR-Auth kann falsche UID reaktivieren, Bot-Ban-Blacklist wird im Reconcile nur per Login statt ID/oder Login geprueft, Grace-Expiry ueberschreibt `blocked`/`bot_banned` zu `token_error_expired`. Report: `/home/naniadm/.claude/projects/-home-naniadm-Claude-Native-Workspace/5b3905c3-f094-4325-b670-f7f72c2d4352/scratchpad/triage/critic/wave3_crit.json`.
- Verifikation: `SQLX_OFFLINE=true cargo build -p tb-raid` gruen; `SQLX_OFFLINE=true cargo check -p tb-bot` gruen; isolierter `postgres:16` auf `127.0.0.1:55437` fuer `TB_TEST_DATABASE_URL=... SQLX_OFFLINE=true cargo test -p tb-raid` gruen (264 Unit-Tests + 90 Integrationstests + 0 Doc-Tests); Container entfernt; `SQLX_OFFLINE=true cargo clippy -p tb-raid` gruen.

## 2026-07-01 — Welle 2 tb-analytics Observability + Billing

- Start: delegierter GPT-Implementierungsworker fuer ANA-REPORT-016/017/007 und DASH-BILL-012; Recherche aus `scratchpad/triage/answers/worker_1.json` und `worker_2.json` gelesen, kein Commit/Push.
- Implementiert: `save_analysis` loggt DB-Fehler best-effort; AI-Parser loggt sanitisierten Parse-Failure-Kontext; Post-Stream-Planfehler werden von fehlendem Analytics-Entitlement unterschieden; Stripe-Webhook-Plan-Sync laeuft nach Commit best-effort.
- Verifikation: `SQLX_OFFLINE=true cargo build -p tb-analytics`, `SQLX_OFFLINE=true cargo test -p tb-analytics`, `SQLX_OFFLINE=true cargo clippy -p tb-analytics` gruen; zusaetzlich `SQLX_OFFLINE=true cargo check -p tb-dashboard-api` gruen.

## 2026-07-01 — Parity Triage Worker 2 Gruppe B

- Start: READ-ONLY Analyse fuer CHAT-PIPE-002, DASH-BILL-012, MON-SUB-005, MON-SUB-006 und DASH-INTERNAL-019; keine Source-Edits, keine Builds/Tests/Commits.
- Geprueft: Rust- und Python-/Altstellen fuer Chat-EventSub-Deserialisierung, Stripe-Webhook-Plan-Sync, EventSub-401/429-Reconcile-Luecke und Rust-only diagnose/scam-guard Error-Shapes.
- Ergebnis: JSON-Report geschrieben nach `/home/naniadm/.claude/projects/-home-naniadm-Claude-Native-Workspace/5b3905c3-f094-4325-b670-f7f72c2d4352/scratchpad/triage/answers/worker_2.json`; Syntax per `jq empty` validiert.

## 2026-06-30 — Kritiker-Rework 5 Befunde

- Start: delegierter GPT-Implementierungsworker fuer CHAT-PIPE-007, ANA-OVERVIEW-006, CHAT-IRC-007, MON-POLL-003 und RAID-SCORE-003; Scope strikt auf die genannten Dateien plus Report, kein Commit/Push.
- Recon: Befundstellen in `chat_wiring.rs`, `overview.rs`, `irc_lurker.rs`, `poller/engine.rs` und `scoring.rs` bestaetigt; bestehende Tests/Fixtures werden lokal in denselben Dateien erweitert.
- Implementiert: Chat-Engagement an Pipeline-Bool gegated; Overview-Zeitbinds wieder `::text::TIMESTAMPTZ` inkl. Category-Fixture; IRC-DB-Fehler werden geloggt/Batch bricht ab; Poller skippt offline-schreibende Wartung im tracked-Stream-Fehler-Tick; `round_score` auf CPython-kompatibles 6-Stellen-Rounding umgestellt.
- Tests/Verifikation: `cargo check -p tb-bot`, `-p tb-analytics`, `-p tb-monitoring`, `-p tb-raid` gruen; `cargo test -p tb-bot -p tb-analytics -p tb-monitoring -p tb-raid --no-run` gruen; gezielte Regressionstests fuer Raid/Bot/Monitoring gruen (DB-optionale Tests skippen ohne `TB_TEST_DATABASE_URL`). Report: `/tmp/claude-1000/-home-naniadm-Claude-Native-Workspace/5b3905c3-f094-4325-b670-f7f72c2d4352/scratchpad/triage/critic/rework_report.json`.

## 2026-06-30 - tb-dashboard-api Critic Worker 3

- Start: READ-ONLY adversariale Pruefung fuer `rust/crates/tb-dashboard-api`; Spec/Findings gelesen, Scope `git diff origin/main..HEAD -- rust/crates/tb-dashboard-api`, keine Code-Fixes/kein Commit.
- Ergebnis: 1 MED-Befund (`DASH-AUTH-007`), weil Rust nur den OAuth-Kontext-Cookie-Namen angleicht, aber Set/Clear weiter mit `Path=/` ausfuehrt, waehrend Python callback-spezifische Pfade nutzt. Billing-, Scope-, Affiliate- und Promo-Aenderungen ohne weiteren belastbaren Befund.
- Verifikation: `git diff --check origin/main..HEAD -- rust/crates/tb-dashboard-api` gruen; `cargo test -p tb-dashboard-api --no-run` aus `rust/` gruen. Report geschrieben nach `/tmp/claude-1000/-home-naniadm-Claude-Native-Workspace/5b3905c3-f094-4325-b670-f7f72c2d4352/scratchpad/triage/critic/crit_dashboard.json`.

## 2026-06-30 - Critic tb-analytics + tb-internal-api

- Start: READ-ONLY adversariale Pruefung fuer `rust/crates/tb-analytics` und `rust/crates/tb-internal-api`; Spezifikation/Finding-Referenzen gelesen, keine Code-Fixes.
- Geprueft: `git diff origin/main..HEAD -- rust/crates/tb-analytics rust/crates/tb-internal-api`, Python-Referenzen fuer EventSub-Requeue/Debug und Overview-Aufrufer.
- Ergebnis: 1 high-Befund in `overview_category_rank` wegen Rueckbau der `ts_utc`-Zeitfilter-Typisierung gegen den aktuellen TIMESTAMPTZ-Migrationsvertrag; Internal-API-Findings ohne Befund.
- Verifikation: `git diff --check -- rust/crates/tb-analytics rust/crates/tb-internal-api` gruen; `cargo test -p tb-analytics -p tb-internal-api --no-run` aus `rust/` gruen. Report: `/tmp/claude-1000/-home-naniadm-Claude-Native-Workspace/5b3905c3-f094-4325-b670-f7f72c2d4352/scratchpad/triage/critic/crit_analytics_internal.json`.

## 2026-06-30 — tb-raid Critic Worker 4

- Start: READ-ONLY adversariale Pruefung fuer `rust/crates/tb-raid`; Scope Diff `origin/main..HEAD`, Fokus `RAID-SETUP-017`, `RAID-SCORE-003`, `RAID-SCORE-016`, Restore-Pfad und i32/i64-Stellen; keine Rust-Source-Edits/kein Commit.
- Ergebnis: 1 MED-Befund (`RAID-SCORE-003`), weil der neue `round_score`-Helper mathematische .5-Mikroeinheiten anders rundet als CPython `round(float, 6)` (`0.1234575`: Rust-Test 0.123458, Python 0.123457). `token_error`-Cleanup/Restore-Pfad und Upsert-Guard ohne weiteren Befund.
- Verifikation: `cargo test -p tb-raid --no-run` aus `rust/` gruen; `git diff --check` gruen. Report geschrieben nach `/tmp/claude-1000/-home-naniadm-Claude-Native-Workspace/5b3905c3-f094-4325-b670-f7f72c2d4352/scratchpad/triage/critic/crit_raid.json`.

## 2026-06-30 — tb-bot Critic Worker 5

- Start: READ-ONLY adversariale Pruefung fuer `rust/bin/tb-bot`; Scope Diff `origin/main..HEAD`, Fokus RAID-SETUP-014, RAID-ARR-011 und CHAT-PIPE-007-Caller-Nutzung; keine Source-Edits/kein Commit.
- Ergebnis: 1 High-Befund in `chat_wiring.rs`, weil `ChatPipeline::handle()`-Bool ignoriert und `spawn_engagement` weiter bedingungslos gestartet wird; RAID-SETUP-014 und RAID-ARR-011 ohne Befund.
- Report geschrieben nach `/tmp/claude-1000/-home-naniadm-Claude-Native-Workspace/5b3905c3-f094-4325-b670-f7f72c2d4352/scratchpad/triage/critic/crit_bot.json`.

## 2026-06-30 — tb-chat Critic Worker 6

- Start: READ-ONLY adversariale Pruefung fuer `rust/crates/tb-chat`; Scope Diff `origin/main..HEAD`, Python-Paritaet fuer Commands/Promos und Caller-Nutzung von `should_spawn_engagement`; keine Source-Edits/kein Commit.
- Ergebnis: 1 high Finding (`CHAT-PIPE-007`) dokumentiert, weil `tb-bot` den neuen `ChatPipeline::handle()`-Bool ignoriert und Engagement weiter bedingungslos spawnt; Commands-/Promo-Texte ohne belastbaren Zusatzbefund. Report: `/tmp/claude-1000/-home-naniadm-Claude-Native-Workspace/5b3905c3-f094-4325-b670-f7f72c2d4352/scratchpad/triage/critic/crit_chat.json`.

## 2026-06-30 — tb-bot Paritaetsfixes

- Start: delegierter GPT-Implementierungsworker fuer 6 Findings in `rust/bin/tb-bot`; Scope nur Binary-Crate plus lokale Tests, kein Commit/Push.
- Eingabe gelesen: `FIX_SPEC.md` und `tb-bot.json`; Evidence-Stellen gegen aktuellen Code geprueft. Sichtbarer Text nur als `PLATZHALTER_TEXT` mit Report-Stelle.
- Implementiert: Re-Auth-Followup nur mit Discord-ID, Chat-HTTP-Body-Snippet, Announcement-Chat-Fallback mit Platzhalter-Erfolgslabel, Stream-Fetch-Fehler als leerer Kandidaten-Snapshot, atomarer Orphan-Claim und Grace-Expiry unabhaengig vom Discord-Broker.
- Tests ergaenzt: Existing-Auth-Followup-Gate, Chat-Error/Fallback-Mapping, Stream-Fetch-Err-Fallback, Orphan-Claim und Token-Lifecycle-Sweep-Policy.
- Verifikation: `cargo check -p tb-bot` aus `rust/` gruen; `cargo test -p tb-bot --no-run` gruen; `git diff --check` gruen. Report geschrieben nach `/tmp/claude-1000/-home-naniadm-Claude-Native-Workspace/5b3905c3-f094-4325-b670-f7f72c2d4352/scratchpad/triage/fix/report_tb-bot.json`.

## 2026-06-30 — tb-raid Paritaetsfixes

- Start: delegierter GPT-Implementierungsworker fuer 9 Findings in `rust/crates/tb-raid`; Scope nur dieses Crate plus Tests, kein Commit/Push.
- Eingabe gelesen: `FIX_SPEC.md` und `tb-raid.json`; Evidence-Gegenpruefung gestartet. Besonderheiten: RAID-SETUP-017 nur `clear_failure_count` exakt `token_error`; Restore-/Sweep-Pfade bleiben unveraendert. RAID-OAUTH-008 nur bei realem Multi-Version-Bedarf aendern.
- Implementiert: Follower-Unknown-Sentinel im Partnerpfad, eingefrorener Target-/Score-Snapshot im Pending, ScoreStore-Fehler-Fallback, Banker's Rounding fuer Scores/avg_duration, timestamp-guarded Score-Upsert, exakter `token_error`-Cleanup, Recruitment-Due-Limit 50 und History-Fehler nach erfolgreicher Helix-Antwort als best-effort.
- NO_CHANGE: RAID-OAUTH-008, weil die realen Writer/Migrationen aktuell ausschliesslich `enc_version=1` nutzen; kein spekulativer Multi-Version-Re-Read.
- Tests ergaenzt/angepasst: Target-Resolution Follower/Snapshot, Auto-Raid Pending/ScoreStore/History-Fallbacks, ScoreStore Timestamp-Guard, Banker's-Rounding, Token-Cleanup exakt, Recruitment-Due-Limit.
- Verifikation: `cargo check -p tb-raid` aus `rust/` gruen; `cargo test -p tb-raid --no-run` gruen; `git diff --check` gruen.

## 2026-06-30 — tb-dashboard-api Paritaetsfixes

- Start: delegierter GPT-Implementierungsworker fuer 12 Findings in `rust/crates/tb-dashboard-api`; Scope nur dieses Crate plus Tests, kein Commit/Push.
- Eingabe gelesen: `FIX_SPEC.md` und `tb-dashboard-api.json`; Evidence-Stellen gegen aktuellen Code geprueft.
- Implementiert: Audience-Fehlerpfade und Bot-Filter, geteilter dynamischer Viewer-Exclusion-Helper, tb_analytics-rawChatStatus-Fallback, Owner-Scope in AI-Chat/Viewer-Timeline, Checkout-customer_email, Cancel-Fallback-Persistenz, OAuth-Kontext-Cookie-Name, Affiliate-Commission-Route, Overview-Streamer-Echo, Self-Explainer-Leak-Marker und Promo-Message-POST.
- Tests ergaenzt/angepasst: Audience-Filter/DB-Fehler, AI-Chat Owner-Mismatch, Overview effektiver Streamer, Viewer-Timeline Pfad-Scope, Billing Checkout/Cancel/Promo, Affiliate-Commissions, Self-Explainer DOKUMENTE-Leak.
- Verifikation: `cargo check -p tb-dashboard-api` aus `rust/` gruen; `cargo test -p tb-dashboard-api --no-run` gruen; `git diff --check` gruen. Report geschrieben nach `/tmp/claude-1000/-home-naniadm-Claude-Native-Workspace/5b3905c3-f094-4325-b670-f7f72c2d4352/scratchpad/triage/fix/report_tb-dashboard-api.json`.

## 2026-06-30 — tb-analytics + tb-internal-api Paritaetsfixes

- Start: delegierter GPT-Implementierungsworker fuer 4 Findings in `rust/crates/tb-analytics` und `rust/crates/tb-internal-api`; Scope nur diese Crates plus Tests, kein Commit/Push.
- Eingabe gelesen: `FIX_SPEC.md`, `tb-analytics.json`, `tb-internal-api.json`; Evidence-Stellen gegen aktuellen Code geprueft.
- Implementiert: `overview_category_rank` propagiert Query-Fehler; Requeue- und EventSub-Debug-Fehlerformen auf Python-Paritaet gemappt; direkter EventSub-Dispatch nutzt vorab `ensure_dispatch_ready`.
- Tests ergaenzt: Category-Rank-Query-Fail, Requeue-Error-Shapes, EventSub-Unknown-Dispatch-503, Debug-DB-Fehler-Message.
- Verifikation: `cargo check -p tb-analytics -p tb-internal-api` aus `rust/` gruen; `cargo test -p tb-analytics -p tb-internal-api --no-run` gruen; `git diff --check` gruen. Report geschrieben nach `/tmp/claude-1000/-home-naniadm-Claude-Native-Workspace/5b3905c3-f094-4325-b670-f7f72c2d4352/scratchpad/triage/fix/report_analytics-internal.json`.

## 2026-06-30 — tb-chat Paritaetsfixes

- Start: delegierter GPT-Implementierungsworker fuer 9 Findings in `rust/crates/tb-chat`; Scope nur dieses Crate plus Tests, kein Commit/Push.
- Eingabe gelesen: `FIX_SPEC.md` und `tb-chat.json`; Evidence-Stellen gegen aktuellen Code geprueft. Wichtige Text-Regel: betroffene user-sichtbare deutsche Chat-Texte werden nur als `PLATZHALTER_TEXT` gesetzt und im Report mit Python-Referenz dokumentiert.
- Befund: `CHAT-PIPE-007` braucht zum vollstaendigen Runtime-Gate auch `rust/bin/tb-bot/src/chat_wiring.rs`; wegen Scope-Grenze wird im Crate nur der Pipeline-Outcome vorbereitet und die Caller-Verdrahtung im Report als offen markiert.
- Implementiert bisher: AutoBan-Notice-Send-Outcome-Logging, Silentban/Silentraid-Reauth-Gates, Engagement-On/Off/Status-Fehlerpfade mit Platzhaltern, Promo-Template-Renderer/Streamer-Validation und Pipeline-Engagement-Outcome im Crate.

## 2026-06-30 — tb-monitoring Paritäts-Bugfixes

- Start: delegierter GPT-Implementierungsworker fuer 8 Findings in `rust/crates/tb-monitoring`; Scope auf dieses Crate plus Tests, kein Commit/Push.
- Eingabe gelesen: Fix-Spezifikation und `tb-monitoring.json`; Gegenprüfung der Evidence-Stellen gestartet.
- Implementiert: IRC-Channel-Normalisierung fuer `get_chatters`, IRC-DB-Fehlerwarnungen mit Kontext, NAMES-Flush/Stagger, EventSub-Receiver-Fail-Closed fuer fehlenden `message-type` und leere Challenge, Core-Delivery-Ablehnung ohne `broadcaster_id`, Inbox-Panic-Supervision und Poller-API-Fehler ohne Offline-Transition.
- Tests ergänzt: Webhook-Header/Challenge-Helper, IRC-`get_chatters` mit `#`, Core-Delivery ohne Broadcaster, Inbox-Panic-Retry/Worker lebt weiter, Poller-API-Fehler erhält Live-State.
- Verifikation: `cargo check --manifest-path rust/Cargo.toml -p tb-monitoring` gruen; `cargo test --manifest-path rust/Cargo.toml -p tb-monitoring --no-run` gruen; `git diff --check` gruen.
- Kritiker-Review: committeten Diff gegen `tb-monitoring` read-only geprüft. Zwei MED-Befunde geschrieben nach `/tmp/claude-1000/-home-naniadm-Claude-Native-Workspace/5b3905c3-f094-4325-b670-f7f72c2d4352/scratchpad/triage/critic/crit_monitoring.json`: IRC-DB-Fehler vor execute bleiben still, Poller-Stale-Sweep kann API-Fehler-Fix umgehen. Verifikation im Review: `cargo check --manifest-path rust/Cargo.toml -p tb-monitoring`, `cargo test --manifest-path rust/Cargo.toml -p tb-monitoring --no-run`, `git diff --check origin/main..HEAD -- rust/crates/tb-monitoring` gruen.

## 2026-06-30 — sqlx Welle 5 Rework-4 tb-social-media Test-i64

- Start: delegierter GPT-Implementierungsworker fuer 3 Test-Build-Typfehler in `clip_queue.rs`, `insights_worker.rs` und `retention.rs`; kein Commit/Push, keine Git-Kommandos, Scope nur `#[cfg(test)]`-Module.
- Implementiert: Test-Fixture-/Erwartungstypen fuer bigint-IDs von `i32` auf `i64` nachgezogen: `seed_clip`, due-target Keys/Fixure-IDs und Retention-Expired-ID-Vergleich.
- Verifikation: `rustfmt --edition 2021` auf den drei Ziel-Dateien erfolgreich; `SQLX_OFFLINE=true cargo test -p tb-social-media --no-run` gruen.

## 2026-06-30 — sqlx Welle 5 Rework-3 tb-social-media

- Start: delegierter GPT-Implementierungsworker fuer zwei MED-Befunde in `analytics.rs` und `enrichment.rs`; kein Commit/Push, kein Prepare, keine Git-Kommandos.
- Implementiert: `list_clip_analytics` liest nullable `bucket` wieder mit Default-Semantik ueber `COALESCE(bucket, '') AS "bucket!"`; `iter_pending_enrichments` filtert vor `LIMIT` per `AND c.id BETWEEN 0 AND 2147483647`, bestehender `i32::try_from`-Skip mit `tracing::warn!` bleibt erhalten.
- Verifikation: `rustfmt --edition 2021 rust/crates/tb-social-media/src/analytics.rs rust/crates/tb-social-media/src/enrichment.rs` erfolgreich; SQL-Ausschnitte per `sed`/`rg` kontrolliert. ORDER BY, uebrige WHERE-Bedingungen und LIMIT-Param bleiben unveraendert.

## 2026-06-30 — sqlx Welle 5 Rework-2 tb-social-media

- Start: delegierter GPT-Implementierungsworker fuer 4 Kritiker-Befunde in `rust/crates/tb-social-media/src`; keine Commits/Pushes, kein `cargo sqlx prepare`.
- Vorab verifiziert: `git diff HEAD --` fuer `clip_queue.rs`, `approval.rs`, `upload_worker.rs`, `enrichment.rs`, `analytics.rs` und `clip/repository.rs` gelesen; die gemeldeten Regressionen im aktuellen Diff bestaetigt.
- Implementiert: Processing-Frische in `clip_queue` wird wieder als `timestamptz` in SQL verglichen (`is_fresh!`), Approval-Gate liefert bei int4-Out-of-range/DB-Fehlern `Result` statt stillem `false`, `iter_pending_enrichments` ueberspringt nur die einzelne nicht-konvertierbare ID mit `tracing::warn!`, int4-Metrik-Writes in Analytics/Repository nutzen checked `i32::try_from`.
- Verifikation: `rustfmt --edition 2021` auf allen geaenderten Rust-Dateien gruen; `git diff --check` gruen; statische Suche in den sechs Befund-Dateien findet kein verbleibendes `as i32`, keinen `COALESCE(... )::text`-Zeitvergleich und kein `try_into()`-Silent-False. `cargo check -p tb-social-media` stoppt erwartbar, weil `SQLX_OFFLINE=true` gesetzt ist und fuer die neue `clip_queue`-Query kein Cache existiert; kein `cargo sqlx prepare` gemaess Auftrag. Verbleibender scope-fremder `as i32`-Treffer liegt in `clip/service.rs`.

## 2026-06-30 — sqlx Welle 5 Rework tb-social-media prepare-Fehler

- Start: delegierter GPT-Implementierungsworker fuer `cargo sqlx prepare --workspace`-Rework in `tb-social-media`; kein Git, kein Build/Prepare/DB-Zugriff gemaess Auftrag. Schemaabgleich ueber `rust/migrations/20260601000000_baseline_schema.sql` plus `20260629120000_live_schema_type_reconcile.sql`.
- Befund: `twitch_clips_social_media.id`, `twitch_clips_upload_queue.id`, `twitch_clips_social_analytics.clip_id` und `clip_templates_* .id` sind im prod-aequivalenten Schema `bigint`; Upload-Flags und `clip_templates_streamer.is_default` sind `boolean`; `social_media_streamer_layout.cam_enabled/mode` sind NOT NULL, werden im LEFT JOIN aber nullable.
- Implementiert: Bool-SQL fuer Upload-Flags/is_default, bigint-RETURNING/Binds fuer Queue/Templates/Clip-/Analytics-IDs, nullable/non-null sqlx-Aliases an Downstream angepasst, Settings-JSON nullable entpackt. Keine `.unwrap()`/`.expect()` oder `.try_into().unwrap()` in neuen Produktionspfaden.
- Verifikation: `rustfmt --edition 2021` auf den geaenderten Rust-Dateien erfolgreich. Statische Suche: keine produktiven `::integer`-ID-Casts oder 0/1-Boolvergleiche mehr; verbleibender 0/1-Treffer liegt in einem `#[cfg(test)]`-Fixture. Kein Build, kein `cargo sqlx prepare`, keine Tests gemaess Auftrag.

## 2026-06-30 — sqlx Welle 5 tb-social-media

- Start: delegierter GPT-Implementierungsworker; Scope strikt auf die 79 CONVERTIBLE_PG-Callsites aus `rust/docs/sqlx-conversion-triage.md` im Abschnitt `tb-social-media — 79`. Keine Commits/Pushes, kein Build/Prepare/DB-Zugriff gemaess Auftrag.
- Eingang gelesen: `WORKFLOW.md` und Triage-Abschnitt ab Zeile 933; DYNAMIC- und TEST_ONLY-Stellen bleiben ausgeschlossen.
- Implementiert: alle 79 gelisteten CONVERTIBLE_PG-Callsites in `rust/crates/tb-social-media/src` auf `sqlx::query!` oder `sqlx::query_scalar!` umgestellt. Datei-Counts: analytics 3, approval 7, clip/repository 6, clip_analytics 3, clip_manager 5, clip_queue 9, clip_templates 13, credentials 1, enrich_pipeline 1, enrichment 6, insights_worker 2, layout 5, oauth 2, refresh_worker 3, report_writer 1, retention 7, settings 2, upload_worker 1, vocab 2.
- Abgrenzung: die 21 DYNAMIC-Stellen bleiben unveraendert (format!/nonliteral SELECT_SQL/QueryBuilder/plattformabhaengige SQL-Strings), ebenso alle TEST_ONLY-Queries. Schema-Loop `schema.rs::ensure_schema` bleibt als Runtime-DDL-Dynamik unveraendert.
- Auffaelligkeiten fuer Review: i32-API gegen potentiell bigint `twitch_clips_social_media.id`/FKs per SQL `id::integer` bzw. `$n::integer` stabilisiert; int4-Zaehler (`view_count`, Analytics-Zaehler, `clip_fetch_history.fetch_duration_ms`) am Bind-Ort auf i32 gecastet; JSONB-Strings ueber `$n::text::jsonb`; RFC3339-Strings fuer timestamptz ueber `$n::text::timestamptz`.
- Verifikation: `rustfmt --edition 2021` auf allen 19 geaenderten Rust-Dateien erfolgreich; statische Makro-Zaehlung ergibt exakt 79. Kein Build, kein Prepare, keine Tests gemaess Auftrag.

## 2026-06-30 — Wave 4b Re-Konvert raid_blacklist + partner_score_refresh

- Start: delegierter GPT-Implementierungsworker; Scope strikt auf `rust/crates/tb-raid/src/raid_blacklist.rs` und `rust/crates/tb-raid/src/partner_score_refresh.rs`. Keine Commits/Pushes, kein Build/Prepare/DB-Zugriff gemaess Auftrag.
- Recon: 14 Runtime-Callsites gefunden: 5 in `raid_blacklist.rs`, 9 in `partner_score_refresh.rs`. `load_all()` ist aktuell die geforderte 2-Arm-UNION (`twitch_raid_blacklist` + `twitch_chatter_global_ban`); kein dritter Arm.
- Implementiert: alle 14 Callsites auf `query!`, `query_as!` oder `query_scalar!` umgestellt. `twitch_live_state.last_started_at` wird als `NULLIF(last_started_at::text, '')::timestamptz AS "last_started_at?"` gelesen; `raid_blacklist::load_all` bleibt bei der 2-Arm-UNION.
- Verifikation: `rustfmt --edition 2021` auf beiden Rust-Dateien erfolgreich; statische Suche findet keine Runtime-`sqlx::query*(`-Callsites und keine `.bind(...)`-Reste in den zwei Ziel-Dateien. Kein Build, kein Prepare, keine Tests gemaess Auftrag.

## 2026-06-30 — sqlx Welle 4b tb-raid

- Start: delegierter GPT-Implementierungsworker; Scope strikt auf `rust/crates/tb-raid` und die 107 CONVERTIBLE_PG-Callsites aus `rust/docs/sqlx-conversion-triage.md`. Keine Commits/Pushes, kein Build/Prepare/DB-Zugriff gemaess Auftrag.
- Eingang gelesen: `WORKFLOW.md` und Triage-Abschnitt `tb-raid — 107`; DYNAMIC- und TEST_ONLY-Stellen bleiben ausgeschlossen.
- Recon: gelistete Produktions-Callsites mit aktuellen `sqlx::query*`-Treffern abgeglichen; `token_store::load_inner` bleibt als DYNAMIC-Stelle unveraendert. Schema-Check: `twitch_stream_sessions.started_at` ist im frischen Snapshot `timestamptz`, `twitch_live_state.last_started_at` bleibt TEXT; Makro-Konvertierung liest beide robust ueber `::text`/`NULLIF(..., '')::timestamptz` und meldet `last_started_at` als Typ-Auffaelligkeit.
- Implementiert: alle 107 gelisteten CONVERTIBLE_PG-Callsites in `rust/crates/tb-raid/src` auf `sqlx::query!`, `query_as!` oder `query_scalar!` umgestellt. Datei-Counts: arrival_tracking_store 4, auth_writer 5, external_recruitment_store 10, offline_eligibility 2, outreach_boost 2, partner_roster 1, partner_score_refresh 9, partner_setup 11, raid_blacklist 5, raid_history_store 3, reauth_admin 1, score_store 4, score_tracking_store 7, state_store 4, strikes_store 1, token_blacklist 10, token_lifecycle 23, token_refresher 4, token_store 1.
- Verifikation: `rustfmt --edition 2021` auf den 19 geaenderten Rust-Dateien erfolgreich; statische Suche zaehlt exakt 107 sqlx-Makros in den Scope-Dateien. Verbleibende `sqlx::query*(`-Treffer sind die 5 DYNAMIC-Stellen (`token_store::load_inner`, `partner_setup::normalize_related_tables`) oder TEST_ONLY-Queries. Kein Build, kein Prepare, keine Tests gemaess Auftrag.

## 2026-06-30 — Ticket 1.2 Runtime Tables to Migrations

- Start: delegierter GPT-Implementierungsworker; Scope auf `rust/migrations/`, Rust-Runtime-DDL-Entfernung und Scratch-Harness. Verbindliche Review-Regel aus Auftrag: keine Commits, kein Push, Aenderungen bleiben uncommitted.
- Eingangsstand: `main...origin/main`, Worktree sauber. Recon-Report und Harness aus Scratchpad gelesen; Produktions-DSN wird nur per Secret-Loader im selben Shell-Befehl verwendet.
- Implementiert: vier Migrationen `20260630141000` bis `20260630144000` fuer die 11 Prod-Tabellen; DDL aus erneutem `pgdump` ueber Harness abgeleitet. Produktive Runtime-Creator fuer `ai_analyses`, `internal_home_changelog`, `tb_chat_autoban_log`, `twitch_roadmap_items`, `twitch_stream_report_ratings` und `twitch_stream_report_ab_votes` entfernt. Test-Fixtures fuer `twitch_billing_events` und `twitch_outbound_chat_suppressions` bleiben mit Migrationsverweis erhalten.
- Scratch-Harness erweitert: `gate`/`gate --update` erzeugt `tb_migtest_drift`, touched den SQLx-Migrationstest und setzt `TEST_DATABASE_URL` nur im Cargo-Subprozess.
- Verifikation: `harness.py gate --update` gruen, final `harness.py gate` gruen; `coldiff`/`consdiff` fuer alle vier Gruppen leer. `cargo build` gruen. Gezielte Tests gruen: `tb-analytics ai_history/post_stream/webhook_apply`, `tb-chat --test suppression_db`, `tb-chat --test moderation_db`, `tb-dashboard-api roadmap/stream_report/internal_home`, `tb-bot chat_wiring`.
- Clippy: `cargo clippy -p tb-db -p tb-analytics -p tb-chat -p tb-dashboard-api -p tb-bot --all-targets -- -D warnings` blockiert vor Abschluss an bestehenden Lints in unveraenderten lokalen Dependencies (`tb-highlight::event_detector` needless_lifetimes, `tb-raid::partner_score_refresh` unnecessary_unwrap). Keine Commits/Pushes gemaess Review-Regel.

## 2026-06-22 — Overlay Spielmodus-Filter (Alle Modi / Standard / Street Brawl)

- Scope strikt auf `rust/crates/tb-dashboard-api/src/handlers/overlay.rs` und `bot/dashboard_v2/src/components/verwaltung/OverlayBuilderSection.tsx`; keine weiteren Crates/Dateien, keine neuen Dependencies, keine deadlock-api als Datenquelle. Review-Regel: Commits ja (auf `sp2/overlay-mode-filter`), aber kein Push/Merge/Restart.
- Befund: `/player-matches` liefert pro Match `game_mode` als Integer (`ECitadelGameMode`): 1 = Standard, 4 = Street Brawl; `match_mode` ist NICHT der Diskriminator. Filter komplett in `overlay.rs` umsetzbar.
- Datenschicht: `SteamMatch` um `#[serde(default)] game_mode: Option<i64>` erweitert und `#[derive(Clone)]` ergänzt. Neue reine Helfer `normalize_mode` (Param → `all|standard|brawl`, Default `all`) und `filter_by_mode` (standard→`Some(1)`, brawl→`Some(4)`, sonst keine Filterung). Der Filter wirkt VOR den bestehenden Stat-Helfern und nur auf match-abgeleitete Stats — rank/mmr-trend/live bleiben unberührt.
- `build_overlay_json` bekommt `mode: &str` und filtert die Match-Liste vor der Berechnung. Cache keyt jetzt pro `login|mode` (`cached_overlay_or_fetch` + `OverlayCache.entries/inflight`), 30s-TTL unverändert. `OverlayQuery` um `mode: Option<String>` erweitert; `overlay_api_handler` liest+normalisiert `mode`. `overlay_html_handler` ignoriert `mode` weiterhin (verzweigt nur auf `streamer`).
- Render-HTML: liest `mode` via `oneOf('mode', ['all','standard','brawl'], 'all')` und hängt `&mode=${mode}` an den Daten-Fetch.
- Builder: neues Select „Spielmodus" (Alle Modi/Standard/Street Brawl, Default `all`) oben neben Stil/Layout; State + `mode=` in der generierten URL.
- Tests: neue reine Tests `normalize_mode_*`, `filter_by_mode_*` (standard schließt brawl aus, brawl schließt standard aus, all enthält beides+unbekannte, kombiniert mit not_scored-Ausschluss). Render-/wiremock-Tests angepasst: HTML prüft mode-Param-Lesecode + `&mode=`-Anhang; Default `all` lässt bestehende JSON-Assertions gültig.
- Verifikation: `cargo build -p tb-dashboard-api` grün; `cargo test -p tb-dashboard-api overlay` 19/19 grün (inkl. 2 wiremock-DB-Tests); `cargo clippy -p tb-dashboard-api` ohne neue Warnungen in `overlay.rs` (nur vorbestehende in `admin_chat_action.rs`/`demo.rs`). `npm --prefix bot/dashboard_v2 run build` grün nach `npm ci --legacy-peer-deps`. Vorbestehender, scope-fremder Fail `handlers::market::tests::market_data_full_payload_shape` nicht angefasst.

## 2026-06-22 — Overlay-Politur nach User-Feedback (Hero-Icons, Strip, OBS-Fit)

- Start: Worktree `sp2/overlay-polish`; Scope strikt auf `rust/crates/tb-dashboard-api/src/handlers/overlay.rs`, `bot/dashboard_v2/src/components/verwaltung/OverlayBuilderSection.tsx`, `WORKFLOW.md`. Review-Regel: Commits pro Einheit, kein Push/Merge/Restart. User-Feedback: (1) Recent-Matches als Farb-Klecks haesslich, (2) Overlay unsauber/Strip laeuft ueber, (3) Groesse passt nicht zur OBS-Quelle.
- Diagnose (verifiziert): Hero-Icons luden nicht, weil die Hero-Namen→Icon-Map per Browser-`fetch()` (`loadHeroAssets`) unzuverlaessig fehlschlug → Map leer → ueberall Fallback-Kreise. Rang-Badge laedt, weil reines `<img>` (kein connect/CORS).
- P1 Root-Cause-Fix (server-seitig): neue gecachte Funktion `hero_icon_map`/`fetch_hero_icon_map` in overlay.rs holt per `reqwest` `<DEADLOCK_ASSETS_BASE>/v2/heroes?only_active=true` (Default `https://assets.deadlock-api.com`), baut Map `hero_name.lower → icon` (`images.icon_image_small`, Fallback `icon_image_small_webp`), eigener `OnceLock<Mutex<HeroIconCache>>` mit 6h-TTL und 5s-Timeout; best-effort (Fehler/Timeout → leere Map, nie `ok:false`). `RecentMatch` um `hero_icon`, `OverlayResponse` um `most_played_icon` erweitert; `build_recent` bleibt pur (`hero_icon: None`), Anreicherung in `build_overlay_json` nach Map-Lookup (Hero-Map parallel im `tokio::join!`). Render nutzt `match.hero_icon`/`data.most_played_icon` direkt als `<img src>`; Browser-`fetch()` der Hero-Map (`loadHeroAssets`/`heroIconByName`/`heroIconUrl`) komplett entfernt. Rang-Badge bleibt client-berechnet.
- P2 Recent-Strip: runde Vollfarb-Kreise → abgerundete quadratische Hero-Kacheln (26px, `border-radius:7px`, Portrait `object-fit:cover`), Sieg/Niederlage als dezenter 2px-Unterstrich (`border-bottom` in `--win`/`--loss`), kein Vollfarb-Klecks. Fallback ohne Icon: dezente Kachel + Buchstabe S/N in `--win`/`--loss`. Strip `flex-wrap:wrap` (kein Ueberlauf mehr); Bar-Layout-Kacheln kompakter (20px, nowrap).
- P3 OBS-Fit/Sauberkeit: Box-Layout 312→332px, Padding/Abstaende gestrafft (`14px 16px`, head-rule/cell enger), main-icon zu abgerundetem Quadrat. Builder: OBS-Groessen-Schritt auf vorgegebenen Text gesetzt; dynamische Groessenempfehlung (Label „Empfohlene OBS-Groesse", Wert je Layout: Box `360 × 280`, Leiste `560 × 120`); Vorschau-Hoehe je Layout angehoben (box 280, bar 120).
- Tests: Cache-Hit-DB-Test um `/v2/heroes`-wiremock-Mock (via `DEADLOCK_ASSETS_BASE` auf denselben MockServer) + Hero-Namen in Match-Mock + Icon-Assertions (`most_played_icon`, `recent[].hero_icon`, webp-Fallback) + Request-Count 3→4 erweitert; neuer `AssetsEnvGuard` + `clear_hero_icon_cache_for_tests` halten den 6h-Hero-Cache test-isoliert; HTML-Test auf entfernten Browser-Fetch (`!/v2/heroes`, `!loadHeroAssets`) und neue Kachel-/Label-Marker (`ov-tile`, `ov-tile-fallback`, `Match-Verlauf`) umgestellt; `build_recent`-/RecentMatch-Tests um `hero_icon: None`. Keine echten Netzcalls — Assets-Abruf gegen den lokalen wiremock-Mock.
- Verifikation: `cargo build -p tb-dashboard-api` gruen; `cargo test -p tb-dashboard-api` gegen reale Test-Postgres (127.0.0.1:5434) — alle 14 Overlay-Tests gruen inkl. DB-Cache-Test; 1 vorbestehender, scope-fremder Failure `handlers::market::tests::market_data_full_payload_shape` (per stash gegengeprueft: faellt auch ohne diese Aenderung, `build_market_data` paniced an Test-DB-Schema). `cargo clippy -p tb-dashboard-api` exit 0, 0 Warnungen in overlay.rs (63 vorbestehende in tb-raid/tb-social-media etc.). `npm --prefix bot/dashboard_v2 run build` (`tsc -b` + vite) gruen nach `npm ci --legacy-peer-deps`. Keine neuen Dependencies (reqwest war schon vorhanden, wiremock dev-dep).

## 2026-06-22 — Overlay-Builder-Rework (schick, GC-nativ)

- Start: Worktree `sp2/overlay-rework`; Scope strikt auf `rust/crates/tb-dashboard-api/src/handlers/overlay.rs`, `bot/dashboard_v2/src/components/verwaltung/OverlayBuilderSection.tsx`, `bot/dashboard_v2/src/pages/InternalHomeLanding.tsx`, `WORKFLOW.md`. Vorgaenger-Scaffolding (Structs/Imports) in overlay.rs war uncommittet vorhanden, nicht kompilierbar.
- Datenschicht (TDD): pure Helfer `summarize_today` (Tagesgrenze Europe/Berlin, `now_utc` als Param), `compute_kd`, `build_recent` (newest-first, Cap 15), `summarize_matches` um last_match + most_played erweitert; alle ignorieren `not_scored`. `build_overlay_json` fuellt alle neuen `OverlayResponse`-Felder; 30s-Cache bleibt. Unused `hero_id` aus `SteamMatch` entfernt. Pure-fn Unit-Tests (#[test], keine DB).
- Render: `OVERLAY_HTML` neu — Glassmorphism-Karte, 3 Themes (dark/light/accent via `data-theme` + CSS-Custom-Properties, accent = Marken-Gradient `#06B6D4`→`#A855F7`), 2 Layouts (box/bar), 4 Positionen, opacity nur auf Karten-Hintergrund (`--bg-alpha`), Recent-Strip (26px Ring-Icons + Punkt-Fallback), pulsierender Live-Dot, deutsche Zahlformatierung (`56,7 %` / `1,80` / `4–2`), leere Module verstecken sich. Alle Modul-Flags (lastmatch/mostplayed Default 0), `recent_n` 1–15 Default 10. Render-Branch-Tests + bestehender Struktur-Test angepasst.
- Builder: `OverlayBuilderSection` erweitert — Stil-/Layout-Select, alle 11 Modul-Toggles, Verlauf-Slider, Deckkraft-Slider, Position; URL traegt alle Params; Vorschau-Hoehe je Layout. Sidebar: `toolNavItems` um Eintrag `Stream-Overlay` (MonitorPlay) nach `Verwaltung` ergaenzt (einzige geteilte Nav).
- Verifikation: `cargo build -p tb-dashboard-api` gruen; `cargo test -p tb-dashboard-api` gruen (14 Overlay-Tests, davon DB-Tests gegen vorhandene TB_TEST_DATABASE_URL); `cargo clippy -p tb-dashboard-api` ohne neue Warnungen in overlay.rs (2 vorbestehende in admin_chat_action.rs/demo.rs bleiben); `npm --prefix bot/dashboard_v2 run build` (`tsc -b` + vite) gruen nach `npm ci --legacy-peer-deps`. Vier Commits (Daten/Render/Builder/Sidebar) auf `sp2/overlay-rework`, kein Push/Merge/Restart.

## 2026-06-22 — Overlay-Baukasten als eigene Seite

- Start: delegierter GPT-Implementierungsworker; Scope auf `bot/dashboard_v2/src/**`, `rust/crates/tb-dashboard-api/src/handlers/overlay.rs` und kleinen shared Helper in `spa.rs`; verbindliche Review-Regel: keine Commits, kein Push, Aenderungen bleiben uncommitted.
- Implementiert: `/twitch/overlay?streamer=<login>` liefert weiter das OBS-Render-HTML; `/twitch/overlay` ohne Streamer liefert den dashboard_v2-SPA-Index ueber gemeinsamen `spa`-Helper.
- Implementiert: eigene React-Seite fuer den Overlay-Baukasten, Route-Konstante und App-Routing; Verwaltung zeigt nur noch den Link zur neuen Seite.
- Verifikation: `npm --prefix bot/dashboard_v2 run build` gruen nach `npm ci --legacy-peer-deps`; `cargo build -p tb-dashboard-api` und `cargo test -p tb-dashboard-api` gruen; `cargo clippy -p tb-dashboard-api` exit 0 mit bestehenden Warnungen ausserhalb der geaenderten Overlay-/SPA-Stellen. Kein Commit gemaess Review-Regel.

## 2026-06-22 — Overlay-Builder-Seite + Config-Params

- Start: delegierter GPT-Implementierungsworker; Scope auf `bot/dashboard_v2/src/**` plus eingebettete JS/CSS-Logik in `rust/crates/tb-dashboard-api/src/handlers/overlay.rs`; verbindliche Review-Regel: keine Commits, kein Push, Aenderungen bleiben uncommitted.
- Implementiert: `/twitch/overlay` liest clientseitig `rank`, `winrate`, `streak`, `live` und `pos`; Default bleibt alles sichtbar und unten links.
- Implementiert: neue Verwaltungssektion `OverlayBuilderSection` mit Toggles, Positionswahl, Live-Vorschau, kopierbarer URL und OBS-Schritten; eingebunden in `Verwaltung.tsx`.
- Tests erweitert: Overlay-HTML-Test prueft Positionsklassen und Flag-Logik.
- Verifikation: `npm --prefix bot/dashboard_v2 run build` gruen nach `npm ci --legacy-peer-deps`; `cargo build -p tb-dashboard-api` und `cargo test -p tb-dashboard-api` gruen aus `rust/`.
- Clippy: `cargo clippy -p tb-dashboard-api` exit 0; bestehende Warnungen in unberuehrten Crates/Dateien bleiben offen. Kein Commit gemaess Review-Regel.

## 2026-06-22 — SP2 Live-Overlay OBS Browser-Source

- Start: Scope auf `rust/crates/tb-dashboard-api`; verbindliche Review-Regel aus Auftrag: keine Commits, Änderungen bleiben uncommitted.
- Befund: Public-Routen liegen in `build_public_router`; vorhandene Resolver-Tabellen sind `twitch_streamers` (`twitch_login` -> `twitch_user_id`) und `twitch_streamer_identities` (`twitch_user_id` -> `discord_user_id`).
- Plan: eigener Overlay-Handler mit öffentlichem JSON-Endpunkt, self-contained HTML-Route, 30s In-Memory-Cache und env-konfigurierbarer Steam-Bot-Basis `STEAM_BOT_RANK_URL`.
- Implementiert: `/twitch/api/v2/public/overlay` und `/twitch/overlay` in `tb-dashboard-api`, inkl. 30s JSON-Cache pro Login, Steam-Bot-Abrufe gegen `/player-mmr-trend`, `/player-matches`, `/player-live` und OBS-HTML ohne externe Assets.
- Verifikation: `cargo build -p tb-dashboard-api` gruen; `cargo test -p tb-dashboard-api` gruen.
- Clippy: `cargo clippy -p tb-dashboard-api` lief durch, meldete aber bestehende Warnungen in unberuehrten Crates/Dateien (`tb-raid`, `tb-social-media`, `tb-analytics`, sowie `tb-dashboard-api/src/handlers/admin_chat_action.rs` und `demo.rs`); gemaess Auftrag hier gestoppt und nicht bereinigt.
- Erweiterung: Overlay JSON gibt `badge_level` aus `current_badge` aus; HTML rendert Rang-Badge- und Live-Hero-Bilder nur ueber oeffentliche Deadlock-Asset-URLs, inkl. Valve-Attribution.
- Tests erweitert: JSON-Schema prueft `badge_level`; HTML-Test prueft Badge-URL-Logik und einmaligen `/v2/heroes?only_active=true`-Fetch.
- Verifikation Erweiterung: `cargo build -p tb-dashboard-api` gruen; `cargo test -p tb-dashboard-api` gruen; `cargo clippy -p tb-dashboard-api` ohne neue Overlay-Lints, aber weiterhin mit vorbestehenden Warnungen in `tb-raid`, `tb-analytics`, `tb-social-media`, `admin_chat_action.rs` und `demo.rs`.

## 2026-06-17 — Dashboard-Login Callback-Portierung

- Start: `WORKFLOW.md` war nicht vorhanden; Datei fuer laufende Implementierung angelegt.
- Ausgangszustand: `main`, ungetracktes `website/testing/` vorhanden und bleibt unangetastet.
- Untersuchung begonnen: Rust-Dashboard-OAuth, Caddy-Callback-Routing und Raid-OAuth-Pfad.
- Branch: `fix/dashboard-login-callback-twitch`.
- Befund: Caddy leitet `/callback/twitch` aktuell auf Python `127.0.0.1:8765`; Python delegiert Raid-OAuth weiter an die interne Rust-API `127.0.0.1:8776/internal/twitch/v1/raid/oauth-callback`.
- Implementierung: Dashboard-Redirect-Default auf `/callback/twitch`, Rust-Dashboard-Route fuer `/callback/twitch`, State-gated Raid-Dispatch zur internen API.
- Verifikation begonnen: `cargo test -p tb-dashboard-api` gruen; Release-Build fuer `tb-dashboard` und `tb-bot` gruen. Breiter Clippy-Lauf zeigte bestehende Warnungen in Ziel-/Abhaengigkeits-Crates; Zielpaket-Warnungen werden minimal bereinigt bzw. begrenzt erlaubt.
- Finale Verifikation: `cargo test -p tb-dashboard-api`, `cargo clippy --no-deps -p tb-dashboard-api -p tb-dashboard -p tb-bot --all-targets -- -D warnings` und `cargo build --release -p tb-dashboard -p tb-bot` gruen.
- Hinweis: `cargo fmt` wurde wegen grosser Workspace-Formatierungswelle nicht als finaler Schritt beibehalten; Diff wurde wieder auf die fachlichen Aenderungen begrenzt.
