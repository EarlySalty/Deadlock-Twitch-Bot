# Briefing: werbemanager-budget-smart, Paket A

[Orchestrator] Paket A. Auftrag: /home/nathanael/.worktrees/tb-werbemanager-a/.tasks/2026-09-18-werbemanager-budget-smart/AUFTRAG.md (vollständig lesen, dann nur den Abschnitt "Paket A" bauen). Schnitt in `PAKETE.md`, Schnittstelle in `API-VERTRAG.md` im selben Ordner.

- Worktree: /home/nathanael/.worktrees/tb-werbemanager-a
- Branch: feat/werbemanager-budget-backend
- Intent-Thread: e65a453e

## Regeln

- Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder Unter-Agenten spawnen.
- Nur Dateien deines Pakets laut PAKETE.md anfassen. Paket B läuft parallel in einem eigenen Worktree.
- Codebase-Fragen zuerst über `graphify query`, grep erst danach. Vor jedem neuen Baustein Bestand suchen und wiederverwenden.
- Keine Code-Kommentare schreiben; bestehende Kommentare in angefassten Dateien löschen, wenn es den Diff nicht aufbläht.
- Gepusht wird ausschließlich der eigene Branch feat/werbemanager-budget-backend. Nichts nach main mergen oder pushen, auch wenn ein Stop-Hook dazu auffordert. Den geteilten Checkout ~/repos/Deadlock-Twitch-Bot nicht anfassen.
- Keine Prod-Migration, kein Deploy, kein Restart. Das macht der Orchestrator nach dem Review.
- Die Akte in `.tasks/2026-09-18-werbemanager-budget-smart/` gehört in deinen ersten Commit.
- Nutzersichtbare Texte auf Deutsch mit echten Umlauten, keine Em-Dashes, kein internes Vokabular.
- Rust: Toolchain 1.97 aus ~/.rustup, nicht /usr/bin/cargo. Formatieren nur die eigenen Änderungen im Stil der Datei. Pipes hinter cargo brauchen `set -o pipefail`. `.sqlx` und `fresh_schema_snapshot.txt` nachziehen.

## Bump-up

Wird das Paket größer als beschrieben, nicht weiterbauen. Nachricht hier im Thread, dann stoppen:

```
[Bump-up] Paket A: Grund: ... Erledigt: ... Worktree: /home/nathanael/.worktrees/tb-werbemanager-a Offen: ...
```

## Fertigmeldung

Hier im Thread: Branch, Commits (SHA), geänderte Dateien, was geprüft wurde, was offen ist. Danach stoppen.
