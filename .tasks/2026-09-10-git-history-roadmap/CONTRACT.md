status: erledigt | 2026-09-10

# Contract: Git-History-Roadmap für den Twitch Bot

## Ziel

Eine automatisch aus der Git-History erzeugte Roadmap-Seite, auf der auf einen Blick
sichtbar ist, was seit Projektbeginn (2026-02-24) am Twitch Bot gebaut, gefixt und
gesichert wurde, woran aktuell gearbeitet wird, und wie sich die Arbeit über die Zeit
verteilt. Keine Handpflege: die Seite wird mit einem Befehl neu erzeugt.

## Anforderungen (user-sichtbar, prüfbar)

- REQ-1: Die Seite zeigt die gesamte Commit-Historie auf main als Zeitachse mit
  Monatskapiteln (Februar 2026 bis Erzeugungsdatum) und Kennzahlen je Monat.
- REQ-2: Jeder Commit ist nach Kategorie eingeordnet (Neu, Fix, Sicherheit, Doku,
  Tests, Pflege, Abhängigkeiten, Sonstiges) und nach Fachgebiet (Scope) gruppierbar.
- REQ-3: Die Seite zeigt eine Gesamtkarte der Subsysteme mit Zahlen zu Features und
  Fixes je Scope.
- REQ-4: Die Seite zeigt eine durchgehende Punktwolke aller Commits über die Zeit,
  farbig nach Kategorie, mit Filtern und Kurzerklärung beim Überfahren.
- REQ-5: Die kuratierten Einträge der historischen CHANGELOG.md (aus git, Stand vor
  ec5dcdc1) erscheinen als lesbare Meilensteintexte an der Stelle ihres PR-Merges.
- REQ-6: Eine Sektion „Woran wir gerade arbeitet" zeigt offene lokale Branches mit
  Alter, die Statuszeilen der features/PROJ-*.md und die offenen Punkte aus
  docs/TODO-OFFEN.md mit deren Standdatum.
- REQ-7: Die Seite ist eine eigenständige HTML-Datei ohne externe Aufrufe (läuft
  über file://) im DDC-Look (dunkel, Gold, Petrol) mit echten Umlauten und ohne
  Gedankenstriche in eigenen Texten.
- REQ-8: Neuerzeugung mit einem Befehl: `python3 tools/generate_roadmap.py`.

## Invarianten (was sich nicht ändert)

- INV-1: Keine Änderung am Repo-Inhalt außer den neuen Dateien (Generator,
  erzeugte Seite, INDEX.md-Zeile, .tasks-Artefakte). Kein Code des Bots wird
  angefasst.
- INV-2: Git-Daten werden nur gelesen, nie umgeschrieben. Historische Commit-Texte
  werden wortgetreu übernommen, auch wenn sie Gedankenstriche enthalten.
- INV-3: Keine Secrets, Tokens oder DB-Verbindungsdaten kommen auf die Seite; der
  Generator liest nur git-Metadaten, features/ und docs/TODO-OFFEN.md.
- INV-4: Die abgeschaffte CHANGELOG-Dateipflege bleibt abgeschafft: es gibt keine
  neue Handpflegedatei, der Generator liest die alte Datei aus der git-Historie.
- INV-5: Abhängigkeits-Commits (deps) sind im Filter standardmäßig ausgeblendet,
  bleiben aber über den Filter erreichbar.

## Nicht-Ziele

- Keine Veröffentlichung auf der Website oder in Discord (interne Sicht zuerst).
- Kein neuer systemd-Dienst, kein Timer, keine Anbindung an Caddy.
- Kein Commit-Diff-Anzeige, kein Code-Blame, kein Deployment-Tracking.
- Keine Anreicherung aus Datenbanken (Discord, Twitch, zentrale PG).

## Änderungsbereich

- Neu: `tools/generate_roadmap.py`, `docs/roadmap/index.html` (erzeugt).
- Neu: `.tasks/2026-09-10-git-history-roadmap/` (diese Artefakte).
- Geändert: `INDEX.md` (eine Zeile in der Schnell-Navigation).

## Verbotener Änderungsbereich

- Alles unter `bot/`, `rust/`, `website/`, `ops/`, `logs/`, `data/`.
- Git-Konfiguration, Hooks, CI-Dateien.

## Offene Produktfragen

Keine. Platzierung entscheidet der Orchestrator nach Bestand und dem Wunsch
„damit wir sehen": interne, generierte Seite im Repo; eine öffentliche Variante
für die Website wäre ein eigener Auftrag mit Inhaltsfilter (Sicherheits-Themen).

## Amendments

- entschieden von User (2026-09-10): Deploy angefordert („Deploy das mal"). Die
  Route /twitch/admin/roadmap-history geht als Caddy-Static-Route hinter das
  bestehende Admin-Session-Gate live; die früheren Nicht-Ziele (keine Caddy-
  und Dashboard-Anbindung) sind damit vom User aufgehoben. Öffentliche Seite
  bleibt unverändert.
