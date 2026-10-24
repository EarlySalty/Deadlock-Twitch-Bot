# Delegator Community (D2)

Zuerst lesen: `/home/nathanael/repos/Deadlock-Brain/.tasks/2026-10-03-brain-fertigstellung/DELEGATOR-REGELN.md` (gilt vollständig).

## Bereich

Community-Streamer-Brücke live: Twitch PR #1035 und Bots PR #472 (Zuschauer-Punkte, Clip-Einreichung, Community-Scouting, Twitch-Verknüpfung, Punkte-Leaderboard, Clip-Contest). Serverguide, Brain-Anteile und C9-Erweiterungen gehören nicht dazu.

Dein Bereichsordner ist dieser Ordner. `AN_HAUPT.md`, `VON_HAUPT.md` und `REGISTER.md` liegen hier.

## Worker

Ein Worker für beide Repos, weil Schreib- und Lesepfad zusammengehören: Briefing-Entwurf `BRIEFING.md` in diesem Ordner, T3-Projekt Deadlock-Twitch-Bot. Auftraggeber auf dich ändern, Statuskanal `AN_D2.md`. Gate-Befunde gehen an einen frischen Fixer-Worker.

## Vor dem ersten Start

Die alten Threads wurden angehalten und schreiben ihren Stand in eine `STAND.md` in ihrem Taskordner: Twitch-Producer `3a7b1c91-7837-4fa0-a927-f275f30d9368`, Bots-Consumer `c0b1d111-e402-4ed3-bb0b-68958a1ba699`, früherer Hauptabschluss `64cab234-aaf8-48e0-ab7c-440c2468a199`. Worktrees erst anfassen, wenn diese Threads nicht mehr `running` sind. Den STAND.md-Pfad findest du in deren letzter Nachricht (`t3-thread.py read`). Schreib diesen Threads nichts. Ältere Handoffs zum Nachschlagen: `/home/nathanael/Documents/.tasks/2026-10-02-offene-branches/HANDOFF-ORCHESTRATOR-2026-10-03.md`; deren Wrapper-, Manifest- und Lockvorgaben gelten nicht mehr.

## Fertig

Beide PRs gemergt (Arbeitsbranch nach main, PRs danach schließen), Migrationen angewandt, beide Dienste deployt und neu gestartet, Leaderboard und Punkte-Sync live belegt, Worktrees nach SHA-Backup gelöscht.
