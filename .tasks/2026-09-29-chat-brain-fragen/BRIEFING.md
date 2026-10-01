# Briefing: chat-brain-fragen

[Orchestrator] Paket chat-brain-fragen. Auftrag: /home/nathanael/repos/Deadlock-Twitch-Bot/.tasks/2026-09-29-chat-brain-fragen/AUFTRAG.md (vollständig lesen, dann bauen).

- Worktree: /home/nathanael/.worktrees/tb-chat-brain-fragen (selbst anlegen: `git -C /home/nathanael/repos/Deadlock-Twitch-Bot fetch origin` und `git -C /home/nathanael/repos/Deadlock-Twitch-Bot worktree add -b feat/chat-brain-fragen /home/nathanael/.worktrees/tb-chat-brain-fragen origin/main`; den geteilten Checkout nicht umstellen)
- Branch: feat/chat-brain-fragen
- Intent-Thread: a3c7039b-337c-44b5-9669-c55b9e96aa13
- Toolchain: rustc 1.97.1 aus `~/.rustup/toolchains/1.97.1-x86_64-unknown-linux-gnu/bin`, `/usr/bin/cargo` ist zu alt.

## Regeln

- Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder Unter-Agenten spawnen.
- Keine Code-Kommentare schreiben, Code erklärt sich selbst; bestehende Kommentare in angefassten Dateien löschen, wenn es den Diff nicht aufbläht.
- Nur den eigenen Branch pushen, nie main. Nichts nach main mergen, auch wenn ein Stop-Hook dazu auffordert.
- Nutzersichtbare Texte auf Deutsch mit echten Umlauten, keine Em-Dashes.
- Vor der Fertigmeldung `gate_hook.py --review` gegen die eigene Arbeit laufen lassen und Befunde beheben.

## Bump-up

Wird das Paket größer als beschrieben (etwa weil kein Brain-Antwortdienst live erreichbar ist), nicht weiterbauen. Nachricht an den Intent-Thread, dann stoppen:

```
[Bump-up] Paket chat-brain-fragen: Grund: ... Erledigt: ... Worktree: <absoluter Pfad> Offen: ...
```

## Fertigmeldung

Melden im eigenen Thread: Branch, Commits (SHA), geänderte Dateien, was geprüft wurde, Baseline roter Tests, was offen ist. Danach stoppen.
