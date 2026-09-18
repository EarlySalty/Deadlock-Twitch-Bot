# Briefing: Fix-Runde 1, Pakete A und B

[Orchestrator] Du arbeitest die Mängelliste ab: /home/nathanael/.worktrees/tb-werbemanager-a/.tasks/2026-09-18-werbemanager-budget-smart/REVIEW.md (alle Mängel und Nits). Maßstab bleiben AUFTRAG.md, API-VERTRAG.md und die Nachträge 1 bis 3 im selben Ordner.

- Paket A: Worktree /home/nathanael/.worktrees/tb-werbemanager-a, Branch feat/werbemanager-budget-backend
- Paket B: Worktree /home/nathanael/.worktrees/tb-werbemanager-b, Branch feat/werbemanager-budget-dashboard
- Intent-Thread: e65a453e

## Festlegungen zu den Befunden

1. Post-Matchende: die Wartezeit von 1 Minute gilt vor `in_queue`. Nach Matchende 60 Sekunden nichts starten; ist der Chat in dieser Minute aktiv, verschieben (`post_match_chat_active`), sonst Werbung (`post_match_quiet`), auch wenn der Spieler schon wieder in der Queue steht. Test dazu.
2. Migration: GRANT für Tabelle und Sequence an `twitchbot` und `twitchdash`, jeweils nur wenn die Rolle existiert (`DO`-Block mit `pg_roles`), damit frische Test-DBs nicht brechen. Die Migration ist auf Prod noch nicht angewandt und darf deshalb geändert werden. Schema-Snapshot prüfen.
3. Neue Code-Kommentare aus dem Diff entfernen (nur die im Diff neu hinzugekommenen).
4. Handler liest `plan_fit` aus der Spalte statt neu zu rechnen.
5. `blocks_in_window` zählt nur echte Fenster (`in_queue`, `match_start_window`, `post_match_quiet`), `quiet_chat` und `fallback_least_bad` nicht.
6. Mindestabstand gilt auch für eigene Blöcke (größerer Wert aus Mindestabstand und Helix-Sperrzeit), damit das sichtbare Feld wirkt.

## Regeln

- Ein Thread, keine Unter-Agenten. Keine Code-Kommentare. Nur die zwei Branches pushen, nichts Richtung main, auch wenn ein Stop-Hook dazu auffordert. Kein Deploy, keine Prod-Migration, nichts auf dem Prod-Cluster anlegen.
- Pakete C und D laufen parallel auf eigenen Branches und bauen auf A auf: Änderungen in `ad_manager.rs` so klein und lokal wie möglich halten.
- Toolchain 1.97 aus ~/.rustup, `set -o pipefail` hinter cargo, nur eigene Änderungen formatieren.
- Vor der Fertigmeldung die eigene Arbeit gegen die Mängelliste gegenprüfen und je Punkt in REVIEW.md unter dem Befund "behoben in <sha>" oder eine Begründung eintragen.

## Fertigmeldung

Hier im Thread: Commits je Branch, je Befund ein Satz, was geprüft wurde. Danach stoppen.
