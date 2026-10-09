# Kategorie-Speicherung: Übergang und Betrieb

Die Speicheränderung betrifft den nativen Kategoriesammler. Das VOD-Archiv bleibt unverändert. Diese Anleitung beschreibt den Kandidaten und seine Freigabeschritte, keinen bereits durchgeführten produktiven Archivlauf.

## Format und Auswertungen

`category_snapshot_samples` speichert Messzeit, Stream-ID, Versionsverweis, Betrachterzahl und `sample_seconds`. `category_snapshot_versions` enthält Stream- und Kanal-ID, den damaligen Login, Titel, Sprache, Startzeit, geordnete Tags einschließlich NULL-Elementen und Array-Grenzen, Vorschaubild und Mature-Kennzeichnung. Unveränderte Inhalte werden wiederverwendet; `valid_from` bezeichnet ihr erstes beobachtetes Auftreten, kein lückenloses Gültigkeitsintervall. Ein Fingerprint beschleunigt die Suche, der Writer vergleicht zusätzlich die tatsächlichen Felder. Widersprüchliche Inhalte für denselben Messschlüssel werden abgewiesen.

Die Rekonstruktion `category_snapshots_normalized` liefert dieselben zwölf Spalten wie das alte Format. Zeitwerte und `sample_seconds` verwenden im Prüf- und Archivformat ihre PostgreSQL-Binärdarstellung. Tags werden als PostgreSQL-Arraytext transportiert. Dadurch bleiben Mikrosekunden, Fließkomma-Bits, Tagreihenfolge, NULL-Elemente und Array-Grenzen erhalten.

Der Bericht liest `category_snapshot_metric_source`. Nach einer Auslagerung bleiben dort die für Sprach-, Kanal-, Stream- und Zuschauerstundenwerte erforderlichen Messwerte erhalten. Es bleiben keine Titel, Vorschaubilder oder Taglisten ausgelagerter Snapshots in dieser Kennzahlenstruktur. Die Stream-Startzeit bleibt für zielbezogene Chatentfernungen verfügbar. Collection-Runs und Kanalprofile werden nicht ausgelagert.

`category_chat_metrics` enthält Identifikatoren, Zeit, Länge, erkannte Sprache, Shared-Chat-Herkunft und Ausschlussmerkmal, keinen Nachrichtentext oder Chatter-Login. Die Identifikatoren ermöglichen weiterhin gezielte Entfernungen und die genaue Zahl unterschiedlicher Schreiber innerhalb einer Kanal-Sprach-Stunde. `flush_rollups` rekonstruiert daraus und aus noch nicht nachgezogenen Rohzeilen. Eine Auslagerung leert keine Stundenaggregate. Neue Rohzeilen erhalten ihre Kennzahlen innerhalb derselben Transaktion über einen Trigger.

Aus den IRC-Tags werden vor der Reduktion Raum, Nutzer, Nachrichten-ID, Sendezeit, Emotes und Shared-Chat-Herkunft ausgewertet. Die ersten vier Werte liegen anschließend in eigenen Spalten. Im gespeicherten Tagobjekt bleiben `emotes`, `source-room-id` und `source-id`. Moderationsziele werden weiterhin direkt aus CLEARMSG und CLEARCHAT gelesen; sie sind keine PRIVMSG-Archivfelder. Der Backfill reduziert auch die alten, bisher ungenutzten Tagfelder.

## Additiver Übergang ohne ungeprüftes Entfernen

1. `20261009110000_category_storage.sql` als Betreiber anwenden und SQLx-Migrationsbuchführung mit passender SHA-384-Prüfsumme führen. Die bereits angewandten Migrationen bleiben unverändert. Die neue Migration entfernt die alte Snapshot-Tabelle nicht. Ein Insert-Trigger erfasst ab diesem Moment auch Writes eines noch laufenden alten Bot-Stands im normalisierten Format.
2. Den neuen Bot-Stand bereitstellen und neue Collection-Runs sowie deren rekonstruierbare Samples nachweisen. Der neue `store_snapshot` schreibt über `category_snapshot_put`. `category_snapshots_read` überbrückt ausschließlich den noch offenen Altbestand. Auslagerung bleibt bis zum abgeschlossenen Übergang gesperrt. Ein Rückwechsel zum alten Writer ist in dieser Phase noch möglich.
3. Den Rust-Backfill für den gesamten tatsächlichen Altbestand ausführen, mindestens ab 2026-09-27 bis einschließlich des aktuellen UTC-Tags. Fehlende Samples und Chat-Kennzahlen werden in Batches von höchstens 1.000 Zeilen nachgezogen. Bereits bestätigte Batches werden beim Wiederanlauf nicht verdoppelt.
4. `finalize --apply` als Betreiber ausführen. Der Chat-Kennzahlenvergleich prüft jede vorhandene Rohzeile. Die Snapshot-Phase sperrt Collection-Runs und die neue Sample-Tabelle gegen Änderungen sowie die alte Tabelle gegen weitere Writes. Der letzte abgeschlossene Messlauf muss mit der vom neuen Writer transaktional gesetzten Marke `writer_snapshot_at` übereinstimmen. Sie rekonstruiert jede Originalzeile erneut und speichert Zeilenzahl, SHA-256 und Prüfzeit pro UTC-Tag in `category_normalization_proofs`. Ein Unterschied bricht vor dem DROP ab. Erst in derselben erfolgreichen Transaktion wird die Übergangsview auf das neue Format umgestellt und die alte Tabelle entfernt.

Der Abschluss vergleicht den vollständigen Originalbestand, nicht eine Stichprobe. Neue Writes, die bereits ausschließlich normalisiert vorliegen, bleiben zusätzlich erhalten. Ein fehlerhafter Vergleich oder eine nicht erhältliche Sperre lässt die alte Form liegen. Der vollständige Abschluss ist ein Wartungsschritt: Schreibzugriffe warten während der Prüfung; seine Dauer hängt vom tatsächlichen Altbestand ab. Die Hauptsession muss davor den einzigen nativen Writer im neuen Stand belegen und danach Messungen, Lease und Watchdog erneut prüfen. Ein abgebrochener Abschluss rollt die Snapshot-Transaktion zurück. Ein erneuter erfolgreicher Abschluss liefert die gespeicherten Tagesnachweise. Nach dem DROP darf kein alter Bot-Stand mit dem alten Snapshot-Writer gestartet werden. Ein solcher Rollback benötigt vorher einen gesonderten, geprüften Wiederaufbau; der Deploy eines älteren Binaries genügt dafür nicht.

Der vorhandene Schema-Snapshot beschreibt den additiven Migrationsstand vor dem ausdrücklich bestätigten DROP. Für die dynamischen SQL-Abfragen sind keine zusätzlichen `.sqlx`-Dateien erforderlich.

## Archiv, Partner und lokale Entfernung

Die Partnerquelle ist `twitch_streamers_partner_state`: `is_partner=1`, aktiver Status und exakte Twitch-User-ID. Logins sind keine Zuordnungsschlüssel. Fehlt bei einem aktiven Partner die stabile ID, bleiben Trockenlauf, Export und lokale Entfernung gesperrt. Ein Export umfasst Nichtpartner-Rohzeilen eines abgeschlossenen UTC-Tags und einer Datenart. Die Partnerquelle wird bei jedem Entfernungsschritt erneut gelesen; eine SHARE-Sperre auf `twitch_partners` schützt diese Entscheidung bis zum Commit.

Ein Manifest speichert UTC-Tag, Datenart, unverwechselbaren objektbezogenen Pfad, Zeilen, SHA-256 des JSONL-Inhalts, SHA-256 und Größe des Chiffretexts, Schlüssel-ID, Zustand und Entfernungscursor. Zustände sind `exporting`, `exported`, `verified`, `removed` und `restored`. Jeder Exportlauf hat einen eigenen UUID-Dateinamen unter `gdrive:category/<UTC-Tag>/`. Späte Daten oder spätere Partnerwechsel können eine weitere unveränderliche Tagesgeneration erfordern. Vorhandene bestätigte Objekte werden dabei nicht überschrieben oder aus Drive entfernt.

Das Format `TBCAT001` besteht aus begrenzten zstd-Blöcken mit AES-256-GCM. Die Authentifizierung bindet Objekt-ID, Tag, Datenart, Schlüssel-ID, Blocknummer und Abschlussmerkmal. Ein verschlüsselter Abschluss enthält Zeilenzahl und Inhaltsprüfsumme. Umordnung, Abschneiden, fremde Objekte, falsche Schlüssel und beschädigte Blöcke werden abgewiesen. Eine Zeile ist auf 512 KiB und ein Klartextblock auf 1 MiB begrenzt. Ein übergroßer Datensatz bricht die Auslagerung ab und bleibt lokal.

Temporäre Verzeichnisse sind privat; Exportdateien haben Modus 0600. Es werden komprimierte und verschlüsselte Dateien geschrieben, keine Klartext-Tagestabellen. Die Datenbank wird zeilenweise gelesen. Manifestmitglieder werden in Batches von 500 Einträgen gespeichert. Dateigröße, freie Plattenreserve und rclone-Laufzeit sind begrenzt. Zusätzlich zum Größenfilter begrenzen `--max-transfer` und `--cutoff-mode hard` die übertragenen Bytes. Der Exporthash wird asynchron in 64-KiB-Stücken berechnet. Die vollständige Archivprüfung läuft außerhalb der Tokio-Laufzeitthreads; ein Abbruch wird zwischen Hashstücken und Archivblöcken geprüft. Das Programm nutzt den vorhandenen rclone-Remote `gdrive:` und dessen Dateikopierbaustein, keinen weiteren Google-Connector.

Nach dem Upload wird das konkrete Objekt erneut heruntergeladen. Chiffretextprüfsumme, Größe, authentifizierte Blöcke, Inhaltsprüfsumme und Zeilenzahl müssen passen. Erst dann wird das Manifest `verified`. Vor einem lokalen Entfernen wird diese Prüfung wiederholt und die Mitgliederzahl kontrolliert. Die Datenbankfunktion entfernt in Batches von höchstens 1.000 bestätigten Mitgliedern, prüft deren aktuelle Inhalte und schreitet erst nach dem Commit weiter. Unbestätigte Tagesreste, veränderte Zeilen und Partnerbestände werden nicht durch ein Alters- oder Näherungsprädikat gelöscht.

Nach der bestätigten Chatentfernung verdichtet `category_archive_compact` die betroffene Tagespartition. Es kopiert die verbliebenen Partnerzeilen und noch nicht bestätigten späten Zeilen vollständig, vergleicht beide Richtungen mit `EXCEPT ALL` und ersetzt die Partition erst innerhalb derselben erfolgreichen Transaktion. Der Schritt gibt die alte Heap- und Indexbelegung frei, statt auf eine zufällige Verkleinerung durch Autovacuum zu hoffen. `compacted_at` hält den Wiederanlauf fest; eine später erneut notwendige Entfernung setzt diese Marke zurück. Ein Fehler beim Ersetzen lässt die verbliebenen lokalen Daten unverändert und wird erneut bearbeitet. Der Rust-Aufrufer prüft zusätzlich zur Archivreserve die vorhandene PostgreSQL-Reserve unter `/var/lib/postgresql`, dieselbe Stelle wie der native Sammler, mit Platz für das Doppelte der bisherigen Partitionsbelegung. Der gemeinsame Chatlock und die Parent-Sperre unterbrechen Schreibzugriffe während dieses Wartungsschritts; Sperrwartezeit und Statement-Laufzeit sind begrenzt.

Die Rolle `twitchcategoryarchive` besitzt den engen Funktionszugriff. Die additive Migration entzieht alte direkte Änderungs-, Lösch- und Leerungsrechte auf Rohchat, dessen Tagespartitionen und alte Snapshots. Der Bot erhält zunächst kein Ausführungsrecht auf die Archiv-Entfernungsfunktion. Zusätzlich bleibt `category_storage_state.removal_authorized=false`, bis der Betreiber nach dem fertigen Trockenlauf ausdrücklich freigibt. Wiederholte Entfernungsläufe sind vorgesehen; bei einem späteren Partnerwechsel kann ein zuvor geschütztes Manifest erneut geprüft werden. Export, Entfernung und Rückholung sind über dieselbe Archivsperre serialisiert; vor der Rückgabe wird ihre Freigabe ausdrücklich bestätigt.

CLEARMSG und zielbezogene CLEARCHAT bearbeiten auch die lokal erhaltenen Chat-Kennzahlen. Ihre Zielmarken bleiben erhalten. Der Rückholweg importiert Chat über denselben Redaktions- und Raumsperrvertrag. Eine Shared-Chat-Kopie einer entfernten Quelle wird ebenfalls abgewiesen. Archivdateien behalten ihren historischen Chiffretext; die persistierten Zielmarken werden bei jeder Rückholung angewandt und bereits entfernte Inhalte nicht wiederhergestellt.

## Konfiguration und Freigaben

Die nicht geheime Betriebsdatei enthält optional folgende Tabelle. Beide Schalter sind standardmäßig `false`.

```toml
[category_archive]
enabled = false
remove_enabled = false
temporary_directory = "/tmp"
max_file_bytes = 2147483648
min_free_bytes = 10737418240
command_timeout_seconds = 1800
```

Bei aktiviertem Archiv braucht der bestehende Bot-Pool mindestens drei Verbindungen. Der Archivschlüssel wird als benannter Secret-Wert `CATEGORY_ARCHIVE_KEY_V1` über die vorhandene Infisical-Hydration geladen. Er muss ein eigener zufälliger 32-Byte-Schlüssel in Hexdarstellung sein. Schlüssel-ID ist `category-v1`. Dieser Schlüssel darf nicht ohne gesondert geprüften Wechsel überschrieben werden; bereits bestätigte Dateien benötigen weiterhin denselben Schlüssel. Die Rohdarstellung und dekodierten Schlüsselbytes werden zeroisiert. Fehlende oder ungültige Werte ergeben keinen vermeintlich vorhandenen Schlüssel. Keine Schlüsseldatei und kein ENV-Verhaltensparameter werden angelegt. Den Schlüssel muss die Hauptsession getrennt bereitstellen, ohne seinen Wert in Auftrag, Bericht oder Modellaufruf aufzunehmen.

Vor dem ersten echten Entfernen ist der fertige Rust-Trockenlauf mit aktuellen Zeilen und Zeilenbytes je UTC-Tag, Datenart und Partnerstatus vorzulegen. `row_bytes` misst die vollständigen rekonstruierten Zeilen; es ist weder die normalisierte physische Belegung noch eine komprimierte Dateigröße. Für Snapshots werden zusätzlich die tatsächlichen Tupelbreiten als `sample_bytes`, `version_bytes` und noch offene `legacy_bytes` gemeldet. Eine Versionszeile zählt je Tagesgruppe einmal, kann aber von mehreren Tagen verwendet werden; Tageswerte sind daher nicht vollständig additiv. Seitenbelegung, Indizes, WAL und vorhandene freie Tabellenbereiche sind darin nicht enthalten. Der Trockenlauf verwendet eine PostgreSQL-Transaktion mit `REPEATABLE READ READ ONLY` und schreibt weder Manifeste noch Proofs oder Dateien.

Erst nach dieser Freigabe darf der Betreiber `removal_authorized` setzen, der tatsächlich verwendeten Laufzeitrolle `EXECUTE ON FUNCTION category_archive_remove(uuid,integer), category_archive_compact(uuid)` gewähren und `remove_enabled=true` aktivieren. Die beiden Änderungen ersetzen keine Uploadprüfung. `enabled=true` startet den nativen stündlichen Exportjob; ohne Entfernungsfreigabe hält er lokale Rohdaten weiter vor. Der Job bearbeitet pro stündlichem Lauf höchstens zwei Tages-/Datenart-Paare und zwei ausstehende Manifeste. Er setzt bestätigte, noch nicht fertig entfernte Manifeste fort und erkennt inzwischen nicht mehr geschützte Zeilen in bereits abgeschlossenen Manifesten erneut. Schreibende CLI-Befehle laufen über einen ausdrücklich gewählten Datenbankzugang mit den erforderlichen Betreiberrechten, nicht über einen allgemeinen Rohdaten-Löschgrant.

Archivfehler werden in PostgreSQL gezählt. Höchstens einmal innerhalb von 24 Stunden wird eine Meldung für den bestehenden Watchdog vorgemerkt. Dieser verwendet seinen vorhandenen lokalen Discord-Broker, einen stabilen täglichen Idempotenzschlüssel und bestätigt die Zustellung in PostgreSQL. Fehlgeschlagene Zustellungen werden erneut versucht; höchstens eine bestätigte Auslagerungsmeldung pro Berliner Kalendertag wird zugestellt. Rohdaten bleiben bei Export-, Upload- oder Integritätsfehlern liegen.

## Rust-CLI

Die Binary heißt `tb-category-storage`. Normale Aufrufe verwenden `--config /absoluter/pfad/bot.toml` und den bestehenden benannten Secret-Zugang des jeweiligen Bot-Startpfads. Die folgenden Unterbefehle stehen danach zur Verfügung:

```text
dry-run <von-UTC-Tag> <bis-UTC-Tag>
backfill <von-UTC-Tag> <bis-UTC-Tag>
finalize --apply
archive <UTC-Tag> <chat|snapshots> --apply
remove <Manifest-UUID> --apply
restore <Manifest-UUID> --apply
```

`restore` lädt und prüft das bestätigte konkrete Objekt zunächst vollständig. Ein zweiter begrenzter Leselauf importiert innerhalb einer Transaktion. Die Collection-Runs und Kennzahlen dieses Tages müssen in derselben Datenbank erhalten sein. Der gemeldete Rückholwert bezeichnet verarbeitete Archivzeilen, nicht die Zahl nach Redaktionsfiltern neu eingefügter Chatnachrichten. Wiederholungen sind dedupliziert. Restore ändert ein Manifest zu `restored`; dessen erfasste Schlüssel lösen keinen erneuten automatischen Tagesexport aus. Kommen andere noch nicht erfasste Nichtpartnerdaten dieses Tages hinzu, kann eine weitere Tagesgeneration auch zurückgeholte Rohzeilen erneut erfassen und nach bestätigter Prüfung entfernen.

`--test-database <Datenbankname>` ist auf den synthetischen Server `127.0.0.1:33100` und Datenbanknamen mit `category_storage_` oder `tb_storage_` beschränkt. Dieser Zugang erlaubt `dry-run`, `backfill` und `finalize`, keine Drive-Befehle. Er ist kein Produktionszugang. Der Container dieses Auftrags bleibt bestehen.

`--postgres-operator` verwendet ausschließlich das Betriebskonto `postgres`, den lokalen Peer-Socket `/var/run/postgresql` und die Datenbank `twitch_analytics`. Dieser Übergangspfad erlaubt ebenfalls `dry-run`, `backfill` und `finalize`, keine Drive-Befehle. Er braucht keine zusätzliche DSN- oder Schlüsseldatei. Die Hauptsession führt ihn für den produktiven Übergang aus; der Implementierer dieses Kandidaten hat ihn nicht produktiv ausgeführt. Archiv-, Entfernungs- und Rückholbefehle verwenden dagegen den normalen Bot-Zugang. Vor einer Rückholung braucht dessen Rolle zusätzlich `EXECUTE ON FUNCTION category_restore_chat(jsonb)`.

## Offene Betriebsgrenze

Der vorhandene rclone-Remote verwendet noch die geteilte Google-Client-ID. Deren angekündigte Ablösung 2026 kann den Drive-Zugang beeinträchtigen. Ein Ersatz gehört nicht zu diesem Speichernachtrag. Der vorhandene Remote und seine Zugänge werden durch diesen Kandidaten nicht verändert.
