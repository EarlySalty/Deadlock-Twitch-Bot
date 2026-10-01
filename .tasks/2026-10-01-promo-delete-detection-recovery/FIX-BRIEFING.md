status: überholt | Datum: 2026-10-01

# Fix-Briefing R2: Promo-Delete-Detection

Überholt durch die direkte Orchestrator-Anweisung vom 2026-10-01: Promo-Autor-Luna korrigiert den eigenen Branch, sichert den WIP und holt unabhängige Intent-Abnahme sowie den Gate-Lauf ein. Die folgenden alten Rechte- und Hold-Angaben sind historische Briefingfassung und gelten nicht mehr.

## 1. Referenz-Auszüge

- `rust/crates/tb-monitoring/src/dispatch.rs:851-855`: `on_chat_message_delete` wird aufgerufen und `outcome.processed = true` gesetzt.
- `rust/crates/tb-chat/src/promos.rs:778-840`: Alert-Zustellung liest den offenen Datensatz, sendet an den Sink und markiert danach `bot_log_sent_at`.
- `rust/crates/tb-monitoring/src/subscriptions.rs:1023-1027`: `CHAT_SUB_TYPES` enthält `channel.chat.message`, `channel.chat.notification` und `channel.chat.message_delete`.
- `rust/crates/tb-monitoring/tests/subscriptions.rs:1038-1044`: `chat_subscribe_lurker_mit_raid_auth_subscribed_normal` erwartet nach `ensure_chat_subscriptions` noch zwei Creates.
- `rust/bin/tb-bot/src/obs_dock.rs:881-884`: `ObsDockHooks::on_chat_message` schreibt ins Dock und delegiert danach an `inner`; der Wrapper delegiert die zwei neu hinzugekommenen Promo-Delete-Hooks derzeit nicht.

## 2. Scope-Zaun

Exakt die fünf offenen Befunde in `REVIEW.md`, kein Refactoring, kein `cargo fmt`, keine Änderung an fremden Pfaden oder Branches. Nur Rust-Code und diese Task-Artefakte. Keine neuen Codekommentare. Keine neuen Unter-Threads.

## 3. Arbeitszustand und Rechte

- Repo: `/home/nathanael/.worktrees/luna-dispatch-deadlock-twitch-bot-promo-delete-detection-20260915-364ad1c8`
- Branch: `codex/luna-dispatch/deadlock-twitch-bot/promo-delete-detection-20260915-364ad1c8`
- Start-HEAD: `364ad1c8d92c6701163b69d7608675ad08e50270`
- Startstatus vor Task-Artefakten: sauber.
- Änderungen nur in diesem Branch. Keine Commits, Pushes, Merges oder Live-Aktionen.
- Host-Resource-Hold: keine Cargo-Checks/Builds, Bundles oder Release-Läufe bis zur Freigabe. Keine fremden Prozesse, Locks oder sccache anfassen.
- TokenDB-Produktionshold bleibt verbindlich.

## 4. Beweisziel

Ein Announcement-Event, das vor dem nachgelagerten Audit-Insert eintrifft, darf seine Korrelation nicht verlieren. Gleichzeitige Event-Beobachtung darf keine Doppelmeldung erzeugen. Fehlgeschlagene Alert-Zustellung muss retrybar bleiben oder mit einem nachweisbaren Wiederherstellungspfad markiert werden. Die OBS-Dock-Hülle muss beide Hooks weiterreichen. Die Subscription-Assertion muss die drei tatsächlich erzeugten Typen prüfen. Wegen des Ressourcen-Holds keine schweren Laufzeitprüfungen starten.

## 5. Intent und Übergabe

Intent-Thread: `a7e16de3-e682-42da-a3d7-6c8c7d434f5e`.
Melde dich mit `[Bump-up] Paket <x>: Grund: ... Erledigt: ... Worktree: ... Offen: ...` an den Intent-Thread `a7e16de3-e682-42da-a3d7-6c8c7d434f5e` und stoppe danach.

Nach den Änderungen Diff-Stat und Git-Status zuerst melden. Kein Gate umgehen. Die neue Runde prüft ausschließlich diese R1-Fundliste.
