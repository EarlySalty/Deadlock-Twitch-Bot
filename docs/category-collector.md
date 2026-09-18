# Globaler Deadlock-Kategoriesammler

Der eigene Rust-Dienst `tb-category-collector.service` liest die gesamte über Helix sichtbare Live-Kategorie ohne Sprachfilter. Er ist kein Twitch-Chatbot: Seine IRC-Komponente nimmt keine Zugangsdaten entgegen und besitzt keine Sende-, Whisper- oder Moderationsschnittstelle. Der vorhandene Scout verwendet dieselbe gemeinsame anonyme Lesekomponente.

## Daten und Grenzen

Minütliche, vollständig paginierte Stream-Snapshots enthalten Kanal- und Stream-IDs, Titel, Zuschaueraggregat, Stream-Sprache, Startzeit, Tags, Vorschaubild und Mature-Kennzeichnung. Öffentliche Kanalprofile werden täglich ergänzt. Der vorhandene Bot-Prozess ergänzt Follower-Gesamtzahlen mit seinem bereits verwalteten User-Token; es gibt keinen zweiten Token-Refresh-Besitzer. Einzelne Follower, Subscriber, vollständige Chatters-/Lurker-Listen und Zuschauerländer werden nicht abgefragt.

Chat wird über `justinfan` gelesen, nach Nachrichten-ID dedupliziert und mit Herkunft, Zeit, Tags, Emotezahl und lokaler Sprachdetektion gespeichert. Nachrichten unter 20 Buchstaben sowie unsichere Erkennungen bleiben `und`. Stream-Sprache und erkannte Nachrichtensprache sind getrennte Merkmale. Sprache ist ausdrücklich keine Geolokation. Shared-Chat-Kopien bleiben als solche erkennbar und werden nicht mehrfach in die Volumensummen aufgenommen. Beobachtete CLEARMSG/CLEARCHAT-Ereignisse entfernen Rohzeilen und korrigieren Stundenaggregate.

Optional werden ausschließlich VOD-/Clip-Metadaten gesammelt, keine Medien heruntergeladen. Clips werden in einem gleitenden Sieben-Tage-Fenster abgefragt; verfügbare VOD-Seiten werden nachgezogen. Cursor und Wiederaufnahme sind persistent. API-Seitenlimits und wiederholte Cursor werden als unvollständig protokolliert. VODs besitzen keine verlässliche kategorieweite Zuordnung und werden deshalb nicht automatisch als Deadlock-Videos ausgegeben.

## Speicherung und Retention

PostgreSQL-Datenbank: `twitch_analytics`. Alle neuen Tabellen beginnen mit `category_`. Rohchat liegt in täglichen UTC-Partitionen. Stundenaggregate werden über gesperrte Dirty-Buckets idempotent neu berechnet. Abgelaufene Partitionen werden erst nach ihrer Aggregation entfernt; dadurch wird tatsächlicher Plattenplatz zurückgegeben.

Die reguläre Rohchat-Retention beträgt maximal 90 Tage. Die Beispielkonfiguration setzt zusätzlich ein Rohdatenbudget von 20 GiB. Bei Budgetdruck können bereits aggregierte, abgeschlossene Tage früher entfernt werden. Reicht das nicht, pausiert die Rohspeicherung; die Oberfläche zeigt Einschränkung und verworfene Ereignisse. Deshalb sind 90 Tage kein garantierter Mindestbestand und 20 GiB keine harte Grenze für die gesamte Datenbank: Snapshots, Indizes, WAL, Metadaten und dauerhafte Rollups belegen zusätzlichen Platz. Wartungsintervalle können eine begrenzte Überschreitung verursachen.

Snapshots und Rollups bleiben bestehen. Sendestunden/Zuschauerstunden sind aus erfolgreichen Messintervallen geschätzt. Längere Abruflücken werden nicht als beobachtete Sendezeit hochgerechnet. Fehlgeschlagene oder unvollständige API-Abfragen erzeugen keinen falschen Null-Snapshot. Stündlich eindeutige Schreiber sind keine global eindeutigen Zuschauer und werden nicht als solche summiert.

## Konfiguration und Diensttrennung

Es gibt keine ENV-Konfiguration für den Collector. Der Start ist explizit:

```sh
tb-category-collector --config /etc/deadlock-twitch/category-collector.json
```

Eine vollständige Vorlage liegt unter `ops/systemd/category-collector.example.json`. Zugangsdaten kommen produktiv aus einem hostverschlüsselten systemd-Credential mit ausschließlich Twitch-App-ID und App-Secret. Der Collector erhält weder Bot-Token noch Infisical-Bootstrap- oder Benachrichtigungs-Token.

Die systemweite Unit läuft als eigener Benutzer `twitchcollector`, analog zur bestehenden isolierten Twitch-Laufzeit, mit Peer-Postgres-Rolle, 512 MiB Speicherlimit, CPU-Begrenzung und Restart-Backoff. Die Rollenmatrix erlaubt nur die eigenen Tabellen und zwei eng begrenzte Partitionsfunktionen. Dashboard und Legacy-Rollen dürfen keinen Rohchat lesen. Der Bot darf ausschließlich Kanalstammdaten lesen und Followerfelder aktualisieren. Eine separate OnFailure-Unit benachrichtigt den bereits konfigurierten Betreiber über den lokalen Discord-Broker.

## Dashboard und Betrieb

Die serverseitig adminpflichtige Seite ist `/twitch/kategorie`; die Datenroute ist `/twitch/api/v2/category-collector?days=7`, mit ausschließlich 7, 30 oder 90 Tagen. Sidebar: **Deadlock weltweit** im Admin-Modus. Die Oberfläche zeigt Sprachen, Kategorie-Trend, Top-Kanäle nach Zuschauerstunden, Chat-Tageszeiten in UTC sowie Messabdeckung und Speicherwarnungen. Es existiert kein Rohchat-API-Endpunkt.

Release-Build und Installation erfolgen über die vorhandenen Herkunftsprüfungen in `deploy-twitch-release` und `install-twitch-release`. Der Collector trägt ebenfalls eine `.twitch_build`-SHA. Nach der Datenbankmigration richtet die geprüfte root-eigene Kopie von `ops/systemd/install-category-collector.py` Benutzer, Credentials, Unit, Peer-Regeln und die eng ergänzten Caddy-Matcher ein. Bestehende Caddy-Regeln werden nicht ersetzt; vor Reload wird die Konfiguration validiert.

Prüfung nach Start:

```sh
systemctl status tb-category-collector.service
journalctl -u tb-category-collector.service --since '-10 minutes'
sudo -u postgres psql -d twitch_analytics -c "SELECT * FROM category_collection_runs ORDER BY snapshot_at DESC LIMIT 3"
sudo -u postgres psql -d twitch_analytics -c "SELECT heartbeat_at,details FROM category_collector_status"
```

Bei einer Störung kann ausschließlich der neue Dienst angehalten werden: `sudo systemctl stop tb-category-collector.service`. Bestehender Bot und Dashboard benötigen dazu keinen Neustart. Vor einem Rollback auf einen Release ohne Collector muss der neue Dienst angehalten werden. Rohdaten bleiben der Retention unterworfen, die Wartung läuft allerdings nur bei aktivem Collector.

## Nachweise

Automatische Tests decken vollständige Pagination über 1.200 Streams, wiederholte Cursor, leere Kategorien, anonymen Socket-Verkehr, inkrementelle Shards, Eingabevalidierung, Deduplizierung, Shared Chat, Löschungen, Rollups, Retention und serverseitige Admin-Sperren ab. PostgreSQL-Tests werden in expliziten Wegwerfdatenbanken ausgeführt, nicht gegen Produktivdaten. Die Schutztests des bestehenden Bots verweigern auch im Raid-Kontext Schreibaktionen außerhalb der autorisierten Partnerliste.

Eine erfolgreiche anonyme IRC-Begrüßung allein beweist noch keine vollständige Kategorieabdeckung. Für eine belastbare 24–48-Stunden-Skalierungsmessung müssen entsprechend lange echte Messdaten vorliegen. Live-Zahlen, Release-SHA und verbleibende Einschränkungen gehören in das zugehörige Abnahmeprotokoll; sie werden nicht durch synthetische Testdaten ersetzt.
