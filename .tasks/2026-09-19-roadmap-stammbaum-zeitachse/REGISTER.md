# Register: Roadmap-Stammbaum mit echter Zeitachse

- Intent-Thread: bc12cdd2-eefd-4fcc-8f17-2f957a14b8dd
- Worktree: /home/nathanael/.worktrees/tb-roadmap-stammbaum
- Branch: feat/roadmap-stammbaum-zeitachse
- Status: fertig, wartet auf Review. Kein Merge, kein Deploy.

## Was gebaut wurde

Die Swimlane-Tabelle (Generationen links, Monatsspalten rechts) ist durch eine
maßstäbliche Zeitachse ersetzt. Kern:

- `model.js`: neue `layoutFamily` mit Tag-genauer Zeitskala (`timeX`), Stamm,
  Ast-Abzweig am ersten Nachweis, Bahnen-Packing mit Wiederverwendung,
  Import-Woche als „Bestand beim Start“ (am globalen Import verankert),
  Meilenstein-Bündelung nach 3-Tage-Lücke (`bundleCommits`), Wochenstriche
  (`weeksBetween`), Astdicke/Helligkeit nach Aktivität, ruhende Äste laufen aus.
- `app.js`: SVG-Rendering von Stamm, Ästen, Abzweig-Kurven, Wochen-/Monatsraster;
  Meilensteine als Punkte mit Hover-Tooltip; Astbeschriftung mit Klapp-Caret;
  Meilenstein-Klick öffnet das komplette Bündel in der Detailleiste
  (`currentBundle`, „Ganze Funktion zeigen“).
- `features.json`: Sammeleimer verfeinert. Neue Unterfeatures für dashboard
  (admin-console, dashboard-shell), social (clips, vod, publish), raids (scoring,
  analytics, execution), runtime (db, deploy, core), moderation (scam, bans),
  partners (partner-profiles). 38 → 52 Einträge. `other` bleibt nur Liste.
- `style.css`, `index.html`: Zeitachsen-Optik, Legende, Methodentext.
- Tests: `model.test.mjs`, `family.test.mjs`, `browser.test.mjs` nachgezogen;
  `test_history.py`, `test_family.py` bleiben grün.
- `README.md`: neuer Aufbau beschrieben.

## Geprüft

- 31 Python-Tests, 29 Node-Tests, 14 Browser-Prüfgruppen grün.
- Datenstand origin/main `bca7e4d4`, 3.677 Commits, 52 Taxonomie-Einträge.
- Layout: 51 Äste, 463 Meilensteine, 2.966 gebündelte Änderungen, keine
  Label-Überschneidung, Astwurzeln treffen die Elternbahn, „other“ kein Ast.
- Screenshots in `screenshots/`: gesamtansicht, fokus-uplink, fokus-raids,
  meilenstein-buendel, mobil. Visuell kontrolliert.

## Offen

- Merge, Deploy und Live-Prüfung macht der Orchestrator nach dem Review.
