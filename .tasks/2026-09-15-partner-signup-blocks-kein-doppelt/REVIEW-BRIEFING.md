# Briefing: Review R1 partner-signup-blocks-kein-doppelt

[Orchestrator] Review-Runde 1. Du fixt nicht. Du schreibst die vollständige
Mängelliste nach REVIEW.md mit `pfad:zeile`.

- Auftrag: /home/nathanael/.worktrees/tb-partner-signup-blocks-kein-doppelt/.tasks/2026-09-15-partner-signup-blocks-kein-doppelt/AUFTRAG.md
- Diff TSX: /home/nathanael/.worktrees/tb-partner-signup-blocks-kein-doppelt/.tasks/2026-09-15-partner-signup-blocks-kein-doppelt/DIFF-tsx.patch
- Worktree: /home/nathanael/.worktrees/tb-partner-signup-blocks-kein-doppelt
- Branch: feat/partner-signup-blocks-kein-doppelt
- Commit: aa16a6c5acd6dde1531bd7fae764fa0237b3aee6 gegen origin/main 58fb2447
- Ziel-Datei: /home/nathanael/.worktrees/tb-partner-signup-blocks-kein-doppelt/.tasks/2026-09-15-partner-signup-blocks-kein-doppelt/REVIEW.md
- Intent-Thread: 162dee5c-eda2-4a5a-b787-8c55a6b42179

## Auftrag in einem Satz

Auf der Admin-Seite Partneraufnahme passieren Ausschließen, Tag sperren und
beide Aufheben-Knöpfe beim ersten Klick. Kein Dialog, kein Abtippen, kein
zweiter Bestätigen-Knopf. Hinweisboxen und Toasts bleiben.

## Prüfumfang

Nur `bot/admin_dashboard/src/pages/community/PartnerSignupBlocks.tsx` plus ob
der Diff den Scope-Zaun hält. `ConfirmTypedDialog.tsx` und `StreamerDetail.tsx`
müssen unangetastet sein.

## Pflichtfragen

1. Sind alle vier `ConfirmTypedDialog` und der Import weg?
2. Laufen die vier Aktionen wirklich beim ersten Klick, ohne `ConfirmDialog`
   als Ersatz?
3. Bleibt die Validierung (leerer Login/Grund/Tag = Toast, kein Speichern)?
4. Bleiben Hinweisboxen (`ADD_STEPS`, `TAG_ADD_STEPS`) und Toasts?
5. Sind Knöpfe während `isPending` disabled?
6. Gibt es einen Zwilling (zweite Bestätigung woanders auf dieser Seite, oder
   ein Dialog der noch hängt)?
7. Kann ein Doppelklick vor `isPending` zwei Mutationen feuern?

## Ausgabe

Schreibe REVIEW.md mit status-Zeile, dann je Befund:

```
- `pfad:zeile`: Schwere <block|fix|hinweis>. Was falsch ist. Was tun.
```

Wenn nichts Fixbedürftiges da ist, schreibe ausdrücklich `Mängel: keine` und
hör auf. Keine Unter-Threads. Keine Code-Edits. Fertigmeldung in DIESEM Thread,
dann stoppen.
