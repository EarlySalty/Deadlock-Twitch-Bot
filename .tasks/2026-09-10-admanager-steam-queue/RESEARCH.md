# Research: Werbemanager Queue-Phase

status: aktiv
datum: 2026-09-10

## Befund in einem Satz

Der Twitch-Werbemanager ist zu etwa 90 Prozent gebaut (Worker, Queue,
Dashboard-API, UI, Migration), die Smart-Strategie entscheidet aber ausschließlich
nach Chat-Ruhe. Die Steam-Match-Anbindung ist die fehlende Erweiterung; alle
Bausteine dafür existieren und werden nur gelesen.

## Ist-Zustand Werbemanager (Twitch-Ad-Breaks, nicht Chat-Werbung)

- Migration `rust/migrations/20260901100000_twitch_ad_manager.sql`: Tabellen
  `twitch_ad_manager_settings` (enabled, strategy monitor|snooze|smart,
  ad_duration_seconds, min_interval_minutes, startup_delay_minutes,
  quiet_window_minutes, action_lead_seconds, Worker-Lease),
  `twitch_ad_manager_state` (Live/Ad-Plan/letzte Entscheidung/Heartbeat),
  `twitch_ad_manager_actions` (Queue mit Lease, idempotency, unknown-Audit).
- Entscheider `rust/crates/tb-analytics/src/ad_manager.rs`: `Strategy`
  (Monitor/Snooze/Smart, L20), `Settings` mit validate (L106),
  `decide()` (L173): Lead-Fenster vor `next_ad_at`; Snooze-Strategie snoozt
  fällige Ads; Smart = Startschutz → chat_ingest_health → Cooldown +
  `quiet_chat_messages == 0` → Commercial, sonst Snooze.
- Worker `rust/bin/tb-bot/src/ad_manager_wiring.rs`: 25s-Tick je Kanal
  (L37–L85), `process_channel` (L96): Live-State-Frische, Scopes, Token,
  `helix.get_ad_schedule`, Reconcile, `decide()`, enqueue, `execute`
  (L299: snooze_next_ad / start_commercial, Acht-Minuten-Sperre L362,
  mark_unknown_before_send L377).
- Verdrahtung: `rust/bin/tb-bot/src/main.rs` L1546 (`ad_manager_wiring::spawn`).
- Dashboard-API `rust/crates/tb-dashboard-api/src/handlers/ad_manager.rs`:
  GET/POST Settings (Session-Identität, Scope-Gates, Reauth 409), POST Action
  mit Rate-Limit; Routen `rust/crates/tb-dashboard-api/src/lib.rs` L759–L764.
- Frontend `bot/dashboard_v2/src/components/verwaltung/AdManagerSection.tsx`
  (Strategiekarten, Live-Status, manuelle Aktionen) +
  `bot/dashboard_v2/src/api/adManager.ts`; eingebunden in
  `bot/dashboard_v2/src/pages/Verwaltung.tsx` L388 (Tab „Werbung“).
- Ad-Plan-Snapshot: `twitch_ads_schedule_snapshot` +
  Handler `rust/crates/tb-dashboard-api/src/handlers/ads_schedule.rs`
  (Historie/next_ad_at/snooze_count).

## Steam-Match-Status (Bestand, nur lesen)

- `core.steam_links` (discord_id ↔ steam_id) und `activity.live_player_state`
  (`in_deadlock_now`, `in_match_now_strict`, `deadlock_hero`,
  `deadlock_stage`, `deadlock_updated_at`, `last_seen_at`) werden vom
  Steam-Bot gepflegt; keine Migration im Twitch-Repo (Fremd-Domäne, Lese-Zugriff).
- Lesemuster inkl. Frische-Schranke:
  `rust/crates/tb-chat/src/steam_lookup.rs` L91–L127
  (`get_live_state_for_discord_user`, LIVE_STATUS_FRESH_SECS = 600 als
  Steam-Bot-kanonischer Wert), Dokumentation der Kreuz-Domänen-Lese-Praxis.
- Twitch-seitige Steam-ID je Kanal: `twitch_engagement_settings.steam_id`,
  gepflegt über `POST /twitch/api/v2/engagement/update`
  (`rust/crates/tb-dashboard-api/src/handlers/engagement_settings.rs` L6–L8,
  Partner nur eigener Kanal); der Engagement-Match-Poller liest genau diese
  Spalte (`rust/crates/tb-engagement/src/background.rs` L63–L76, L113–L127).
- `twitch_channel_match_state` (tb-engagement match_context.rs) ist eine
  zweite, Engagement-eigene Quelle (deadlock-api.com) und wird nur für
  Engagement-aktivierte Kanäle gepollt — bewusst NICHT verwendet, damit der
  Werbemanager nicht von Engagement-Einschreibung abhängt.

## Design-Entscheidungen

- Quelle = `activity.live_player_state` über `twitch_engagement_settings.steam_id`
  (bestehender Anknüpfungspunkt, vom Streamer selbst pflegbar). Kein neuer
  OAuth-Weg, keine zweite Wahrheit.
- Frische-Schwelle für Ads: 3 Minuten, enger als die 600-Sekunden-Presence-
  Toleranz des Titel-Generators, weil die Queue-Phase kurz ist; bei Überschreitung
  greift der REQ-04-Fallback (konservative Fehlerichtung: kein Fehlverhalten,
  nur altes Verhalten).
- „Nicht im Match“ (`in_match_now_strict = false`, frisch) genügt als
  Werbefenster; die UI unterscheidet in Queue/Menü (in Deadlock) vs. nicht in
  Deadlock nur bei der Anzeige, nicht in der Entscheidung.
- Loader-Funktion `steam_match_state(pool, channel_login)` in tb-analytics,
  damit Worker und Dashboard-API denselben Pfad nutzen (kein doppeltes SQL).

## Testlandschaft (Vorbilder)

- Entscheider-Unit-Tests: `rust/crates/tb-analytics/src/ad_manager.rs` L28ff.
- Worker-Tests mit Quelltext-Verankerung: `ad_manager_wiring.rs` L529ff.
- Handler-Tests mit Testschema: `ads_schedule.rs` L110ff (TB_TEST_DATABASE_URL,
  Schema-Isolation, skip ohne DSN).
- Frontend: `bot/dashboard_v2/tests/` (26 Dateien), Build via vite.

## Offene Risiken

- Steam-ID-Feld ist Engagement-Sache; Streamer ohne Engagement-Nutzung haben
  es evtl. nie gesetzt → REQ-08-Hinweis ist Pflicht, Fallback fängt Verhalten ab.
- Unbekannt ist der tatsächliche Update-Takt des Steam-Bots auf
  `deadlock_updated_at`; die 3-Minuten-Schwelle ist konservativ gewählt und im
  Fallback schadlos.
