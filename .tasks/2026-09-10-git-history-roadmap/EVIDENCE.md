status: erledigt | 2026-09-10

# Evidence: Git-History-Roadmap

Belege je als Pfad:Zeile oder git-Befund, geprüft am 2026-09-10 im Repo
Deadlock-Twitch-Bot (HEAD 020f1c91).

1. `features/PROJ-1-partner-raid-score-cache.md:3` — Statuszeile `## Status: 🔵 Done`,
   gleiche Struktur in PROJ-2 (Planned), PROJ-3 (Planned), PROJ-4 (Planned).
   Parsbar mit einer Zeile.
2. `docs/TODO-OFFEN.md:1` — `# Offene Punkte (Stand 2026-07-16)`, nummerierte
   Abschnitte `## N. Titel` darunter; Standdatum steht im Titel.
3. `website/src/ddc-design-tokens.css:14` bis `:22` — Farb-Tokens `--color-bg:
   #1c150d`, `--color-web-primary: #c8a86b`, Petrol-Akzent #55978f; Schriften
   Manrope und Sora im Import-Kommentar.
4. `bot/shared-theme/industrial-gold.css:11` bis `:31` — Materialfarben
   (--parchment #E3D4B6, --gold #C5A059, --iron #1F1815) als Ergänzungstonlage.
5. `INDEX.md:5` — Tabelle „Schnell-Navigation", Anknüpfung für den Verweis auf die
   erzeugte Seite.
6. git-Befund: `git log --oneline main` zeigt Conventional-Commit-Muster
   (`fix(chat):`, `feat(lurker-steuer):`), Merge-Subjects mit PR-Nummern
   (`Merge pull request #857 from EarlySalty/feat/uplink-neubau-dashboard`).
7. git-Befund: `git show ec5dcdc1^:CHANGELOG.md` liefert die historische kuratierte
   Changelog-Datei mit 424 Einträgen (`## #NNN — Titel`, Problem, Änderung,
   Aktuelles Verhalten); entfernt in ec5dcdc1 „CHANGELOG.md entfernt:
   Datei-Pflege abgeschafft (2026-08-07)".
