# Workflow

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
