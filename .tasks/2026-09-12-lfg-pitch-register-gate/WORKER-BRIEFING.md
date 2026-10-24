# Briefing: lfg-pitch-register-gate

[Orchestrator] Paket lfg-pitch-register-gate (A). Auftrag:
/home/nathanael/repos/Deadlock-Twitch-Bot/.tasks/2026-09-12-lfg-pitch-register-gate/AUFTRAG.md
(vollständig lesen, dann bauen).

- Worktree: /home/nathanael/.worktrees/tb-lfg-pitch-register-gate (anlegen mit
  `git -C /home/nathanael/repos/Deadlock-Twitch-Bot worktree add /home/nathanael/.worktrees/tb-lfg-pitch-register-gate -b fix/lfg-pitch-register-gate origin/main`)
- Branch: fix/lfg-pitch-register-gate
- Intent-Thread: siehe REGISTER.md im selben Ordner
- PATH: `/home/nathanael/.rustup/toolchains/*1.97*/bin` vor `/usr/bin`, nie `/usr/bin/cargo` (1.75).

## Regeln

- Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder
  Unter-Agenten spawnen.
- Keine Code-Kommentare schreiben, Code erklärt sich selbst; bestehende
  Kommentare in angefassten Dateien löschen, wenn es den Diff nicht aufbläht.
- Nur den eigenen Branch pushen, nie main. Nichts nach main mergen.
- Nutzersichtbare Texte auf Deutsch mit echten Umlauten, keine Em-Dashes.
- Nichts in `/home/nathanael/repos/Deadlock-Twitch-Bot` direkt ändern (geteilter Checkout).
- Formatieren nur eigene Dateien per `rustfmt <datei>`, kein repoweiter Formatterlauf.

## Bump-up

Wird das Paket größer als beschrieben, nicht weiterbauen. Nachricht in diesen
Thread, dann stoppen:

```
[Bump-up] Paket A: Grund: ... Erledigt: ... Worktree: <absoluter Pfad> Offen: ...
```

## Fertigmeldung

Melden hier im Thread: Branch, Commits (SHA), geänderte Dateien, Testlauf mit
Baseline und Endstand (Zahlen), was offen ist. Danach stoppen, kein Mitlaufen.
