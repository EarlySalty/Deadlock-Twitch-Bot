# Pakete

| Paket | Inhalt | Status |
|---|---|---|
| Bestand | Nebenrepo, SQLite und aktiven Export abgleichen | erledigt |
| Archiv | zentrale Config, Playlist, manuelles Drive-Ziel, sichere temporäre Bereinigung | umgesetzt, Prüfung offen |
| Verwaltung | bestehende Social-Shell, Liste und Aktionen, Session-Scope | umgesetzt, Prüfung offen |
| Integration | origin/main übernehmen, Verbindungsmechanik von A berücksichtigen | offen |
| Abschluss | Gate, Merge, Migration, Release, Live-Beweis, Altdateien sichern und entfernen | offen |

## Entscheidungen

Keine SQLite-Zeile wird importiert: Die 63 IDs des tatsächlichen Hauptkatalogs stehen bereits in Postgres. Die Inventurzahl von 1.824 beschreibt nicht den aktuell gefundenen Hauptkatalog.
Der separate EventSub-Drive-Export wird nicht mehr gestartet. Drive ist eine ausdrückliche Wahl pro VOD innerhalb des Archiv-Workers.
YouTube-Playlist-Aufnahme wird idempotent nachgeholt. Fehlende Playlist-Rechte verursachen einen sichtbaren Uploadfehler, keinen zweiten Video-Upload.
Die neuen API-Pfade liegen unter den vorhandenen Caddy-Mustern `/social-media/*` und `/social-media/api/*`. Die UI liegt unter `/social-media-admin?view=archiv`. Links zu YouTube und Drive sind Navigation, keine neuen CSP-Connect- oder Frame-Ziele.
