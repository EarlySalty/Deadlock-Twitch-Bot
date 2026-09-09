# Evidence

- Erinnerungstext heute generisch ohne Belohnungsname: rust/crates/tb-chat/src/promos.rs:1946
- Versandlogik, Limits, Session-Dedupe: rust/crates/tb-chat/src/promos.rs:1679 (maybe_send_lurker_tax_reminder), Konstanten :283-292
- Kandidaten-SQL (3 Sessions, 240 Minuten, Chatters-API, 5 Minuten frisch): rust/crates/tb-chat/src/promos.rs:1852
- Scope-Gate Laufzeit akzeptiert Streamer-Scope ODER Bot-Token: rust/crates/tb-chat/src/promos.rs:1827-1849
- Bot-Token-Scope-Provider: rust/bin/tb-bot/src/chatters_wiring.rs:41,68-72
- Dashboard-Warnung prüft nur twitch_raid_auth des Streamers: rust/crates/tb-dashboard-api/src/handlers/lurker_tax_settings.rs:91-108,119-121
- Kein Streamer-OAuth-Profil enthält moderator:read:chatters: rust/crates/tb-raid/src/scope_profiles.rs:32-111 (grep leer), nur token_provider.rs:64 erwähnt ihn
- Redemption-EventSub wird abonniert: rust/crates/tb-monitoring/src/subscriptions.rs:484-491, dispatch.rs:70-71
- Redemption wird nur gespeichert, keine Chat-Reaktion: rust/crates/tb-monitoring/src/dispatch.rs:948-956, telemetry.rs:201-229 (Tabelle twitch_channel_points_events mit reward_title, user_login, session_id)
- Scope channel:read:redemptions im Basisprofil: rust/crates/tb-raid/src/scope_profiles.rs:39,70
- Chat-Befehl !lurkersteuer_off: rust/crates/tb-chat/src/catalog.rs:224, commands.rs:1437-1513
- Doku: docs/LURKER_TAX.md (Reminder-Copy: bisher bewusst ohne Belohnungsname)
- Journal deadlock-twitch-bot-rust, 14 Tage: keine einzige Lurker-Steuer-Sendung
