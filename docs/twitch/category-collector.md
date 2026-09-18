# Weltweiter Deadlock-Kategoriesammler

Verbindlicher Auftrag dieser Umsetzung: 18.09.2026, Rohchat maximal 90 Tage.
Ein parallel erstellter Entwurf ohne Löschfrist ist ausdrücklich **nicht** Bestandteil
 dieses Releases.

## Laufzeit und Sicherheit

`tb-category-collector` ist ein eigener Prozess, kein Plugin des sendenden Bots.
Er verbindet sich nur über den wiederverwendeten anonymen IRC-Transport mit
`justinfan`-Identitäten. Dessen ausgehende Protokoll-Allowlist enthält lediglich
NICK, CAP, JOIN, PART, PING und PONG; weder PASS/OAuth noch PRIVMSG, Whisper oder
Moderationsbefehle sind zugelassen. IRC-Tags und Löschereignisse werden gelesen.
Getrennte Prozess-/Warteschlangengrenzen verhindern die Weitergabe in Engagement,
Coaching oder Bot-Antworten. Bestätigte native Shared-Chat-Nachrichten zählen einmal;
weitergeleitete Kopien bleiben im Rohbestand, aber nicht im Nachrichtenvolumen.

Der Hauptbot prüft zusätzlich bei jeder Raid-Nachricht/Whisper die aktuelle
operative Partnerfreigabe. Ein Raid-Kontext allein berechtigt nicht zum Senden.
Widerrufene und unbekannte Ziele sind gesperrt. Die Änderung ist konservativ:
auch ein bislang automatisch adressierter, nicht als Partner freigegebener
Whisper-Empfänger wird nicht angeschrieben.

Der Collector verwendet ausschließlich Helix-App-Credentials. Ein Bot-Token wird
nicht geladen oder als Konfigurationsfeld akzeptiert. Mehrere dynamische IRC-Shards
teilen ein konservatives Verbindungs-/Joinbudget. Rate-Limit-Header werden über
Helix-Client-Klone hinweg berücksichtigt. Das ist keine Garantie gegen serverseitige
Drosselung und kein Beweis lückenloser Chaterfassung.

## Daten und ehrliche Messgrenzen

* Alle Sprachen der **exakt** als Deadlock aufgelösten Kategorie; keine unscharfe
  Namenssuche. Vollständig paginierte Minutenabfragen, unveränderliche Snapshots,
  Profil-/Kanalinformationen und paginierte VOD-/Clip-Metadaten. Keine Mediendateien.
* Raw-Chat mit Zeit, Kanal-/Nutzer-/Nachrichtenkennung, Text, Quelltags,
  Shared-Chat-Herkunft, Emotezahl und lokaler whatlang-Erkennung. Kurze/unsichere
  Nachrichten bleiben `und`. Konfidenz ist ein Detektorwert, keine kalibrierte
  Wahrscheinlichkeit. Besonders kurze Texte und ähnliche Sprachen bleiben oft
  unbestimmt. Es gibt keine Annahme „Stream-Sprache = Nachrichtensprache“.
* Sprache ist **nicht** Land, Wohnort oder Zuschauer-Geolokation. Viewerzahl ist
  ein öffentliches Aggregat, Chat-Schreiber sind keine vollständige Zuschauerliste.
  Follower-/Abo-Details und Chatters-Listen werden hier nicht abgefragt.

VOD-/Clip-Sweeps bearbeiten höchstens fünf Kanäle je Minute. Pro Kanal und Medientyp
wird je Sweep eine Seite bis 100 Einträge nachgezogen; Cursor und Abschlussstatus
persistieren. Nach 100 Seiten stoppt der Schutz mit unvollständigem Status, statt
unbegrenzt API-Abfragen auszulösen. Nur das von Twitch tatsächlich angebotene
Listenfenster ist erfasst; keine Garantie vollständiger historischer Archive.

Stundenaggregate entstehen aus einer transaktionalen Nachholwarteschlange, auch
nach Neustarts und verspätetem Chat. `distinct_chatters` gilt **nur** je Kanal,
Stunde und Sprache, nicht als aufsummierte globale Nutzerzahl.

Retention ist in Postgres auf 1..90 Tage begrenzt und kann vom Runtime-Datenbanknutzer
nicht verändert werden. Sie läuft auch bei `enabled=false`. Ganze Stunden werden
an der Fristgrenze abgeräumt: maximal knapp eine Stunde früher, niemals planmäßig
länger als 90 Tage. Vor Löschung wird der Stundenrollup fertiggestellt. Bei
Datenbank-/Wartungsfehlern kann die Bereinigung zurückliegen; dann sind
`last_retention_run_at`, Dienstzustand und Journal zu prüfen. Snapshots, Rollups
und Listenmetadaten bleiben erhalten. Gelöschte Chattexte werden bei empfangenen
CLEARMSG/CLEARCHAT-Ereignissen entfernt; die bereits beobachteten Aktivitätszahlen
bleiben bestehen. Während einer Verbindungslücke verpasste Löschereignisse können
nicht nachträglich garantiert werden.

Eine niedrige zweistellige GB-Zahl ist **keine Zusage**: Rohtextmenge, Indizes,
Nachrichtenrate, Metadaten und dauerhaft wachsende Snapshots müssen gemessen werden.

## Dashboard

Seite: `/twitch/dashboard-v2/kategorien-weltweit`
API: `GET /twitch/api/v2/admin/category-collector?days=7|30|90`

Die Seite verwendet die bestehende Schwarz-Gold-Shell. Der API-Endpunkt ist strikt
serverseitig adminpflichtig, auch bei Loopback/gefälschtem Host oder Forwarded-User.
Partner erhalten 403, nicht angemeldete Clients 401. Antworten sind `no-store`.
Die Dashboard-DB-Rolle hat kein SELECT auf Rohtext, Chat-Nutzerkennungen oder Tags.

Separate Stream-Sprach- und Nachrichtensprach-Auswertungen, Kategorie-Stundentrend,
Top-20-Kanäle je Sprache und UTC-Tageszeit-Heatmap. Die Anzeige behält leere
Messstunden als Lücken. Sendezeit/Zuschauerstunden werden nur zwischen zwei
benachbarten vollständigen Polls mit höchstens zwei Pollintervallen Abstand
abgeleitet. Der letzte noch nicht begrenzte Poll erhält keine erfundene Dauer.
Zuschauer-Durchschnitte der Kanäle sind zeitgewichtet. Der Betrachtungsbeginn wird
auf volle UTC-Stunden abgerundet, damit die 90-Tage-Grenze auch nach Rohdatenlöschung
mit den dauerhaften Rollups korrekt auswertbar bleibt.

Caddy hat bereits passende `/twitch/dashboard-v2/*`- und `/twitch/api/v2/*`-Matcher.
Keine breitere öffentliche Freigabe und keine öffentliche Daten-API erforderlich.

## Installation und Betrieb

1. Änderungen geprüft als Release bauen, einschließlich `tb-category-collector`.
   Der Installer verifiziert auch für diesen Prozess die eingebettete Git-Herkunft.
2. Migration `20260918123000_category_collector.sql` ausschließlich als postgres
   über `deadlock-twitch-migrate.service` anwenden. Bei bereits vorhandenem
   abweichendem Schema/Migrationschecksum abbrechen, nicht überschreiben.
   `twitch-runtime-roles.sql` richtet die eigene Rolle `twitchcollector` ein und
   entzieht dem Dashboard den Zugriff auf Rohtexte und einzelne Chat-Identitäten.
3. Einmalig App-Secrets aus dem vorhandenen Broker in eine private JSON-Datei
   übergeben, beispielsweise mit `provision-category-collector.py --output
   /home/nathanael/.config/deadlock/category-collector.json` als Kind des bestehenden
   `dl-infisical-env`-Brokers. Das ist ausschließlich der Provisionierungsschritt.
   Das Skript setzt das Passwort der dedizierten Datenbankrolle über lokales
   postgres-stdin, gibt keine Secrets aus und überschreibt keine vorhandene Datei.
4. `ops/systemd/tb-category-collector.service` nach
   `~/.config/systemd/user/tb-category-collector.service` installieren,
   `systemctl --user daemon-reload`, dann `systemctl --user enable --now
   tb-category-collector.service`. Der Dienst verwendet **keine EnvironmentFile,
   keine ENV-Konfiguration und keinen Secret-Broker zur Laufzeit**.
5. `systemctl --user status tb-category-collector.service` und `journalctl --user
   -u tb-category-collector.service` prüfen. `unit-failure-notify@` ist die vorhandene
   Ausfallbenachrichtigung. Neustartabstand 30 Sekunden, Startlimit 5/10min,
   RAM-Hardlimit256MiB, CPU-Quota ein Kern, zwei Runtime-Threads.

Die JSON-Datei enthält nur `dsn`, `client_id`, `client_secret`, muss ein reguläres
0400-/0600-File sein und darf nicht versioniert werden. Betriebseinstellungen liegen
nur in `category_collector_config`. Ein PostgreSQL-Advisory-Lock verhindert einen
zweiten gleichzeitigen Sammler. Für einen begrenzten Ersttest existiert
`--duration-seconds 90`; niemals parallel zum laufenden Dienst starten.

Read-only Belege (als berechtigte Administration, ohne Chattexte auszugeben):

```sql
SELECT status,count(*),max(completed_at) FROM category_polls GROUP BY status;
SELECT count(*),count(DISTINCT user_id) FROM category_stream_snapshots;
SELECT count(*),min(sent_at),max(sent_at) FROM category_chat_messages;
SELECT detected_lang,count(*) FROM category_chat_messages GROUP BY detected_lang;
SELECT count(*) FROM category_media_items;
SELECT * FROM category_collector_status WHERE id=1;
```

Für ein vollständiges Release sind Hauptbot, Dashboard und danach der separate
User-Dienst neu zu starten. Ein Fehler bei Migration/Berechtigungen, Einzeltests
oder Build darf nicht durch direkten Einsatz eines alten oder Dirty-Binaries
umgangen werden. Ein echter 24–48-Stunden-Nachweis beginnt erst mit dem Live-Start.

## Tests

Relevante gezielte Suites: 18 Collector-Tests (davon 9 echte, isolierte
PostgreSQL-Fälle), 3 Credential/Binary-Tests, 8 Admin-API-Tests, 11 Bot-Zielschutz-
Tests, 23 Helix-Stream-Tests, 7 anonyme Transporttests und 17 Frontend-Tests.
Die Datenbanktests starten eigene Unix-Socket-Instanzen und verwenden keine
Produktions-DSNs; kein Test wird bei fehlender Testdatenbank still übersprungen.

Der rote Raid-Test ließ vor der Korrektur ein nicht freigegebenes Ziel passieren.
Die grüne Suite sperrt dieses Ziel und prüft den Widerruf zwischen zwei Aktionen.
Der frühere russische Test erwartete trotz Detektorkonfidenz 0,56 eine Zuordnung;
die Schwelle0,60 wurde nicht gesenkt. Tests decken jetzt sowohl diese bewusst
unbestimmten Texte als auch ein sicher erkanntes russisches Textbeispiel ab.
