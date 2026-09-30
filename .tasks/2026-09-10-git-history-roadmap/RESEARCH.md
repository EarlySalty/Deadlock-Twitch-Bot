status: erledigt | 2026-09-10

# Research: Git-History-Roadmap

## Befund 1: Die History ist maschinenlesbar kategorisiert

main nutzt Conventional Commits mit Scope, deutschsprachigen Subjects. Verteilung
über alle 3.331 Commits (Februar bis September 2026): fix 740, feat 627,
deps 446, merge 251, docs 202, chore 63, test 50, refactor 27, security 22,
ci 21, perf 6, build 4, Rest ohne erkennbares Präfix (Frühphase). Merge-Commits
tragen PR-Nummern im Muster `Merge pull request #NNN from EarlySalty/<branch>`.
Monatsverteilung: 2026-02: 38, 2026-03: 207, 2026-04: 251, 2026-05: 216,
2026-06: 1084, 2026-07: 672, 2026-08: 739, 2026-09: 124 (Stand 2026-09-05).
Es gibt keine Tags; Meilensteine sind PR-Merges.

## Befund 2: Kuratierte Meilensteintexte existieren in der git-Historie

Die alte `CHANGELOG.md` (angelegt a60df635, entfernt in ec5dcdc1 am 2026-08-07 mit
dem Grund „Datei-Pflege abgeschafft") enthält 424 Einträge im Format
`## #NNN — Titel` gefolgt von Problem, Änderung, Aktuelles Verhalten. Lesbar per
`git show ec5dcdc1^:CHANGELOG.md`. Die PR-Nummern der Einträge lassen sich den
Merge-Commits auf main zuordnen. Wiederverwendbar als Meilensteintexte, ohne die
abgeschaffte Dateipflege wieder einzuführen.

## Befund 3: Offene Arbeit liegt in drei Quellen

- Lokale Branches ohne Merge nach main, viele mit `feat/uplink-*` und `fix/*`
  Präfix, letzte Aktivität August/September 2026.
- `features/PROJ-*.md` mit Statuszeile in Zeile 3 (Done, Planned).
- `docs/TODO-OFFEN.md` mit Standdatum in Zeile 1 und nummerierten Abschnitten.

## Befund 4: Brand-Tokens liegen vor

`website/src/ddc-design-tokens.css` definiert ab Zeile 14 den Dunkel-Gold-Petrol-Look:
Hintergrund #1c150d/#241c11/#2c2318, Gold #c8a86b/#efd49d, Petrol #55978f/#6fb3aa,
Schriften Manrope und Sora. `bot/shared-theme/industrial-gold.css` ergänzt
Materialklänge (--parchment, --gold, --iron). Die Seite übernimmt diese Werte als
CSS-Variablen, mit Systemfont-Fallback, damit die Datei ohne Netz funktioniert.

## Entscheidungen

- Generator in Python, nur Standardbibliothek, Ausgabe eine eigenständige HTML-Datei
  mit eingebetteten Daten (JSON) und Vanilla-JS; läuft über file://.
- Zeitachse nach Merge-Datum auf main (first-parent), nicht nach Branch-Datum:
  das erzählt, was wann geliefert wurde.
- deps-Commits sind Rauschen (446 Dependabot-Commits) und im Filter standardmäßig
  aus, über den Filter zuschaltbar (INV-5).
- Historische Commit-Subjects werden wortgetreu gezeigt; eigene UI-Texte folgen der
  Doku-Redakteur-Akte (echte Umlaute, 0 Gedankenstriche, keine Absolutwörter).

## Hypothesen (nicht belegt, egal für den Bau)

- Unpräfixte Commits stammen überwiegend aus der Python-Frühphase vor der
  Conventional-Commit-Umstellung. Für die Anzeige als „Sonstiges" ausreichend.
