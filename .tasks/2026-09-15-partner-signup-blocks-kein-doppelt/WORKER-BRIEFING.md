# Briefing: partner-signup-blocks-kein-doppelt

[Orchestrator] Paket partner-signup-blocks-kein-doppelt. Auftrag: /home/nathanael/.worktrees/tb-partner-signup-blocks-kein-doppelt/.tasks/2026-09-15-partner-signup-blocks-kein-doppelt/AUFTRAG.md
(vollständig lesen, dann bauen). Dieselbe Datei liegt im Worktree unter
`.tasks/2026-09-15-partner-signup-blocks-kein-doppelt/AUFTRAG.md`.

- Worktree: /home/nathanael/.worktrees/tb-partner-signup-blocks-kein-doppelt
- Branch: feat/partner-signup-blocks-kein-doppelt
- Basis-SHA: 58fb24477fa454a4ddae5d9d61b3b3addea59dfa (origin/main)
- Intent-Thread: 162dee5c-eda2-4a5a-b787-8c55a6b42179
- Bump-up und Fertigmeldung in DIESEM Thread.

## Pflichtteile

1. Referenz: Die Seite hat vier `ConfirmTypedDialog`-Blöcke ab
   `PartnerSignupBlocks.tsx:509`. Jeder Knopf setzt zuerst einen `pending*`-State
   und verlangt dann Login bzw. Tag abgetippt plus einen zweiten Klick. Die
   eigentliche Mutation steht schon in `confirmAdd` / `confirmRemove` /
   `confirmTagAdd` / `confirmTagRemove`. Hinweisboxen mit `ADD_STEPS` und
   `TAG_ADD_STEPS` bleiben. `ConfirmTypedDialog.tsx` wird auch von
   `StreamerDetail.tsx:856` genutzt, nicht löschen.
2. Scope-Zaun: exakt dieser Auftrag, nur
   `bot/admin_dashboard/src/pages/community/PartnerSignupBlocks.tsx` und die
   `.tasks/`-Dateien dieses Ordners. Kein Refactoring, kein fmt, keine anderen
   Admin-Seiten, Backend unangetastet. Andere Doku im Repo ignorieren.
3. Nur Branch `feat/partner-signup-blocks-kein-doppelt` committen und nach
   origin pushen, nie main. Der Worktree ist sauber von origin/main
   `58fb24477fa454a4ddae5d9d61b3b3addea59dfa`. Uncommitteter Zustand im
   Documents-Checkout und in anderen Worktrees ist fremd, nicht anfassen.
4. Beweisziel: In `PartnerSignupBlocks.tsx` kommt `ConfirmTypedDialog` und
   `ConfirmDialog` nicht mehr vor. Die vier Aktionen laufen beim ersten Klick.
   Pflichtfelder leer bleiben Toast. Keine Testpflicht.
5. Bump-up-Format wörtlich, an den Intent-Thread 162dee5c, dann stoppen:

```
[Bump-up] Paket partner-signup-blocks-kein-doppelt: Grund: ... Erledigt: ... Worktree: /home/nathanael/.worktrees/tb-partner-signup-blocks-kein-doppelt Offen: ...
```

## Regeln

- Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder
  Unter-Agenten spawnen.
- Keine Code-Kommentare schreiben, Code erklärt sich selbst; bestehende
  Kommentare in angefassten Dateien löschen, wenn es den Diff nicht aufbläht.
- Nur den eigenen Branch pushen, nie main. Nichts nach main mergen.
- Nutzersichtbare Texte auf Deutsch mit echten Umlauten, keine Em-Dashes.
- AUFTRAG.md liegt schon im Worktree-`.tasks/` und wird mitcommitten.

## Fertigmeldung

Melden hier im Thread: Branch, Commits (SHA), geänderte Dateien, was geprüft
wurde, was offen ist. Danach stoppen, kein Mitlaufen.
