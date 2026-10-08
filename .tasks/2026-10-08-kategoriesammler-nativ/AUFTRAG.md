# Auftrag: Kategoriesammler nativ in den Twitch-Bot, falsche Ausfallmeldung beheben

Auftraggeber: Claude-Hauptsession (Orchestrator) im T3-Thread des Nutzers.
Rolle: Teil-Orchestrator für diesen Bereich. Native Subagenten erlaubt, keine weiteren T3-Threads.
Worktree: `/home/nathanael/.worktrees/tb-kategoriesammler-nativ`, Branch `feat/kategoriesammler-nativ` (Basis origin/main f04c0ef0).

## Nutzerwunsch

„Das Ding sollte eigentlich ein nativer Teil vom Twitch-Bot sein und nicht irgendwie extern dran sein. Alles sauber korrigieren.“
Auslöser: zwei Discord-DMs des Watchdogs (03:33 und 15:37 am 2026-10-08):
„Der Deadlock-Kategoriesammler hatte seit 2026-10-08 13:35:57.998275 UTC einen Ausfall. Kategorie- und Chatdaten können Lücken enthalten. … Bitte tb-category-collector.service prüfen.“

## Befund des Orchestrators (verifiziert am 2026-10-08)

1. **Kein echter Ausfall.** `tb-category-collector.service` läuft seit 06:02 CEST ununterbrochen (PID 1830776) und committet jede Minute Snapshots. Zur Meldezeit loggt er nur `category storage state changed; archive unchanged raw_paused=true disk_paused=true`. Ursache war die volle Platte (100 %, 8 GB frei, Reserve 10 GiB). Der Orchestrator hat Cargo-Caches gelöscht, jetzt rund 106 GB frei. Der Watchdog (`rust/bin/tb-category-collector/src/bin/tb-twitch-watchdog.rs`, Unit `deadlock-twitch-bot-watchdog.timer`, alle 30 s) wertet die Speicherpause als „Ausfall“. Außerdem flappt der Zustand nachts mehrfach (pause/unpause im Halbstundentakt).
2. **Architektur extern.** Eigenes Binary `rust/bin/tb-category-collector`, eigene System-Unit mit eigenem OS-/DB-Nutzer `twitchcollector`, eigenes systemd-Credential (`/etc/credstore.encrypted/deadlock-category-twitch.cred`), eigene Bootstrap-Config `/etc/deadlock-twitch/category-collector.json`. Doku: `docs/category-collector.md`. Der Watchdog-Binary liegt ebenfalls im Collector-Crate.

## Ziel

1. Der Kategoriesammler läuft als nativer Teil des Twitch-Bots (`deadlock-twitch-bot-rust.service`, tb-bot), als Modul/Crate mit eigenen Tasks im Bot-Prozess, und nutzt die vorhandenen Bausteine des Bots: Config-Weg, Secrets-Weg (Infisical bzw. bestehende Bot-Credentials, App-ID/Secret wiederverwenden statt eigenes Credential), DB-Pool, Helix-Client, Logging, Alerting. Erst Bestand prüfen (Graphify, Skill `code-suche`), nichts doppelt bauen.
2. Datenschutz- und Sicherheitsgrenzen bleiben inhaltlich erhalten: Chat nur anonym über `justinfan` (nie über das Bot-Konto), kein Sende-/Moderationspfad im Lesepfad, keine Alterslöschung des Archivs, Speicherbudget und Plattenreserve weiter wirksam. Wenn die bisherige DB-Rolle `twitchcollector` für die Rechtetrennung zwingend gebraucht wird, sauber begründen; sonst Rechte über die Bot-Rolle mit neuer Migration (nie alte Migrationen ändern).
3. Die externe Unit `tb-category-collector.service`, ihr Credential, ihre Bootstrap-Config und das separate Binary werden nach erfolgreichem Live-Beweis entfernt (keine Doppelsammlung im Übergang: erst Bot mit Sammler live, dann alte Unit stoppen und disable).
4. **Meldungen korrigieren:** Eine Speicherpause ist kein Ausfall. Echter Ausfall = keine Snapshots mehr bzw. Heartbeat veraltet. Speicherpause bekommt eine eigene, ehrliche Meldung (Platte bzw. Budget voll, Bestand bleibt, neue Daten pausieren), einmal pro Vorfall, entprellt (Hysterese gegen Flappen), höchstens eine Meldung pro Tag, Wiederholungszahl mitführen. Text in natürlichem Deutsch, echte Umlaute, keine Em-Dashes, Zeit lesbar in deutscher Ortszeit statt Mikrosekunden-UTC, kein Verweis auf eine Unit, die es nach dem Umbau nicht mehr gibt. Der Watchdog prüft den Sammler danach als Teil des Bots.
5. Dashboard (`bot/dashboard_v2/src/pages/CategoryCollector.tsx`, `categoryCollectorStaleness.ts`) und Doku (`docs/category-collector.md`, Architekturdoku) an den neuen Aufbau anpassen.

## Regeln

- Rust, keine Code-Kommentare, keine ENV-Dateien oder ENV-Config, Secrets nur aus Infisical bzw. bestehendem Bot-Weg.
- Build-Toolchain siehe Memory `tb-bot-build-toolchain` (rustc 1.97.1). `cargo fmt`, `cargo clippy`, betroffene Tests. Bestehende Suites nicht brechen.
- Releases nur im eigenen Worktree bauen. Migrationen von Hand als `postgres` einspielen (Deploy-Wrapper migriert nicht), Grants an `twitchbot`/`twitchdash`, `_sqlx_migrations` nachtragen. `.sqlx` und Schema-Snapshot mitpflegen.
- Abschluss nach CLAUDE.md: Merge-Gate (`gate_hook.py --review`), bei BLOCK frische Fixer-Subagenten bis ALLOW; dann `git push origin HEAD:main` als Einzelschritt, Deploy über `deploy-twitch-release <sha>` (Skill `deploy-restart-selbstdienst`), Bot-Neustart, Live-Beweis (Snapshots im Bot-Journal, neue Zeilen in `category_stream_snapshots`, alte Unit gestoppt und disabled, Watchdog grün), Branch und Worktree aufräumen, danach `t3-thread.py settle --selbst`.
- Rücksprache nur bei echter Architekturfrage oder Datenverlustrisiko, dann als `FRAGE AN ORCHESTRATOR:` mit Empfehlung.

## Bericht

Kurz: was umgebaut wurde, Commits/Merge-SHA, Deploy-SHA, Live-Beweise, entfernte Altteile, offene Punkte.
