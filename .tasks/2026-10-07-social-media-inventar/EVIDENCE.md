# Nachweismethode

## Fixierter Stand

Codebasis: `e0b0dbaf662d7680c4ceaa210bf15f1443693cd8`, am 2026-10-07 aus `origin/main` in einen eigenen detached Worktree übernommen. Anschließend eigener Dokumentationsbranch `docs/social-media-inventar`. Der gemeinsame Checkout mit fremden Änderungen wurde nicht bereinigt.

Graphify wurde zuerst global befragt. Die Treffer wurden an aktuellen Dateien überprüft. Der Graph enthält repoübergreifende Namenskollisionen im älteren Knotenschema; Graph-Kanten gelten deshalb als Suchhinweise, nicht als Laufzeitbelege.

## Produktive Datenbank

Datenbank `twitch_analytics`, Schema `public`. Verifiziert durch `pg_stat_activity`: aktive Verbindungen von `twitchbot` mit `application_name=tb-bot` sowie `twitchdash` und `twitchcontest` mit `application_name=tb-dashboard`. Testschemata mit Präfixen `t_sm_` und `t_dash_sm_` wurden nicht in den Produktivbestand gezählt.

Die Tabelle im Inventar stammt aus einer gemeinsamen Transaktion mit `ISOLATION LEVEL REPEATABLE READ READ ONLY`; Beginn `2026-10-07 06:13:23.975592 UTC`. Pro Tabelle wurden `COUNT(*)` und Maxima vorhandener Aktivitätsfelder abgefragt. Keine Nutzernamen, IDs, Videotitel, Clipinhalte, Transkripte oder Nachrichtentexte ausgegeben. Zugangsdaten und verschlüsselte Credential-Spalten wurden nicht selektiert.

Beispiel des Abfrageverfahrens:

```sql
BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;
SET LOCAL statement_timeout = '20s';
SELECT now() AT TIME ZONE 'utc';
SELECT count(*), greatest(max(created_at), max(completed_at), max(last_attempt_at))
FROM public.twitch_clips_upload_queue;
SELECT count(*), max(created_at) FROM public.twitch_clip_merkmale;
COMMIT;
```

`track_commit_timestamp=off`. Exakte letzte PostgreSQL-Schreibzeitpunkte sind damit nicht aus dem vorhandenen Zustand rekonstruierbar. Angegeben werden ausdrücklich die letzten gespeicherten Anwendungsaktivitätszeiten. Zeitpunkte wie `scheduled_at`, `retention_until`, `next_pull_at`, Remote-Erstellzeiten und Reportperioden wurden nicht als Schreibzeiten verwendet. In Tabellen ohne `updated_at` können spätere Änderungen ohne Aktualisierung des ausgewählten Feldes unentdeckt bleiben. `social_media_platform_auth` speichert seine Zeitfelder als Text; deren Maxima wurden nach `timestamptz` konvertiert. `twitch_clip_context_seconds` hat kein Zeitfeld.

Zusätzliche Status- und Signalzählungen wurden zwischen 06:07 und 06:10 UTC in separaten lesenden Transaktionen erhoben. Sie sind als zusätzliche Stichproben gekennzeichnet, nicht als Teil desselben Snapshots.

## Laufzeit

Systemdienste, nicht User-Dienste: `deadlock-twitch-bot-rust.service` und `deadlock-twitch-dashboard-rust.service` waren aktiv. Startzeiten: 2026-10-07 07:38:40 bzw. 07:38:37 CEST. Der Release-Symlink zeigte bei der Erhebung auf `/opt/deadlock/twitch/releases/e0b0dbaf662d7680c4ceaa210bf15f1443693cd8`.

Das normale Journal war für die Sitzung leer. Lesender Zugriff über `sudo -n journalctl` gelang. Systemd lieferte einen Teil der Nachrichten als JSON-Bytearrays; diese wurden lokal dekodiert und zusammengezählt. Rohes Journal wurde nicht in den Bericht übernommen. Fenster: 2026-10-06 00:00 bis etwa 2026-10-07 06:10 UTC.

| Modulgruppe | Journalmeldungen | Letzte UTC-Aktivität |
| --- | ---: | --- |
| `tb_social_media::clip` | 14 | 2026-10-07 05:41:18.960 |
| `tb_social_media::approval` | 27 | 2026-10-07 05:49:12.715 |
| `tb_vod_archive::worker` | 25 | 2026-10-07 06:00:47.026 |

Die Startmeldung um 05:38:46.033 UTC bestätigt: `HighlightClipper` laut Betriebskonfiguration deaktiviert. Fehlende Journalmeldungen anderer Module sind kein Beleg für fehlende Aktivität. Für diese Module werden Verdrahtung und gespeicherte Daten getrennt ausgewiesen.

## Historische Aufgaben und Refs

`git merge-base --is-ancestor` wurde mit Exit-Code geprüft. Die Mergecommits `a05a61e1` (Highlight-Erkennung) und `0608d088` (Clip-Aufbereitung) sowie die Python-Löschungen `cbcfeca2` und `dfaed810` sind Vorfahren des fixierten Stands, Exit 0. Die alten Beispielbranches und der Highlight-Worktree existierten nicht mehr.

`git cherry` unterscheidet echte verbleibende Änderungen von bereits übernommenen Patchkopien. Sieben von acht Nicht-Mergecommits von `feat/clip-social-format-20260929` sind patchgleich auf Main; allein die reine Ancestry-Differenz hätte diesen Branch irreführend als unintegriert ausgewiesen. Die verbleibenden Abweichungen müssen vor einer späteren Branchlöschung gesondert gesichert oder abgeglichen werden.

## Ergänzende Prozess- und Auslieferungsprüfung

Der periodische Fetchstart wurde im Journal explizit bestätigt: Target `tb_social_media::clip::task`, Nachricht `clip_fetch: Task startet`, 2026-10-07 05:38:46.034 UTC. Default `false` darf hier nicht als tatsächliche Abschaltung berichtet werden. Die Enrichment-Abschaltmeldung und der deaktivierte Highlight-Clipper sind für denselben Startzeitpunkt bestätigt.

Das kompilierte Hauptfrontend ist unter `/opt/deadlock/twitch/current/bot/analytics/dashboard_v2/dist` vorhanden. Die fünf JS-Dateien enthalten Socialroute, Social-API, Preview- und Titelroute. Das statische Index nennt unter anderem `index-uARA_sFq.js`; kein neuer Frontendbuild wurde ausgeführt.

Systemprozesse: `tb-bot`, `tb-dashboard`, `tb-stream-audit`, `tb-category-col`. Userunit `rs-relay.service` hatte den Rustprozess `uplink-service`; `tb-gold-glanz-preview.service` einen Nodeprozess für die temporäre Vite-Preview. Die Prozessnamen wurden ohne Argumente oder Umgebungen erhoben. Ein produktiver Social-Python-/Node-Daemon wurde in diesem abgegrenzten Bestand nicht gefunden.

Der installierte Userdienst `deadlock-stt-server.service` war inaktiv. Default-STT-Port 8791 antwortete auf drei lesende HTTP-Proben mit 401; Socketbesitzer war `deadlock-patchn`, nicht `tb-stt-server`. Daraus folgt kein STT-Erfolg. Die aktive STT-Konfiguration wurde nicht gelesen, ein möglicherweise abweichender Zielport bleibt offen. Der eigenständige Systemdienst `vod-archive.service` war ebenfalls inactive/dead; sein SQLite-Katalog existiert.

Der Auftraggeber präzisierte die Sprachregel während der Inventur: Rustpflicht für Bots, Worker und dauerhaft laufende produktive Dienste. Kleine kurz laufende Timer-Skripte und Prüf-/Einmalskripte dürfen bleiben. Deshalb ist die Python-Detektor-CLI nicht als laufender Python-Daemon ausgewiesen; ein späterer dauerhafter Worker daraus wäre ein Rust-Port.

Keine Tests, Builds, Uploads, neuen Analysen, Dienstneustarts oder Datenbankänderungen ausgeführt. Empfehlungen im Inventar sind keine ausgeführten Aufräumaktionen. Während der Erhebung wurden durch andere Aufträge weitere Änderungen auf Main übernommen; der Bericht bleibt absichtlich SHA-gebunden.
