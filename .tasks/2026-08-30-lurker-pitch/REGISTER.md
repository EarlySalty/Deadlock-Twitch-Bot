status: aktiv
Datum: 2026-10-01

# Register

- Intent-Thread: `6a3e284e-bb7b-4a33-85cd-536fd9b190bd`
- Source-Branch/SHA: `feat/lurker-discord-pitch` / `6ff725ee6808eed2235429ed4cadddfa8f0deead`
- Arbeitsbranch/Start-SHA: `codex/luna-dispatch/deadlock-twitch-bot/feat-lurker-discord-pitch-6ff725ee` / `6ff725ee6808eed2235429ed4cadddfa8f0deead`
- Worktree: `/home/nathanael/.worktrees/luna-dispatch-deadlock-twitch-bot-feat-lurker-discord-pitch-6ff725ee`
- Source-Worktree: `/home/nathanael/repos/tb-lurker-pitch`, unveränderte uncommittete/untracked Pfade laut Inventur: keine
- Source-Commit: `wip: lokale Arbeit vor Worktree-Bereinigung`
- Auftrag: `CONTRACT.md`
- Worker-Threads: keine

## Branchgruppe

- PR 1035 teilt `rust/bin/tb-bot/src/chat_wiring.rs` und `rust/crates/tb-db/tests/fresh_schema_snapshot.txt` mit diesem Branch.
- Die PR-Hunks betreffen Clip-Contest-Wiring und Community-Tabellen, nicht den Lurker-Pitch-Vertrag. Gemeinsame Pfadabnahme und Gate bleiben vor Integration erforderlich.
- Community-Streamer-Brücke Integrator: T3-Thread `e60a2e14-1b57-4700-bb31-bc0492f4487e`.
- Ein unabhängiger Coaching-Pitch-Branch wurde auf Commit `4d5c899a9da2bb4d6f5215e271736db4fb9426a7` geprüft. Keine gemeinsamen Dateipfade.

## Status

- Ausgangsstatus, HEAD und Branchhistorie geprüft. Arbeitsbaum war sauber.
- `origin/main` enthält weder `lurker_pitch_enabled`, `twitch_lurker_pitch_log`, `LURKER_PITCH_REPLY` noch den ForeignChannel-Guard. PR 1035 enthält die Featureänderung ebenfalls nicht.
- Statische Vertragsprüfung: Pitch-Kandidaten werden vollständig gegen das dauerhafte Log gefiltert, die Channel-Points-Erinnerung bleibt auf höchstens zwei Kandidaten begrenzt. Unit-Tests für die Kandidatenauswahl ergänzt. Fehler beim Pitch-Senden werden mit Rohfehler, Kanal, Chatter und Login protokolliert.
- Fehler beim Lesen des Pitch-Logs werden mit Rohfehler und Kanal protokolliert; in diesem Fall wird kein neuer Pitch gesendet und der bestehende Channel-Points-Pfad läuft weiter.
- Pitch-Log-Insert erfolgt nach erfolgreichem Send. Bei DB-Insertfehler ist die Einmaligkeit über Prozessneustarts nicht sicherzustellen, ohne vor dem Senden zu markieren und damit bei Sendfehlern unberechtigt dauerhaft zu sperren. Der vorhandene Fehlerpfad protokolliert den Insertfehler; keine Contract-Erweiterung vorgenommen.
- Cargo-Checks, Builds und Review-Gate ausgesetzt, bis Ressourcenaufsicht den Host-Hold aufhebt.
- Koordinatorstatusmeldung am 2026-10-01 versucht, aber nicht gesendet: Zielthread war beim Senden wieder aktiv; kein `--force` verwendet.
- Main-Merge, Produktions-DDL, Deploy, Restart und Cutover gesperrt durch TokenDB-Live-Hold.
