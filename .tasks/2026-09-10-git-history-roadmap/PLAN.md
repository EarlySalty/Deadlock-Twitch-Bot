status: erledigt | 2026-09-10

# Plan: Git-History-Roadmap

Ziel, Anforderungen und Invarianten stehen in CONTRACT.md in diesem Ordner.

## Milestone 1: Generator schreiben

Änderungen: `tools/generate_roadmap.py` neu. Der Generator sammelt per git nur
Metadaten: Commits auf main (Hash, Datum, Typ, Scope, Subject, PR-Nummer bei
Merges), offene lokale Branches mit letztem Commit-Datum und Abstand zu main,
Statuszeilen aus features/PROJ-*.md, Abschnitte aus docs/TODO-OFFEN.md, und die
historische CHANGELOG.md aus `git show ec5dcdc1^:CHANGELOG.md`. Erzeugt wird
`docs/roadmap/index.html` als eigenständige Datei mit eingebettetem JSON,
Vanilla-JS und SVG, DDC-Farbtokens, ohne externe Aufrufe.

Erwarteter Zwischenzustand: Skript läuft ohne Fremdpakete mit Python 3.

Validierung: `python3 tools/generate_roadmap.py` endet mit Exit 0 und schreibt
`docs/roadmap/index.html`; die Datei enthält die Kennzahlenzeile mit der
Gesamtzahl 3.331 Commits und einen Monatskapitel-Kopf für 2026-02.

Stop-Regel: Exit ungleich 0 oder fehlende Datei, dann fixen, nicht weiterbauen.

## Milestone 2: Seite im Browser verifizieren

Änderungen: keine am Code, außer falls die Verifikation Fehler findet.

Erwarteter Zwischenzustand: Seite lädt über file:// in Brave.

Validierung: Desktop- und Mobilansicht gesichtet; Kategorie-Filter schalten
sichtbar durch (inkl. deps-Zuschalten); Monatskapitel klappen auf; Punktwolke
zeigt Kurzerklärungen beim Überfahren; Subsystemkarten zeigen Zahlen; die Sektion
„Woran wir gerade arbeitet" listet Branches und Projekte. Keine Konsolenfehler.

Stop-Regel: Ein gefundener Fehler wird gefixt und die Prüfung läuft erneut,
bevor der Milestone als fertig gilt.

## Milestone 3: Verdrahtung und Abschluss

Änderungen: `INDEX.md` bekommt eine Zeile in der Schnell-Navigation auf
`docs/roadmap/index.html`. PLAN.md und CONTRACT.md-Status werden auf erledigt
gesetzt.

Erwarteter Zwischenzustand: Verweis von der Repo-Indexseite aus auffindbar.

Validierung: `grep roadmap INDEX.md` trifft; erzeugte Seite vorhanden.

Stop-Regel: Kein Merge, kein Push in dieser Aufgabe; Übergabe an den User als
Arbeitsstand.

## Milestone 4: Deploy (nachgezogen auf User-Wunsch)

Änderungen: Caddy-Route im Repo /home/nathanael/Documents/Caddy (Commit
c5a913f), Deploy-Skript ops/deploy-roadmap-history.sh, Live-Deploy durch den
User per Skript am 2026-09-10. Ausgelieferte Kopie: /srv/deadlock-roadmap.

Validierung: Gate-Probe 302 auf Discord-Login, Panel-Regression 302,
Journal -p err leer, Config enthält Route, Bytes identisch zur geprüften
Seite (sha256 47fe999c).

Befund (nicht von dieser Änderung verursacht): die öffentliche Seite
/twitch/roadmap liefert 404, weil der Public-Host-Block den Pfad nicht an das
Backend weiterreicht. Vorher-Befund, separat zu entscheiden.
