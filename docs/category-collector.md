# Globaler Deadlock-Kategoriesammler

`tb-category-collector.service` ist ein eigener Rust-Dienst mit dem Betriebssystem- und PostgreSQL-Benutzer `twitchcollector`. Er liest die gesamte über Helix sichtbare Live-Kategorie ohne Sprachfilter. IRC läuft ausschließlich anonym über `justinfan`. Die gemeinsame Lesekomponente wird auch vom bestehenden Scout verwendet und besitzt keine Sende-, Whisper- oder Moderationsschnittstelle.

## Daten und Grenzen

Vollständig paginierte Stream-Snapshots enthalten Kanal- und Stream-IDs, Titel, Zuschaueraggregat, Stream-Sprache, Startzeit, Tags, Vorschaubild und Mature-Kennzeichnung. Öffentliche Kanalprofile werden ergänzt. Der bestehende Bot ergänzt Follower-Gesamtzahlen mit seinem bereits verwalteten User-Token; ein fehlgeschlagener Abruf bleibt unbekannt. Es werden keine Follower-Einzellisten, Subscriber-Daten, vollständigen Zuschauer-/Lurker-Listen oder Zuschauerländer abgefragt.

Chat wird mit Nachrichten-ID, Herkunft, Zeit, Tags, Emotezahl und lokaler Sprachdetektion gespeichert. Kurze und unsichere Texte bleiben `und`. Stream-Sprache und Nachrichtensprache sind getrennte Merkmale, keine Geografie. Shared-Chat-Kopien sind gekennzeichnet und werden nicht mehrfach in die Volumensummen aufgenommen.

VOD- und Clip-Metadaten sind in der DB standardmäßig aktiviert. Es werden keine Videos oder Audiodaten heruntergeladen und keine STT- oder Cloud-Sprachdienste verwendet. Clips werden im gleitenden Sieben-Tage-Fenster abgefragt; verfügbare VOD-Seiten werden mit persistenten Cursorn nachgezogen. Das ist kein vollständiges historisches Medienarchiv. Kanal-VODs werden nicht automatisch als Deadlock-Videos ausgegeben.

## Dauerhafte Speicherung

Datenbank: `twitch_analytics`. Die Sammlertabellen beginnen mit `category_`; Rohchat liegt in täglichen UTC-Partitionen. **Es gibt keine automatische Alterslöschung und keine Kürzung des Bestands bei Speicherknappheit.** Stundenaggregate ergänzen die Rohdaten, sie ersetzen sie nicht.

Die additive Migration `20260918170000_category_permanent_archive.sql` deaktiviert auch die alte Partitions-Prune-Funktion. Der Rust-Kompatibilitätseinstieg zur Alterslöschung ist ebenfalls wirkungslos. Die Laufzeitrolle hat weder DELETE-, UPDATE- noch TRUNCATE-Rechte auf Rohchat. Bereits angewandte historische Migrationen werden nicht geändert.

Die DB-Konfiguration setzt anfänglich 20 GiB Rohdatenbudget und 10 GiB freien Plattenplatz als Reserve. Ab 80 Prozent des Budgets erscheint eine Warnung. Bei erreichtem Rohdatenbudget pausieren neue Chatzeilen; bei zu wenig oder nicht prüfbarem freien Plattenplatz pausieren auch neue Snapshots und Medienmetadaten. Bestandsdaten bleiben erhalten. Diese Werte sind veränderbare Betriebsgrenzen, keine Aufbewahrungsfristen und keine garantierte Grenze für die gesamte PostgreSQL-Belegung einschließlich WAL und anderer Dienste.

Explizite Twitch-Moderationsereignisse sind ein getrennter Vorgang: CLEARMSG entfernt nur die konkret bezeichnete Nachricht einschließlich zugehöriger Shared-Chat-Kopien. Gespeicherte Ziel-IDs und transaktionsgebundene Raumsperren verhindern deren Wiederherstellung durch verspätete oder parallel laufende Zustellungen. Ein personenbezogenes CLEARCHAT ist auf das Zeitfenster des aktuell beobachteten Streams begrenzt; auch hierfür bleibt eine Zielmarke aus Nutzer-ID und Zeitfenster bestehen, damit verspätete Zustellungen nicht erneut gespeichert werden. Ein allgemeines CLEARCHAT ohne Ziel löscht kein Kanalarchiv. Stundenaggregate werden danach neu berechnet. Eine rechtlich oder vom Betreiber angeordnete Datenlöschung ist nicht Teil einer pauschalen Retention.

## Konfiguration ohne ENV

Der Bootstrap enthält ausschließlich DB-Zugang und die Quelle der geschützten App-Zugangsdaten:

```sh
tb-category-collector --config /etc/deadlock-twitch/category-collector.json
```

Vorlage: `ops/systemd/category-collector.example.json`. Produktiv erhält der Dienst ein hostverschlüsseltes systemd-Credential mit nur Twitch-App-ID und App-Secret. Er bekommt weder Bot-Token noch Infisical-Bootstrap- oder Benachrichtigungs-Token. Verhaltensparameter werden laufend aus `category_collector_config` gelesen. Alte Retention-Felder in einem vorhandenen Bootstrap werden lediglich zur Abwärtskompatibilität akzeptiert, niemals als Löschfreigabe verwendet.

```sql
SELECT * FROM category_collector_config;
-- Änderungen führt der Betreiber als postgres aus, nicht der Collector:
UPDATE category_collector_config
SET raw_budget_bytes = 107374182400, updated_at = now()
WHERE singleton;
```

`enabled=false` pausiert die Erfassung; vorhandene Daten bleiben zugänglich. Das Plattenmessziel `/var/lib/postgresql` muss auf demselben Dateisystem wie PostgreSQL liegen. Auf diesem Host liegt dessen Datenverzeichnis unter `/var/lib/postgresql/16/main`.

## Dashboard

Admin-Seite: `/twitch/kategorie`, Navigation **Deadlock weltweit**. Datenroute: `/twitch/api/v2/category-collector?days=7` mit 7, 30 oder 90 Tagen. Diese Werte sind ausschließlich Ansichtsfenster. Beide Routen sind serverseitig adminpflichtig: 401 ohne Anmeldung, 403 für normale Partner. Es gibt keinen Rohchat-Endpunkt; auch die Dashboard-DB-Rolle darf Rohchat nicht lesen.

Die Seite zeigt Sprachen, Stundenverlauf, Top-Kanäle nach Zuschauerstunden und Chat-Tageszeiten in UTC. Stundenwerte bleiben unverändert; Lücken werden nicht zu Nullwerten oder falschen Tagesmitteln. Unbekannte gewichtete Durchschnitte bleiben unbekannt. Kanal- und Shard-Abdeckung, Datenbeginn, letzte Messung, Speicherstand, Warnungen und seit Prozessstart bekannte Verluste sind sichtbar. Stündlich eindeutige Schreiber werden nicht über Stunden zu angeblich eindeutigen Zuschauern aufsummiert.

## Installation und Prüfung

Der vorhandene Release-Weg baut alle vier Rust-Binaries aus demselben sauberen SHA: `tb-bot`, `tb-dashboard`, `tb-stream-audit` und `tb-category-collector`, dazu die bestehenden Frontends. Herkunftsnachweis: ELF-Sektion `.twitch_build`. Die aktualisierten, geprüften Wrapper unter `ops/systemd/deploy-twitch-release` und `ops/systemd/install-twitch-release.sh` müssen installiert sein; ältere Host-Wrapper kennen den Collector noch nicht.

`deploy-twitch-release` wendet Migrationen als postgres an. Danach richtet die root-eigene Kopie von `ops/systemd/install-category-collector.py` im versiegelten Release Benutzer, Peer-Zugang, eingeschränkte Rollen, Credentials, Unit und Caddy-Matcher ein. Dieser vorhandene Installationshelfer ist ein Einmalwerkzeug; die Sammlerlaufzeit selbst ist Rust. Neue Caddy-Regeln werden vor Reload validiert, bestehende Regeln nicht ersetzt.

Die Unit hat 512 MiB Speicherlimit, CPU-Begrenzung und eine getrennte OnFailure-Benachrichtigung. Die neue Migration und Rollenmatrix müssen vor dem Collector-Start angewandt sein, damit er ausschließlich gezielte Moderationsereignisse entfernen kann.

```sh
systemctl status tb-category-collector.service
journalctl -u tb-category-collector.service --since '-10 minutes'
sudo -u postgres psql -d twitch_analytics -c "SELECT * FROM category_collection_runs ORDER BY snapshot_at DESC LIMIT 3"
sudo -u postgres psql -d twitch_analytics -c "SELECT heartbeat_at,details FROM category_collector_status"
```

Ein Stopp nur des Collectors beeinträchtigt Bot und Dashboard nicht. Vor einem Rollback auf eine Version vor der dauerhaften Archivierung den Collector anhalten: Der alte Rust-Retentionpfad würde mit den absichtlich entzogenen DELETE-Rechten scheitern. Die Datenbank-Löschsperre nicht zurücknehmen.

Tests verwenden isolierte PostgreSQL-Instanzen und Mock-/lokale IRC-Verbindungen, keine produktiven Testnachrichten. Ein kurzer Live-Lauf belegt nur den beobachteten Datenfluss. Eine vollständige 24–48-Stunden-Abdeckungsmessung benötigt entsprechende reale Laufzeit; konkrete Live-Zahlen und Release-SHA stehen separat im Abnahmebericht.
