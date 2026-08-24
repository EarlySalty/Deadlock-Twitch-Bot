# Review: Uplink-Warteliste im Admin-Modus

status: erledigt
datum: 2026-08-25
contract: CONTRACT.md

## Vertragsabdeckung

- REQ-01/REQ-02: Die rechte Wartelistenkarte hängt ausschließlich an `authStatus.adminMode` und zeigt Twitch-ID sowie Anfragezeit.
- REQ-03: `Freischalten` wartet den Relay-Erfolg ab, entfernt den Eintrag aus dem Cache und lädt die Liste neu.
- REQ-04: Laden, Fehler, fehlendes CSRF-Token und leere Liste haben eigene sichtbare Zustände.
- REQ-05: Beide Handler prüfen das effektive `DashboardAuthLevel`; Partner erhalten 403, fehlende Authentifizierung 401.
- REQ-06: Der private OBS-Hinweis besitzt eine eigenständige Warnfläche mit Icon, Rand und Akzentlinie.
- INV-01/INV-02: Relay-Secret und erzeugter Ingest-Schlüssel bleiben serverseitig; die Browserantwort wird reduziert.
- INV-03/INV-04: Nutzer-Warteliste blieb unverändert und bestehende Tests wurden nicht abgeschwächt.

## Sicherheits- und Wirkungsprüfung

- GET-Warteliste und POST-Freischaltung prüfen Relay-Status und reichen Fehler sichtbar weiter; Wiederholung ist möglich.
- Der privilegierte POST protokolliert erst nach Erfolg Actor und Ziel-ID, ohne Secrets.
- Zwillingssuche: kein zweiter Dashboard-Adminproxy; die vorhandene Nutzeraktion ist absichtlich getrennt.
- Behobener Blocker: fehlendes Erfolgs-Auditlog.
- Akzeptierter NIT außerhalb des Änderungsbereichs: Aktivieren und Entfernen sind im Relay nicht atomar, aber idempotent wiederholbar.
- `npm audit --audit-level=high`: keine hohe Schwachstelle.

WIRKUNGSPRUEFUNG[WP-1]: 0 Befunde | Zwillingssuche: grep-belegt | Fremddienst-Pfade: 2/2 geprüft

## Testnachweis

- Rot-Gegenprobe: vor der Implementierung 9 grüne statt 11 Uplink-Strukturtests.
- `npm test`: 147 bestanden, 0 fehlgeschlagen.
- `npm run lint`: 0 Fehler; 16 vorbestehende Warnungen außerhalb des Diffs.
- `npm run build`: erfolgreich.
- `cargo test -p tb-dashboard-api --all-features uplink -- --include-ignored`: 35 bestanden, 0 fehlgeschlagen.
- `cargo check -p tb-dashboard-api --all-features`: erfolgreich.
- `git diff --check`: erfolgreich.

## Veröffentlichung

Kein Community-Post: Die neue Karte ist ein Admin-Werkzeug; die Warnbox ist nur kleiner Uplink-Feinschliff.
