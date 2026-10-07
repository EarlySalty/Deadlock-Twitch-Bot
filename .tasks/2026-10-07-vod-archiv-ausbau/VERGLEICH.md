# Funktionsabgleich

Vergleichsbasis: Nebenrepo `5342074751f082b92ed573b7904e96a7b92ed345`, Bot `9315b3cf7e4a6feeffc32b603db26eca78b03a2c`.

| Funktion aus dem Nebenrepo | Bot-Bestand | Ergebnis |
|---|---|---|
| VOD-Erkennung, laufenden Stream auslassen | Helix mit Pagination und ID-Prüfung | wiederverwendet |
| Download-Retries, Fragment-Retries, Bandbreite und Timeout | vorhanden, zusätzlich ffmpeg-Ausweichweg | wiederverwendet |
| Freiplatzschutz | vorhanden | wiederverwendet |
| Schnitt bei 11,5 Stunden | vorhanden | wiederverwendet |
| Datum, Titelvorlage und Sichtbarkeit | vorhanden, je Streamer | wiederverwendet |
| Verschlüsselte Kontozugänge, eigener Kanal | vorhanden in Postgres | wiederverwendet |
| Upload-Fortsetzung und Fehler-Retry | vorhanden, mit Byte-Offset und fertiger Resume-ID | wiederverwendet |
| Projektkontingent und Laufgrenzen | vorhanden, pro Teil statt pro VOD | wiederverwendet |
| Optionale Playlist | Einstellung unverdrahtet | idempotente Aufnahme ergänzt |
| Lokale Bereinigung | optional nach Tagen, standardmäßig aus | nach bestätigtem Upload, YouTube-Verarbeitung wird geprüft |
| Einmallauf unter eigener Unit | eigener Bot-Worker vorhanden | Nebenprozess entfällt |
| SQLite-Katalog | Postgres-Katalog vorhanden | kein Import notwendig |

## Datenabgleich vom 7. Oktober 2026

`~/vod-archive/state/vods.db`: 63 VODs, 62 Teile. Sämtliche VOD-IDs stehen bereits in Postgres. Alte SQLite-Statuswerte werden nicht über den laufenden Stand geschrieben. Zwei Clip-Lab-Kopien mit 47 VODs sind ebenfalls historische Duplikate.

Postgres: 82 VODs, 80 Teile, 77 bestätigte Teile mit YouTube-ID. Drei archivierte Einträge besitzen offene Teile ohne Video-ID: `v2880484237`, `v2881260675`, `v2881297447`. Die Verwaltungsseite stellt sie nicht als saubere YouTube-Sicherung dar. Kein automatisches erneutes Hochladen dieser historischen Einträge.

## Sicherung und Löschvorschlag

Git-Stand des Nebenrepos: `5342074751f082b92ed573b7904e96a7b92ed345`. Vorherige SHA-Sicherung: `37eecd5762198a0d4b10878beb6a5b6f2d4ee404`, siehe `/home/nathanael/Documents/.tasks/2026-10-01-workspace-abschluss/twitch-uplink/vod-archive.sha-backup.tsv`.

Haupt-SQLite SHA256: `41cb3e061acc740b04c7d489c9fa9bd7cc34dde13068757675840cc296488be1`.
Clip-Lab-Kopien SHA256: `baba82383ad643ccda355b0471391c1761b652226049a480681ee886e388f7a5`.

Das Nebenrepo wird in diesem Auftrag nicht gelöscht. Nach bestätigter Zusammenführung und gesicherter Wiederherstellung kann der Nutzer die Löschung freigeben. Eigene Unit, Timer und Legacy-Python werden vorher separat gesichert und entfernt.
