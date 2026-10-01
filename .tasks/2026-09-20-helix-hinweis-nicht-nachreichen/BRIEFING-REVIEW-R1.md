# Briefing: Review Runde 1, helix-hinweis-nicht-nachreichen

[Orchestrator] Paket A ist fertig gemeldet. Du bist Reviewer Runde 1
(`review_1`). Du fixt nicht selbst. Du schreibst die vollständige
Mängelliste nach
`/home/nathanael/.worktrees/tb-helix-hinweis-stale/.tasks/2026-09-20-helix-hinweis-nicht-nachreichen/REVIEW.md`
mit `pfad:zeile`.

- Worktree: /home/nathanael/.worktrees/tb-helix-hinweis-stale
- Branch: fix/helix-hinweis-nicht-nachreichen
- Basis: origin/main (`818e21523b13648a26e76f150aba89ae1ad19a89`)
- Diff: `git diff origin/main...HEAD` im Worktree
- Auftrag: `.tasks/2026-09-20-helix-hinweis-nicht-nachreichen/AUFTRAG.md`
- Intent-Thread: 65d5c809-b313-4fec-b278-c94f43f7453b
- Worker-Thread: 86e36c05-e5b4-4cb6-b936-4a1583ddc9e2 (glm-5.3-flash)
- Commits: `dfba3d06` Fix, `e3b4cbc9` Auftrag im `.tasks/`-Ordner
- Geteilter Checkout `/home/nathanael/repos/Deadlock-Twitch-Bot` nicht anfassen.

## Auftrag in einem Satz

Abgelegte Helix-Ausfälle dürfen nach Reboot nicht als aktuelle Störung
nachgereicht werden. Der Live-Loop bleibt Quelle der Wahrheit. Die
System-Unit startet nach Reboot, weil sie enabled ist.

## Was der Worker gemeldet hat

- `offene_hinweise_senden` löscht `helix-ausfall.json` wie `start-*`, kein
  `dm_rohtext`.
- Admin-DM in `helix_ausfall_text()` mit echten Umlauten (`Anläufen`).
- Test `ein_alter_helix_ausfall_wird_nicht_verspaetet_nachgereicht`.
- Doku-Absatz Fehlerverhalten.
- Tests: Baseline 47 passed, Endstand 48 passed, 0 failed, 0 ignored.
- Push auf `origin HEAD:fix/helix-hinweis-nicht-nachreichen`.

## Prüfauftrag

Vollständiger Diff gegen den Auftrag. Alles Fixbedürftige mit `pfad:zeile`
in `REVIEW.md`. Keine Runde prüft den eigenen Bau; das laufende
`gate_hook.py --review` des Workers zählt nicht als diese Runde.

Scope-Zaun des Auftrags: nur `rust/bin/tb-stream-audit/src/main.rs` und
der Architektur-Absatz Fehlerverhalten, plus `.tasks/` als Register.
Keine Helix-Timeouts, keine Mitschnitte, keine anderen Hinweisarten.

## Ausgabe

`REVIEW.md` im Aufgabenordner, Kopf `status:` und Datum. Urteil: keine
Mängel / Mängel mit Liste. Danach stoppen, nicht mergen, nicht nach main
pushen.

## Regeln

- Du bist der einzige Thread für dieses Review. Keine Unter-Threads oder
  Unter-Agenten spawnen.
- Keine Code-Kommentare schreiben.
- Nur den eigenen Branch anfassen, nie main.
- Nutzersichtbare Texte auf Deutsch mit echten Umlauten, keine Gedankenstriche.
