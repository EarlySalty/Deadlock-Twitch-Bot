# Plan: KI-Hilfe-Assistent im Streamer-Dashboard

status: aktiv
datum: 2026-09-04
klasse: hoch
research: .tasks/2026-09-04-dashboard-assistent/RESEARCH.md
contract: .tasks/2026-09-04-dashboard-assistent/CONTRACT.md

## Ziel

Siehe CONTRACT.md. Fertig, wenn: eingeloggter Streamer sieht auf jeder Dashboard-Route unten rechts "Hilfe bekommen", bekommt auf "Wie liefen meine letzten Streams?" eine Antwort mit seinen echten Zahlen und auf "Ist mein Spam-Schutz an?" den echten Schalterstand; ohne Session antwortet der Endpoint 401; Tests grün (cargo, npm); Sichtprüfung im Preview bestanden.

## Nicht-Ziele

Siehe CONTRACT.md (kein Tool-Calling, keine Schreibaktionen, kein Streaming, kein Plan-Gate, keine Admin-Fremdsicht).

## Vorgaben für alle Milestones

- Toolchain: `export PATH="$HOME/.rustup/toolchains/1.97.1-x86_64-unknown-linux-gnu/bin:$PATH" RUSTUP_TOOLCHAIN=1.97.1-x86_64-unknown-linux-gnu`, Arbeitsverzeichnis `rust/`.
- Keine Code-Kommentare. Keine Gedankenstriche in Nutzertexten. Echte Umlaute in allen sichtbaren Texten.
- Jede Änderung an bestehenden Dateien außerhalb von `dashboard_assistent.rs` nur als Sichtbarkeit (`pub(crate)`) oder Extraktion einer Lesefunktion, Verhalten identisch.
- Commits je Milestone, Präfix `feat(dashboard-assistent):`.

## Milestones

### M1 - Bausteine sichtbar machen
Änderungen:
- `handlers/self_explainer.rs`: `build_system_prompt` bleibt privat (eigener Prompt), aber `looks_like_injection`, `parse_history`, `truncate`, `split_message`, `output_unusable`, `knowledge_base`, `retrieval_query`, `RateLimiter` (samt `new(window, max)`), `mono_now`, `SelfExplainerAnswer` auf `pub(crate)`. Kein Verschieben.
- `handlers/internal_home.rs`: `AccessState`, `OauthData`, `KpisData`, `BanData` samt Feldern und `access_state_block`, `oauth_block`, `kpis_recent_block`, `raid_events_block`, `ban_events_block`, `last_stream_summary` auf `pub(crate)`.
- `handlers/moderation_settings.rs`: SQL des GET in `pub(crate) async fn load_settings(pool, channel_user_id) -> Result<ModerationSettings, sqlx::Error>` ziehen (Struct mit vier bool, Default alle true), GET ruft sie auf.
- `handlers/scam_guard_settings.rs`: analog `pub(crate) async fn load_settings(pool, channel_login) -> Result<ScamGuardSettings, sqlx::Error>`.
- `handlers/uplink.rs`: `live_status`, `verbindungen_lesen` auf `pub(crate)` (falls für die Uplink-Karte gebraucht), `twitch_identitaet` auf `pub(crate)`.
- `handlers/ad_manager.rs`: `scopes(pool, uid)` auf `pub(crate)`.
Erwarteter Zwischenzustand: Verhalten aller bestehenden Endpoints identisch, nur Sichtbarkeit.
Validierung: `cargo check -p tb-dashboard-api && cargo test -p tb-dashboard-api self_explainer moderation_settings scam_guard`
Stop-Regel: Wenn ein bestehender Test rot wird, der vorher grün war, stoppen und Ursache klären; keine Testanpassung.

### M2 - Protokoll-Tabelle und Schreibfunktion
Änderungen:
- `rust/migrations/20260904120000_twitch_dashboard_assistent_log.sql`: `CREATE TABLE IF NOT EXISTS public.twitch_dashboard_assistent_log (id BIGSERIAL PRIMARY KEY, twitch_user_id TEXT NOT NULL, page TEXT, language TEXT NOT NULL DEFAULT 'de', question TEXT NOT NULL, answer TEXT NOT NULL, grounded BOOLEAN NOT NULL DEFAULT FALSE, flagged_injection BOOLEAN NOT NULL DEFAULT FALSE, provider TEXT, model TEXT, latency_ms BIGINT, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW())` plus Index auf `(twitch_user_id, created_at DESC)`.
- `rust/crates/tb-analytics/src/dashboard_assistent_log.rs`: `pub struct Eintrag{...}`, `pub async fn insert(pool, &Eintrag) -> Result<(), sqlx::Error>`; in `lib.rs` exportieren. Kein `query!`-Makro (Offline-Daten), sondern `sqlx::query(...).bind(...)`, wie `self_explainer_log.rs`.
Erwarteter Zwischenzustand: Migration liegt vor, tb-analytics kompiliert.
Validierung: `cargo check -p tb-analytics -p tb-dashboard-api`
Stop-Regel: Bei sqlx-Offline-Fehlern nicht `cargo sqlx prepare` gegen Prod laufen lassen; auf `sqlx::query` ausweichen.

### M3 - Endpoint `dashboard_assistent.rs`
Änderungen: neue Datei `handlers/dashboard_assistent.rs`, Eintrag in `handlers/mod.rs`, Route `.route("/twitch/api/v2/dashboard/assistent/ask", post(dashboard_assistent::ask))` in `build_authed_router` neben `/twitch/api/v2/ai/chat`.
Aufbau:
1. Extractoren: `auth: DashboardAuthLevel`, `State(pool)`, `body: String`. `None` -> `unauthorized_v2_response()`. Identität über `uplink::twitch_identitaet` (user_id, login); Anzeigename aus `Partner.display_name`, sonst Login.
2. Body-Parsing: `question` (trim, HARD_MAX 1000, MAX 500 wie Bestand), `history` über `parse_history`, `page` (String, auf 64 Zeichen gekappt, nur `[a-z0-9/_-]`), `language` (`de`|`en`, sonst `de`). Ungültiges JSON -> 400 `invalid_json`. Leere Frage -> 400 `empty_question`.
3. Rate-Limit: zwei `RateLimiter`-Singletons per `OnceLock` (60 s / 20 und 86400 s / 150), Schlüssel = Twitch-User-ID. Überschreitung -> 429 `{error: "rate_limited"}`.
4. Datenkarten (parallel via `tokio::join!`, jede Karte fehlertolerant, bei Fehler "nicht verfügbar"): `partner_status` (access_state_block), `oauth` (oauth_block + scope_snapshot: connected, needs_reauth, granted, missing), `kpis_7` und `kpis_30` (kpis_recent_block mit since = now-7d / now-30d), `letzter_stream` (last_stream_summary), `raids` (raid_events_block, since now-30d, max 10), `bans` (ban_events_block, now-30d, nur Zähler), `moderation` (moderation_settings::load_settings), `scam_guard` (scam_guard_settings::load_settings), `uplink` (live_status + verbindungs_status, ohne Keys/URLs). Ausgabe als kompakter Textblock `## Deine Daten` mit Zeilen `Feld: Wert`, nur Zahlen, Status-Wörter, Datumsangaben, Kanalnamen von Raid-Partnern. Verbotene Schlüssel (`token`, `secret`, `key`, `url`, `session`, `cookie`) dürfen nie im Kartentext auftauchen; dafür gibt es die Funktion `karten_sind_frei_von_geheimnissen(&str) -> bool`, die vor dem Modellaufruf läuft und bei Verstoß die Karte verwirft.
5. Wissen: `knowledge_base().select(&retrieval_query(history, question), Namespace::Bot, Some("streamer"), 4)` -> `assemble_grounding`.
6. System-Prompt (eigene Konstante): Rolle "freundlicher Hilfe-Assistent der Deutschen Deadlock Community im Streamer-Dashboard", duzt, spricht den Streamer mit `{display_name}` an, antwortet in `{language}` (de: natürliches Deutsch mit echten Umlauten, keine Gedankenstriche; en: englisch), aktuelle Seite `{page}` als Kontext, Regeln: nur Fakten aus `## Wissen` und `## Deine Daten`, bei fehlender Info ehrlich sagen und auf den Community-Discord verweisen, nie Daten anderer Kanäle, nie Einstellungen ändern (nur den Weg im Dashboard beschreiben), kurz (max 6 Sätze), keine Listen länger als 5.
7. LLM: `tb_llm::complete("dashboard_assistent", Request::history(messages).system(prompt).max_tokens(768).temperature(0.2).timeout(110 s).total_deadline(110 s).strip_think().accept(!output_unusable).ledger_purpose("dashboard-assistent"))`. Fehler -> 502 `{error: "model_unavailable"}` mit Nutzertext.
8. Antwort: `{answer, parts: split_message(answer, 400), sources, grounded: true, page}`; Injection-Flag nur ins Log.
9. Log: `tokio::spawn` mit 3 s Timeout auf `tb_analytics::dashboard_assistent_log::insert`.
Tests (im Modul, nach Muster self_explainer): Body-Parsing (page-Sanitizing, language-Fallback), Rate-Limit blockt nach 20 und nach 150, Kartentext-Filter erkennt `token`/`key`, Prompt enthält Anzeigename, Sprache und Seite, History kann keine System-Rolle setzen, Karten-Formatierung aus Struct-Fixtures. Router-Test ohne Session -> 401 nach Muster `auth/idor_e2e_tests.rs` (falls dort ein Test-Router ohne DB möglich ist, sonst Unit-Test auf den Auth-Zweig).
Erwarteter Zwischenzustand: Endpoint kompiliert, Tests grün, Route registriert.
Validierung: `cargo test -p tb-dashboard-api dashboard_assistent && cargo clippy -p tb-dashboard-api -- -D warnings`
Stop-Regel: Wenn eine Datenkarte einen neuen Roh-SQL-Zugriff bräuchte, den es als Funktion nicht gibt, Karte weglassen und im Verlauf notieren, nicht neu schreiben.

### M4 - Frontend-Widget
Änderungen:
- `src/api/assistent.ts`: `askAssistent({question, history, page, language, csrfToken}) -> Promise<AssistentAntwort>`; POST über `buildApiUrl('/dashboard/assistent/ask')` mit `withCookieCredentials`, Header `X-CSRF-Token` nur wenn vorhanden, Timeout 125 s, 429 -> `AssistentRateLimitError`, 401 wird von `fetchJson` behandelt.
- `src/components/assistent/DashboardAssistent.tsx`: Port von `SiteChatbot.tsx` (Knopf unten rechts, Panel, Begrüßung mit Anzeigename aus dem Auth-Status, Vorschläge je Seite, Verlauf, Eingabe, Enter sendet, Escape schließt, Ladezustand, Fehlerzeile, Quellenzeile). Seite aus `window.location.pathname` gegen die Konstanten in `preview/routes.ts` plus `resolveTabParam` für `/analyse?tab=`. Sprache und Texte über `useLanguage()`/`useT()`. Nur Farben aus den Design-Tokens (`var(--color-primary)`, `var(--color-card)`, `var(--color-bg)`), Klassen in `src/components/assistent/assistent.css` oder Tailwind mit Token-Variablen.
- `src/components/assistent/vorschlaege.ts`: reine Funktion `vorschlaegeFuer(page, language) -> string[]` mit je drei Fragen für home, verwaltung, uplink, social-media, analyse und Standard.
- `src/App.tsx`: `<DashboardAssistent />` innerhalb des `LanguageProvider` als Geschwister der Routen-Ternär.
- `src/i18n/dictionary.ts`: neue Einträge (Knopf, Begrüßung, Platzhalter, Fehler, Quelle, Schließen).
- `tests/dashboardAssistent.test.ts`: `vorschlaegeFuer` je Seite und Sprache; Quelltext-Vertrag: App.tsx bindet das Widget im LanguageProvider ein, `assistent.ts` sendet `page` und `language` und setzt `X-CSRF-Token` bedingt. In `package.json` `test` eintragen.
Erwarteter Zwischenzustand: Build grün, Lint grün, Tests grün, Farbtest grün.
Validierung: `cd bot/dashboard_v2 && npm test && npm run lint && npm run build`
Stop-Regel: Wenn `brandPalette.test.ts` rot wird, Farbe auf Token umstellen, nie die Erlaubnisliste erweitern.

### M5 - Sichtprüfung
Änderungen: keine im Code. `npm run dev:preview` im Preview-Modus starten, Widget auf `/twitch/dashboard`, `/twitch/verwaltung`, `/twitch/uplink` öffnen, Screenshots ablegen unter `.tasks/2026-09-04-dashboard-assistent/screens/`. Falls Preview ohne Backend keine Session hat: Widget-Optik im Preview prüfen, Antwortpfad erst nach dem Deploy live.
Validierung: Screenshots vorhanden, Panel in Gold-Look, Knopftext "Hilfe bekommen".
Stop-Regel: Headless-Chrome in der Sandbox stallt bekanntermaßen (SwiftShader); dann Screenshot-Schritt überspringen und im Verlauf notieren, nicht an Flags drehen.

## Verlauf

- 2026-09-04: Contract, Research, Evidence, Plan angelegt (Orchestrator).
- 2026-09-04: M1 verifiziert (Commit 94033e6a). Baseline gesamt 1114 passed, 0 failed, 4 ignored (keine roten Alttests). cargo check grün, self_explainer 17 passed. moderation_settings und scam_guard sind DB-Tests und liefen als Skip (kein TB_TEST_DATABASE_URL), Verhalten der Extraktion identisch zur bisherigen Inline-Query.
- 2026-09-04, M4: verifiziert. Frontend-Widget gebaut (api/assistent.ts, components/assistent/{DashboardAssistent.tsx,vorschlaege.ts,assistent.css}, App.tsx-Einhängung im LanguageProvider, i18n-Einträge, tests/dashboardAssistent.test.ts, package.json). npm test 177/177 grün, npm run lint 0 Fehler, npm run build grün; Knopftext, assistent-knopf, dashboard/assistent/ask im gebauten Bundle nachgewiesen. Farben nur aus Design-Tokens (index.css @theme), keine ddc-design-tokens-Variablen verwendet, da diese nicht ins App-Bundle importiert werden. Kein Merge/Push/Deploy (nicht beauftragt).
- 2026-09-04: M2 verifiziert (Commit 97468635). Migration 20260904120000 und tb_analytics::dashboard_assistent_log angelegt (sqlx::query mit bind, kein Makro). cargo check -p tb-analytics -p tb-dashboard-api grün. DB-Tests echt gegen lokale Wegwerf-DB (Peer-Auth über Socket, kein Secret) gefahren: 2 passed. Rot-Gegenprobe durch Vertauschen der insert-Bindings: 2 failed statt 2 passed, danach zurückgesetzt.
- 2026-09-04: M3 verifiziert (Commit 554c3ea5). Endpoint handlers/dashboard_assistent.rs, Route POST /twitch/api/v2/dashboard/assistent/ask im build_authed_router, mod.rs-Eintrag. cargo test dashboard_assistent 9 passed (inkl. 401 ohne Session via lazy Pool), cargo clippy -D warnings 0 Fehler. Rot-Gegenprobe des Secret-Filters (karten_sind_frei_von_geheimnissen auf true gezwungen): 1 failed statt grün, danach zurückgesetzt. Abweichung vom Plan: eingehende Raids weggelassen, weil raid_events_block nur ausgehende Raids liest und es keine Lesefunktion für eingehende gibt (Stop-Regel INV-06); ausgehende Raids sind in der Karte. ad_manager::scopes wurde in M1 auf pub(crate) gehoben, in M3 aber nicht genutzt (oauth_block liefert granted/missing bereits); es entsteht kein Dead-Code, da scopes weiter innerhalb ad_manager verwendet wird.
