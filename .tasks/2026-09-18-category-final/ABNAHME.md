# Abschluss: Kategoriesammler ohne automatische Datenlöschung

## Verbindlicher Umfang

Die Nutzerkorrektur ersetzt den ursprünglichen 90-Tage-Plan: Rohchat, Snapshots und Metadaten bleiben erhalten. Speichergrenzen warnen bzw. pausieren neue Erfassung. 7/30/90 Tage im Dashboard sind ausschließlich Auswertungsfenster. Der Collector bleibt ein getrennter anonymer Leser; der Hauptbot antwortet bekannten Bots, aktiven ID-Sperren und erkannten Scam-/Spam-Auslösern nicht. Stille Zuschauer sind kein Bot-Beweis.

Integriert wurde der auf main vorhandene Collector aus `60013dfa`/`e73d02af`, nicht der ältere parallele Prototyp mit anderem Schema und anderem API-Pfad. Basis dieses Abschlusses: `f13f49c6`. Arbeitsbranch: `fix/category-final-release-20260918`.

## Änderungen

- Additive Archivmigration mit DB-Konfiguration, rein lesender alter Prune-Signatur und beschränkter gezielter Nachrichtenentfernung. Der Collector kann Rohdaten nicht frei löschen, überschreiben oder leeren. Ziel-IDs verhindern Wiederherstellung explizit entfernter Nachrichten durch verspätete Wiederholungen.
- Keine Alterslöschung im Rust-Dienst. Laufende Speicherprüfung, Warnung und Erfassungspause statt Kürzung. Keine ENV-Verhaltenskonfiguration; Bootstrap ausschließlich für Zugänge, Verhalten aus PostgreSQL.
- Reversible Antwortsperren mit stabilen Twitch-IDs, bestehendem Bot-Verzeichnis und Scam-/Spam-Erkennung. Archivierung bleibt vor den Antwortsperren. Automoderationsschalter deaktivieren den Antwortschutz nicht. Alte Befehlsausnahme für bekannte Automationsbots entfällt.
- Dashboard mit unveränderten Stundenwerten, UTC-Zeitachse, erkennbaren Datenlücken, unbekannten statt erfundenen Durchschnitten und bestätigter Archivstatusanzeige.
- Laufzeitrollen erlauben gezielte Entfernung nur durch eine eingeschränkte Funktion; Dashboard und Bot erhalten keinen Zugriff auf Rohchat.
- Unit mit Restart-Backoff; Betriebsdokumentation und Bootstrapvorlage korrigiert.

## Zusätzlicher Deploy-Blocker behoben

`20260918120000` war auf main doppelt vergeben. Die erste vollständige frische Migrationsprobe scheiterte mit SQLSTATE 23505 an `_sqlx_migrations_pkey`. Die tatsächliche Produktionsmigration heißt `werbemanager budget smart`; deren SHA384 war identisch zur unveränderten Datei. Ausschließlich die noch nicht angewandte `viewer_fairness_score`-Datei wurde zunächst auf die freie Version 121000 verschoben. Beim anschließenden Abgleich mit main lag dort bereits derselbe unabhängige Fix unter Version 124000 vor. Der integrierte Release übernimmt deshalb ausschließlich `20260918124000_viewer_fairness_score.sql`; es gibt keine zweite Kopie. Keine Produktionsprüfsumme und keine angewandte SQL-Datei wurden verändert.

Danach lief die gesamte Migrationskette in einer neuen, ausschließlich per privatem Unix-Socket erreichbaren PostgreSQL16-/TimescaleDB-Instanz durch. Der Schema-Snapshot wurde aus der echten Datenbank erzeugt; anschließend bestand der reguläre Snapshot-Abgleich erneut. Die temporäre Instanz wurde sauber beendet.

## Durchgeführte Prüfungen

Alle isolierten Datenbanktests verwenden eigene kurzlebige PostgreSQL-Instanzen, keine Produktivdatenbank und keine echten Twitch-Schreibaktionen.

| Prüfung | Ergebnis |
|---|---|
| Archiv/Alter/Budget/CLEARCHAT/Rollen | 5 bestanden |
| Speicherung, Rollups, Report, Shared-Chat-Wiederholung | 2 bestanden |
| Collector-Bootstrap und Speicherpausen | 3 bestanden |
| Antwortschutz einschließlich echter Pipeline und erhaltenem Rohchat | 12 bestanden |
| Kanal-Policy einschließlich Raid-Zielschutz | 9 bestanden |
| Anonymer IRC-Transport | 4 bestanden |
| Helix-Pagination, kein 1.200-Cap, keine Teilbestands-Nullen | 3 bestanden |
| Neuer Dashboard-Verlauf und Archivstatus | 3 bestanden |
| Vollständige frische Migrationskette und erneuter Schema-Abgleich | beide Läufe bestanden |
| Dashboard TypeScript/Vite-Produktionsbuild | bestanden |

Fokussierte Rust-Dateien wurden mit rustfmt formatiert. `cargo clippy` für Collector, Analytics, Chat und Dashboard-API lief erfolgreich; bestehende Warnungen liegen in unveränderten Funktionssignaturen bzw. Uplink-Code. Zwei Hinweise im geänderten Collector wurden korrigiert. Abschließender Wiederholungslauf und zusätzliche API-/Command-Tests werden im Live-Protokoll ergänzt.

### Ehrliche Frontend-Gesamtsuite

Unveränderte Basis `f13f49c6`: 366 Tests, 359 bestanden, 7 fehlgeschlagen. Abschlussstand: 369 Tests, 362 bestanden, dieselben 7 fehlgeschlagen. Die neuen 3 Tests sind bestanden. Die Fremdfarben des bereits integrierten Collector-Tabs wurden zusätzlich auf die vorhandene Markenpalette korrigiert; verbleibende globale Farbverstöße betreffen andere Seiten.

Die sieben bestehenden Fehler sind: drei globale Farb-/Kontrastprüfungen, drei Social-Media-Vertragsprüfungen (Übersetzungen und Routenabgleich) und ein OBS-Hilfetexttest. Diese Suite ist ausdrücklich nicht vollständig grün; Tests wurden weder deaktiviert noch abgeschwächt.

## Installation

Routen: `/twitch/kategorie` und `/twitch/api/v2/category-collector?days=7`, Navigation **Deadlock weltweit**. Admin-Authentifizierung bleibt serverseitig verpflichtend. Kein Rohchat-Endpunkt.

Die installierten Host-Deploy-Wrapper waren älter als der Collector und müssen aus dem geprüften Repository-Stand aktualisiert werden. Release aus einem sauberen eigenständigen Clone bauen, alle vier Binaries mit passender `.twitch_build`-Revision und alle bestehenden Frontends. Danach Migration und Bot-/Dashboard-Restart über den vorhandenen serialisierten Wrapper; Collector-Rollen, Credentials, Unit und eng ergänzte Caddy-Pfade über den versiegelten Release-Installer einrichten.

Dieser Bericht enthält keine erfundenen Live-Zahlen. Der tatsächliche Release-SHA, Dienststatus, Zähler, Auth-Abnahme und Grenzen der kurzen Beobachtung werden nach Durchführung in `LIVE.md` festgehalten. Eine 24–48-Stunden-Messung wird nicht aus einem Kurztest abgeleitet.
