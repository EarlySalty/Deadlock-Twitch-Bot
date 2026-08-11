status: erledigt
datum: 2026-08-11

# Plan

## Milestone 1, Erfolgspfad sichtbar machen

- Änderung: In `sync_live_announcement_at` die gerenderte Preview-URL vor dem Transport-Call sichern und beim Ergebnis `Updated` mit Bucket, Message-ID, URL und vorherigen Fehlversuchen loggen.
- Erwarteter Zustand: Jeder ausgeführte 5-Minuten-Refresh ist im Journal von `deadlock-twitch-bot-rust.service` nachvollziehbar. Verhalten und Discord-Payload bleiben gleich.
- Validierung: `cargo --config 'build.rustc-wrapper=""' test -p tb-monitoring --test announce -- --include-ignored`
- Stop-Regel: Bei Compile- oder Testfehlern nicht weitergehen.

## Milestone 2, Review und Live-Prüfung

- Änderung: Diff und Log-Felder prüfen, Task-Artefakte abschließen.
- Validierung: `cargo build --workspace` und Journalprüfung nach dem Service-Neustart.
- Stop-Regel: Keine Behauptung über Live-Wirkung ohne erfolgreichen Build und laufenden Service.
