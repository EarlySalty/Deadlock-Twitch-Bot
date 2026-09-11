# Evidence: Werbemanager Queue-Phase

status: aktiv
datum: 2026-09-10

Fundstellen als pfad:zeile, gelesen und verifiziert am 2026-09-10.

## Entscheider (Änderungsziel)

- rust/crates/tb-analytics/src/ad_manager.rs:20 — `Strategy` Monitor/Snooze/Smart
- rust/crates/tb-analytics/src/ad_manager.rs:107 — `Settings` inkl. validate
- rust/crates/tb-analytics/src/ad_manager.rs:150 — `DecisionInput` (zu erweiterndes Struct)
- rust/crates/tb-analytics/src/ad_manager.rs:173 — `decide()` (Smart-Zweig ab L204: Startschutz, chat_ingest, quiet/cooldown)
- rust/crates/tb-analytics/src/ad_manager.rs:28 — bestehende decide-Testkonvention

## Worker (Änderungsziel)

- rust/bin/tb-bot/src/ad_manager_wiring.rs:37 — 25s-Tick, Kanal-Loop, Semaphore(8)
- rust/bin/tb-bot/src/ad_manager_wiring.rs:96 — `process_channel`: Reihenfolge live→scopes→token→schedule→decide
- rust/bin/tb-bot/src/ad_manager_wiring.rs:196 — `DecisionInput`-Bau, hier Match-Status einfügen
- rust/bin/tb-bot/src/ad_manager_wiring.rs:362 — Acht-Minuten-Sperre Commercial (unverändert)
- rust/bin/tb-bot/src/ad_manager_wiring.rs:529 — Testkonvention (Source-Verankerung)

## Dashboard-API (Änderungsziel)

- rust/crates/tb-dashboard-api/src/handlers/ad_manager.rs:111 — `StatusResponse` (Steam-Block ergänzen)
- rust/crates/tb-dashboard-api/src/handlers/ad_manager.rs:151 — `response()` (Lese-/Merge-Pfad)
- rust/crates/tb-dashboard-api/src/handlers/ad_manager.rs:390 — Testkonvention
- rust/crates/tb-dashboard-api/src/lib.rs:759 — Routen-Registrierung (unverändert)

## Steam-Status-Lesemuster (Vorlage, Kreuz-Domäne)

- rust/crates/tb-chat/src/steam_lookup.rs:91 — `get_live_state_for_discord_user`: JOIN core.steam_links × activity.live_player_state, Frische über COALESCE(deadlock_updated_at, last_seen_at)
- rust/crates/tb-chat/src/steam_lookup.rs:9 — LIVE_STATUS_FRESH_SECS=600 als Steam-Bot-kanonischer Frische-Wert (unsere Ad-Schwelle: enger, 180s)
- rust/crates/tb-chat/src/steam_lookup.rs:140 — Testschema core.steam_links / activity.live_player_state (Vorbild für Worker-Tests)

## Steam-ID-Anknüpfung Twitch-Kanal

- rust/crates/tb-dashboard-api/src/handlers/engagement_settings.rs:6 — POST engagement/update pflegt steam_id (Partner: eigener Kanal)
- rust/crates/tb-engagement/src/background.rs:63 — `load_enabled_channels` liest `twitch_engagement_settings(channel_login, enabled, steam_id)`
- rust/crates/tb-engagement/src/background.rs:113 — Match-Poller nutzt genau diese steam_id (30s-Takt) — Abgrenzung: wir lesen live_player_state direkt, nicht twitch_channel_match_state

## Bestätigte Verdrahtung (muss unberührt bleiben)

- rust/bin/tb-bot/src/main.rs:1546 — `ad_manager_wiring::spawn(...)` läuft
- rust/crates/tb-dashboard-api/src/lib.rs:759 — GET/POST ad-manager
- bot/dashboard_v2/src/pages/Verwaltung.tsx:388 — AdManagerSection im Tab „Werbung"
- rust/migrations/20260901100000_twitch_ad_manager.sql:9 — Settings-Tabelle (keine Migration nötig)
