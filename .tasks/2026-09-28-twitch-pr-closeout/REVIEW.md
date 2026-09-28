status: aktiv
Datum: 2026-09-28

# Offene Befunde und Abhängigkeiten

## #995 Partner-Challenges-Dashboard

1. `rust/crates/tb-dashboard-api/src/lib.rs:1771`: Der native SPA-Router registriert `/twitch/challenges` nicht. Ein direktes Öffnen oder Neuladen der Seite liefert ohne passenden Legacy-Pfad keine Dashboard-Shell. Caddy #3 ergänzt nur die CSP, nicht die Rust-Route.
2. `bot/dashboard_v2/src/pages/Challenges.tsx:94`: Der Countdown rechnet mit der lokalen Browser-Zeitzone statt der Berliner Wochenfrist aus dem Serververtrag.
3. `bot/dashboard_v2/src/pages/Challenges.tsx:444`: Ein Fehler beim Abruf der Werber erscheint als inhaltlich leere Liste. Ein Ladefehler braucht einen eigenen Zustand.
4. Die Endpunkte für persönliche Challenges und Werber sowie das Ledger liegen in #997. #995 kann ohne diesen PR nicht wirken.

## #997 Partner-Einsatz-Engine

1. `rust/crates/tb-effort/src/lib.rs:142`: Eine Transaktion hält für den gesamten Poll eine Pool-Verbindung; `collect`, `settle` und `source_state` fordern weitere Verbindungen aus demselben Pool an. Bei der gültigen Einstellung `pool_max = 1` läuft jeder Poll in den Acquire-Timeout.
2. `rust/crates/tb-effort/src/sources/live.rs:108`: Die 48-Stunden-Untergrenze schließt weiterhin laufende längere Streams aus, obwohl der Live-State frisch sein kann.
3. `rust/crates/tb-effort/src/sources/live.rs:118`: Ein nach dem Tick aktualisierter `last_seen_at` ist neuer als `now` und wird trotz frischem Live-State abgewiesen.
4. Die Engine verwendet Daten aus den noch nicht produktiv bereitstehenden Einladungen (#999, Deadlock-Bots #466) und Clip-Events (#998). Ohne diese Quellen ist die API laut PR-Vertrag 503 statt einer vollständigen Auswertung.

## #996 Monatlicher Raid-Boost

1. `rust/crates/tb-raid/src/monthly_raid_boost.rs:441`: Der `LEFT JOIN` führt aktive Partner ohne ein einziges Punkteereignis als Rangliste mit null Punkten. Bei einem leeren Monat erhält die erste Twitch-ID einen Boost ohne nachgewiesenen Einsatz. Die übrigen alten Review-Befunde zu Reihenfolge, Serialisierung und Ledger-Schlüssel sind im aktuellen Head sichtbar korrigiert; die Integration gegen #997 bleibt zu prüfen.

## Drafts und gekoppelte Repos

- #998 Clip-Wettbewerb: Draft; öffentliche `/clips`-Routen hängen an Caddy #7. Dort ist der `routes`-Check rot, also zunächst den realen Grund prüfen.
- #999 persönliche Einladungen: Draft; der Discord-Broker und die zentrale Migration liegen in Deadlock-Bots #466, ebenfalls Draft.
- Ohne `main`-Merge findet keine Migration und kein Deploy statt. Das lokale Scheduler-Review-Gate hat den Abhängigkeits-Head `d3fb1c665196` in der Queue. Ein bestehender älterer Twitch-Queue-Eintrag liegt seit 2026-09-24 im Status `release`; für diesen Auftrag ist noch kein Freigabeurteil eingetroffen.
