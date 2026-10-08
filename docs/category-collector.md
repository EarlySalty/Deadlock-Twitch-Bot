# Globaler Deadlock-Kategoriesammler

Der Kategoriesammler läuft als Rust-Bibliothek mit eigenen Tasks im `tb-bot`-Prozess (`deadlock-twitch-bot-rust.service`). Er nutzt den PostgreSQL-Pool, den Helix-Client, den geschützten Zugangspfad und das Logging des Bots. Er liest die über Helix sichtbare Deadlock-Live-Kategorie ohne Sprachfilter. Für die Sammlung sind keine eigene App-Konfiguration, kein separates App-Credential und kein dauerhaft laufender Sammlerdienst erforderlich.

IRC läuft anonym über `justinfan`, unabhängig vom angemeldeten Bot-Konto. Die vorhandene gemeinsame Lesekomponente wird auch vom Scout verwendet und besitzt keine Sende-, Whisper- oder Moderationsschnittstelle. Ein nativer Leader-Lock verhindert parallele Sammlung durch mehrere Bot-Prozesse. Beim Übergang darf auch der bisherige externe Sammler nicht gleichzeitig Daten erfassen; die Umschaltung ist unten beschrieben.

## Daten und Grenzen

Vollständig paginierte Stream-Snapshots enthalten Kanal- und Stream-IDs, Titel, Zuschaueraggregat, Stream-Sprache, Startzeit, Tags, Vorschaubild und Mature-Kennzeichnung. Öffentliche Kanalprofile werden ergänzt. Der bestehende Bot ergänzt Follower-Gesamtzahlen über seinen geschützten Zugang; ein fehlgeschlagener Abruf bleibt unbekannt. Follower-Einzellisten, Subscriber-Daten, vollständige Zuschauer-/Lurker-Listen und Zuschauerländer gehören nicht zur Sammlung.

Chat wird mit Nachrichten-ID, Herkunft, Zeit, Tags, Emotezahl und lokaler Sprachdetektion gespeichert. Kurze und unsichere Texte bleiben `und`. Stream-Sprache und Nachrichtensprache sind getrennte Merkmale, keine Geografie. Shared-Chat-Kopien sind gekennzeichnet und werden nicht mehrfach in die Volumensummen aufgenommen.

VOD- und Clip-Metadaten sind über `category_collector_config.media_enabled` steuerbar, standardmäßig aktiviert. Die Sammlung lädt keine Videos oder Audiodaten herunter und nutzt keine STT- oder Cloud-Sprachdienste. Clips werden im gleitenden Sieben-Tage-Fenster abgefragt; verfügbare VOD-Seiten werden mit persistenten Cursorn nachgezogen. Das ist kein vollständiges historisches Medienarchiv. Kanal-VODs werden nicht automatisch als Deadlock-Videos ausgegeben.

## Dauerhafte Speicherung und Speicherpause

Datenbank: `twitch_analytics`. Die Sammlertabellen beginnen mit `category_`; Rohchat liegt in täglichen UTC-Partitionen. Es gibt keine automatische Archiv-Alterslöschung und keine Kürzung des Bestands wegen Speicherknappheit. Stundenaggregate ergänzen die Rohdaten, sie ersetzen sie nicht.

Die additive Migration `20260918170000_category_permanent_archive.sql` deaktiviert die alte Partitions-Prune-Funktion. Der Rust-Kompatibilitätseinstieg zur Alterslöschung ist wirkungslos. Rechte für die native Bot-Rolle werden über neue additive Migrationen vergeben, nicht durch Änderungen an bereits angewandten Migrationen. Pauschale DELETE-, UPDATE- oder TRUNCATE-Rechte auf Rohchat bleiben der Bot-Rolle entzogen.

Die DB-Konfiguration setzt anfänglich 20 GiB Rohdatenbudget und 10 GiB freien Plattenplatz als Reserve. Ab 80 Prozent des Budgets erscheint eine Warnung. Bei erreichtem Rohdatenbudget pausieren neue Chatzeilen; bei zu wenig oder nicht prüfbarem freien Plattenplatz pausieren auch neue Snapshots und Medienmetadaten. Bestandsdaten bleiben erhalten. Diese Werte sind veränderbare Betriebsgrenzen, keine Aufbewahrungsfristen und keine garantierte Grenze für die gesamte PostgreSQL-Belegung einschließlich WAL und anderer Dienste.

Die Speicherpause bleibt bis zur Erholungsgrenze aktiv. Die Hysterese trennt Pause und Wiederaufnahme, damit kleine Schwankungen um die Speichergrenze die Erfassung nicht fortlaufend umschalten. Heartbeat und Speicherprüfung laufen während der Pause weiter. Ein aktueller Heartbeat mit `disk_paused=true` bezeichnet eine Speicherpause, keinen Sammlerausfall. `raw_paused=true` betrifft Rohchat; aktuelle Kategoriemessungen können weiterhin vorliegen. Ein veralteter Heartbeat bleibt auch bei zuletzt gemeldeter Speicherpause prüfbedürftig.

Explizite Twitch-Moderationsereignisse sind ein getrennter Vorgang: CLEARMSG entfernt die konkret bezeichnete Nachricht einschließlich zugehöriger Shared-Chat-Kopien. Gespeicherte Ziel-IDs und transaktionsgebundene Raumsperren verhindern deren Wiederherstellung durch verspätete oder parallel laufende Zustellungen. Ein personenbezogenes CLEARCHAT ist auf das Zeitfenster des zum Twitch-Ereignis beobachteten Streams begrenzt; die Zielmarke aus Nutzer-ID und Zeitfenster endet am Twitch-Zeitstempel, bei fehlendem Tag an der IRC-Empfangszeit. So bleiben verspätete ältere Nachrichten entfernt, aber später gesendete Nachrichten erhalten. Ein allgemeines CLEARCHAT ohne Ziel löscht kein Kanalarchiv. Stundenaggregate werden danach neu berechnet. Eine rechtlich oder vom Betreiber angeordnete Datenlöschung ist nicht Teil einer pauschalen Retention.

## Konfiguration ohne ENV

Der Sammler erhält Pool und Helix-Client aus dem Bot. App-Zugangsdaten kommen aus dem vorhandenen geschützten Bot-Weg, etwa Infisical oder den bestehenden Bot-Credentials. Es gibt keinen zusätzlichen Sammler-Secretweg. Verhaltensparameter werden laufend aus `category_collector_config` gelesen. Die Einstellungen lassen sich lesend prüfen:

```sql
SELECT enabled, poll_seconds, raw_budget_bytes, min_free_bytes, media_enabled
FROM category_collector_config
WHERE singleton;
```

Änderungen führt der Betreiber als `postgres` aus, nicht die Sammlerlaufzeit:

```sql
UPDATE category_collector_config
SET raw_budget_bytes = 107374182400, updated_at = now()
WHERE singleton;
```

`enabled=false` deaktiviert neue Erfassung; vorhandene Daten bleiben zugänglich. Das Plattenmessziel `/var/lib/postgresql` muss auf demselben Dateisystem wie PostgreSQL liegen. Auf diesem Host liegt dessen Datenverzeichnis unter `/var/lib/postgresql/16/main`.

## Dashboard und Berichtvertrag

Admin-Seite: `/twitch/kategorie`, Navigation **Deadlock weltweit**. Datenroute: `/twitch/api/v2/category-collector?days=7` mit 7, 30 oder 90 Tagen. Diese Werte sind Ansichtsfenster, keine Löschfristen. Beide Routen sind serverseitig adminpflichtig: 401 ohne Anmeldung, 403 für normale Partner. Es gibt keinen Rohchat-Endpunkt; die Dashboard-DB-Rolle darf Rohchat nicht lesen.

Die Seite zeigt Sprachen, Stundenverlauf, Top-Kanäle nach Zuschauerstunden und Chat-Tageszeiten in UTC. Stundenwerte bleiben unverändert; Lücken werden nicht zu Nullwerten oder falschen Tagesmitteln. Unbekannte gewichtete Durchschnitte bleiben unbekannt. Kanal- und Shard-Abdeckung, Datenbeginn, letzte Messung, Speicherstand, Warnungen und seit Prozessstart bekannte Verluste sind sichtbar. Stündlich eindeutige Schreiber werden nicht über Stunden zu angeblich eindeutigen Zuschauern aufsummiert.

Der bestehende Bericht liefert `collector_config.enabled` und `collector_config.poll_seconds`, `heartbeat_at`, `last_snapshot` sowie die Statusdetails unter `status`. Die Details enthalten `disk_paused`, `raw_paused`, `storage_checked_at`, `last_discovery` und `discovery_state`. Intern liegen Heartbeat und Details in `category_collector_status`; das Frontend erwartet keine neue Statushülle.

Die Anzeige unterscheidet deaktivierte Sammlung, Plattenpause, Rohchatbudget-Pause, veraltete Statusmeldung, fehlende aktuelle Kategoriemessungen und einen nicht aktuell bestätigten Kategorieabruf. Sie prüft Aktualität gegen die laufende Uhr, nicht gegen `generated_at`. Der alle 15 Sekunden erneuerte Heartbeat hat eine Toleranz von zwei Minuten. Für Snapshots und erfolgreiche Kategorieabrufe beträgt die Toleranz mindestens zwei Minuten beziehungsweise zwei konfigurierte Messintervalle. Eine Plattenpause mit aktuellem Heartbeat wird auch bei alten Snapshots als Pause dargestellt. Fehlende Snapshots ohne Plattenpause bleiben ein eigener Hinweis. Bei veralteter Statusmeldung sind Speicherangaben und Chat-Abdeckung zuletzt gemeldete Werte. Historische Auswertungen bleiben sichtbar.

## Bereitstellung und Umschaltung

Die Integration bringt den Sammler mit dem Bot-Release. `tb-category-collector` ist danach eine Bibliothek, kein eigenes Sammlerbinary. `tb-twitch-watchdog` bleibt ein separates, kurz laufendes Rust-Prüfprogramm im bestehenden `deadlock-twitch-bot-watchdog.timer`; es sammelt selbst keine Kategorie- oder Chatdaten. Der Herkunftsnachweis der Release-Artefakte steht in der ELF-Sektion `.twitch_build`.

`deploy-twitch-release` startet tatsächlich `deadlock-twitch-migrate.service`. Für diese Umschaltung spielt die Integration die neuen additiven Migrationen vor dem Release manuell als `postgres` in `twitch_analytics` ein. Die passenden Grants für Bot und Dashboard sowie Einträge in `_sqlx_migrations` müssen mit Version, Beschreibung, Erfolg und passender SQLx-Prüfsumme korrekt geführt werden. Der Wrapper prüft den angewandten Stand vor dem Bot-Neustart. Der Schema-Snapshot gehört zum integrierten Stand; zusätzliche `.sqlx`-Metadaten sind für die dynamischen Sammlerabfragen nicht erforderlich.

Für die Umschaltung gilt diese Reihenfolge:

1. Migrationen prüfen und einspielen, dann den Bot mit nativer Sammlerbibliothek bereitstellen. Der native Leader-Lock muss doppelte native Sammlung verhindern. Solange der alte externe Sammler noch aktiv ist, darf der native Sammler nicht parallel erfassen; die Integrationsprüfung muss die Übergangssperre belegen.
2. Den bisherigen `tb-category-collector.service` stoppen und deaktivieren. Danach native Sammlung im Bot-Journal, neue `category_collection_runs` und neue Zeilen in `category_stream_snapshots` nachweisen. Ein aktueller Heartbeat allein beweist keine neuen Messungen.
3. Watchdog und Dashboard prüfen. Erst nach erfolgreichem nativen Live-Nachweis externe Unit, Sammler-Credential, Bootstrap-Konfiguration und bisherige Installationsreste endgültig entfernen. Bestehende Archivdaten und benötigte Watchdog-Rechte erhalten.

Die native Bereitschaft liegt während des Wartens auf die bisherige Lease in `category_native_runtime`. Nach der Übernahme enthalten die Statusdetails `runtime=tb-bot`, `process_id`, `process_started_at`, `lease_id` und `native_lease_active=true`. Der Wrapper verknüpft diese Angaben mit der gestarteten Bot-PID und verlangt neue Sammlerläufe sowie Stream-Snapshots nach dem Umschaltzeitpunkt, bevor er alte Unit, Bootstrap und App-Credential entfernt.

Der Watchdog prüft Bot und Sammlerzustand über den vorhandenen Status- und Datenpfad. Speicherpausen werden getrennt von Ausfällen geführt. Fehlende aktuelle Messungen ohne erklärende Pause und veraltete Heartbeats bleiben Ausfallhinweise. Speicherpausen werden nach fünf Minuten gemeldet, nach 30 durchgehend freien Minuten abgeschlossen und höchstens einmal je Vorfall sowie insgesamt einmal pro Berliner Kalendertag zugestellt. Wiederholungen innerhalb des Vorfalls werden mitgezählt. Der persistente Zustellauftrag behält Inhalt und Idempotenzschlüssel bei erneuten Versuchen. Bekannte deaktivierte Zeiten und Plattenpausen werden nicht nachträglich als Messausfall gemeldet. Historische Lücken vor Einführung dieser Unterscheidung lösen keine neue Ausfallmeldung aus. Zustellung erfolgt über den vorhandenen lokalen Discord-Broker, Vorfälle bleiben in `twitch_watchdog_incidents` gespeichert. Die historische Migration `20261001220000_twitch_watchdog_incidents.sql` bleibt Voraussetzung für diesen Vorfallpfad.

Eine historische Collector-Lücke schaltet Rangliste und Erfolge nicht ab. Betroffene Stream-Aufgaben und Ausdauer bleiben ausgesetzt, während bestätigte Punkte und unabhängige Aufgaben sichtbar bleiben. Das Dashboard kennzeichnet die Teilwertung. Monatsboosts verlangen weiterhin die sieben gesunden Effort-Quellen.

```sh
systemctl status deadlock-twitch-bot-rust.service
journalctl -u deadlock-twitch-bot-rust.service --since '-10 minutes'
sudo -u postgres psql -d twitch_analytics -c "SELECT * FROM category_collection_runs ORDER BY snapshot_at DESC LIMIT 3"
sudo -u postgres psql -d twitch_analytics -c "SELECT max(snapshot_at) FROM category_stream_snapshots"
sudo -u postgres psql -d twitch_analytics -c "SELECT heartbeat_at,details FROM category_collector_status"
systemctl status deadlock-twitch-bot-watchdog.timer
```

Ein Bot-Neustart startet auch die Sammler-Tasks neu. `enabled=false` pausiert die Sammlung ohne Bot-Stopp. Beim Rollback muss die Übergangssperre erneut geprüft werden; der externe Sammler darf nicht gleichzeitig mit dem nativen Pfad laufen. Die Archiv-Löschsperre bleibt bestehen.

Tests verwenden isolierte PostgreSQL-Instanzen und Mock-/lokale IRC-Verbindungen, keine produktiven Testnachrichten. Ein kurzer Live-Lauf belegt den beobachteten Datenfluss, keine vollständige Tagesabdeckung. Eine 24–48-Stunden-Abdeckungsmessung benötigt entsprechende reale Laufzeit; konkrete Live-Zahlen und Release-SHA gehören zum Integrationsnachweis.
