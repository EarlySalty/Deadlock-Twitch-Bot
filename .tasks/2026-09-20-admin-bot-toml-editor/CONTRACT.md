# Twitch-TOML im eigenen Admin-Dashboard

Nutzerauftrag: Alle bestehenden Bot-TOMLs aus dem jeweiligen Admin-Dashboard bearbeiten, speichern und aktivieren; Twitch bleibt im Twitch-Admin. Vollständige Integration, Prüfung, Merge und Deployment.

## Implementierung

Neue Seite `/twitch/admin/config/toml`, Menü Operations / Bot-Konfiguration. Die Bedienoberfläche und der Dateischreibkern werden vom Discord-Admin-Dienst bereitgestellt, aber ausschließlich serverseitig über die vorhandene Loopback-Dienstauth angebunden. Browser bekommen keine internen Diensttokens und können keine Bot-ID, Serverpfade oder Unit-Namen wählen.

Die Twitch-Routen verlangen den bestehenden Admin-Extractor. Konfigurationszugriff erfordert zusätzlich eine tatsächlich aufgelöste Admin-Sitzung (Discord-Admin oder Twitch-Admin) mit deren vorhandenem CSRF-Token. Schreibanfragen benötigen dieses Token, passende Herkunft und JSON-Content-Type. Partnerkonten und bloße interne/Localhost-Promotion ohne Sitzung werden abgewiesen. Die Seite erhält ihr CSRF-Token nur über ihren authentifizierten GET, nicht über einen neuen Authentifizierungsweg.

Speicherung, Schema-Validierung, Byte-CAS, private Versionshistorie und beaufsichtigte Aktivierung liegen im registrierten zentralen Editor. Twitch ist dort von den Discord-Ziel-IDs getrennt. Die laufende Twitch-TOML-Migration wird nicht mit fremden uncommitteten Änderungen überschrieben; eine fehlende produktive TOML wird als fehlende Verbindung angezeigt.

## Scope

rust/crates/tb-dashboard-api/src/handlers/admin_bot_toml.rs
rust/crates/tb-dashboard-api/src/handlers/mod.rs
rust/crates/tb-dashboard-api/src/lib.rs
rust/crates/tb-dashboard-api/src/auth/discord_admin_login.rs (bestehende Broker-URL und Secret-Helfer crate-intern verfügbar machen)
bot/admin_dashboard/src/pages/config/BotTomlConfig.tsx
bot/admin_dashboard/src/App.tsx
bot/admin_dashboard/src/components/layout/Sidebar.tsx
.tasks/2026-09-20-admin-bot-toml-editor/**

## Abnahme
Admin-/Partner-/Session-Negativtests, CSRF/Origin/Content-Type, kein freier Proxy-Pfad, begrenzte und nicht umgeleitete Upstream-Antworten, keine Credentials im Browser oder Logs, Draft-Erhalt bei Fehlern und interner Navigation, TypeScript-/Frontend-Build, Rust-Checks und Review-Gate. Live-/Merge-Status ausschließlich nach tatsächlicher Durchführung melden.
