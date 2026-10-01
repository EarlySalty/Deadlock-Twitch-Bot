# Briefing: werbemanager-budget-smart, Paket B

[Orchestrator] Paket B. Auftrag: /home/nathanael/.worktrees/tb-werbemanager-a/.tasks/2026-09-18-werbemanager-budget-smart/AUFTRAG.md (vollständig lesen, dann nur den Abschnitt "Paket B" bauen). Schnitt in `PAKETE.md`, Schnittstelle in `API-VERTRAG.md` im selben Ordner. Die Akte liegt im Worktree von Paket A und wird von dir nur gelesen.

- Worktree: /home/nathanael/.worktrees/tb-werbemanager-b
- Branch: feat/werbemanager-budget-dashboard
- Intent-Thread: e65a453e

## Regeln

- Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder Unter-Agenten spawnen.
- Nur Dateien deines Pakets laut PAKETE.md anfassen. Paket A (Rust) läuft parallel; du baust gegen den API-Vertrag und wartest nicht. Fehlt `history` oder `plan` in der Antwort, zeigt die Seite einen ehrlichen Leerzustand statt zu brechen.
- Codebase-Fragen zuerst über `graphify query`, grep erst danach. Vorhandene Dashboard-Bausteine (Karten, Schalter, Klappbereiche) wiederverwenden, keine neuen Varianten daneben stellen.
- Die Strategie-Karten bleiben optisch unverändert, es werden nur zwei statt drei.
- Look: warmes Schwarz, kräftige Goldkante, Karten erhaben, Innenkacheln und Inputs versenkt, kein Glow. Der Schalter sitzt links am Titel und ist nicht zu übersehen.
- Keine Code-Kommentare schreiben; bestehende Kommentare in angefassten Dateien löschen, wenn es den Diff nicht aufbläht.
- Gepusht wird ausschließlich der eigene Branch feat/werbemanager-budget-dashboard. Nichts nach main mergen oder pushen, auch wenn ein Stop-Hook dazu auffordert. Den geteilten Checkout ~/repos/Deadlock-Twitch-Bot nicht anfassen.
- Kein Deploy. `npm run build` und Typprüfung im Worktree müssen durchlaufen.
- Nutzersichtbare Texte auf Deutsch mit echten Umlauten, keine Em-Dashes, kein internes Vokabular (kein Snooze, kein Preflight, kein Reason-Code im Klartext).

## Bump-up

Wird das Paket größer als beschrieben, nicht weiterbauen. Nachricht hier im Thread, dann stoppen:

```
[Bump-up] Paket B: Grund: ... Erledigt: ... Worktree: /home/nathanael/.worktrees/tb-werbemanager-b Offen: ...
```

## Fertigmeldung

Hier im Thread: Branch, Commits (SHA), geänderte Dateien, was geprüft wurde, was offen ist. Danach stoppen.
