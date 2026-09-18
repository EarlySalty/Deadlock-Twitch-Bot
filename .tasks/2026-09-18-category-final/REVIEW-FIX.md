# Nachprüfung: indexierte Entfernung und sichere Wiederaufnahme

Die erste unabhängige Prüfung erlaubte den ursprünglichen Stand. Die Prüfung des mit main integrierten Standes fand anschließend einen berechtigten Skalierungsblocker: Der Shared-Chat-Zweig einer gezielten Nachrichtenentfernung hatte keinen passenden Index. Dieser Befund wurde nicht übergangen.

## Echte Rot-/Grün-Probe

Neuer Test `targeted_removal_has_bounded_index_work_in_a_large_archive` verwendet eine isolierte PostgreSQL16-Instanz mit 60.000 synthetischen Shared-Chat-Zeilen. Er bereitet den tatsächlichen SQL-Funktionsrumpf aus `pg_proc.prosrc` vor, erzwingt einen generischen Abfrageplan und führt `EXPLAIN (ANALYZE, BUFFERS)` aus. Kein abgeschriebenes Ersatz-DELETE, keine Produktion und kein abgeschalteter Seq-Scan-Planner.

Vorher: 1 Test fehlgeschlagen. Eine gezielte Entfernung filterte 60.000 unbeteiligte Zeilen der großen Partition aus. Gemessene Ausführungszeit dieses Laufs: 106,601 ms.

Korrektur: Partieller Ausdrucksindex auf `source-room-id` und `source-id`, einschließlich ausdrücklich übereinstimmendem Teilindex-Prädikat im Funktionsrumpf. Die Änderung liegt in der noch nicht produktiv angewandten Archivmigration. Der neue Index wird an die täglichen Partitionen vererbt.

Nachher: 1 Test bestanden. Der generische Plan verwendet BitmapOr und den neuen Ausdrucksindex. In der großen Partition wird ein Datenblock gelesen; die einzige zusätzlich herausgefilterte Zeile liegt in der winzigen historischen Testpartition. Exakt die bezeichnete Nachricht wird entfernt, 59.999 synthetische Nachbarzeilen bleiben. Gemessene Ausführungszeit dieses Laufs: 2,354 ms. Das ist ein Abnahmefall, kein allgemeiner Produktionsbenchmark oder garantierter Geschwindigkeitsfaktor.

Die vollständige Archiv-Testgruppe mit Rollen-, Retention- und CLEARCHAT-Fällen besteht danach mit 6 Tests; die Speicher-/Rollup-/Shared-Chat-Gruppe mit 2 Tests ebenfalls.

## Wiederaufnahme nach Speicherpause

Ein zusätzlicher Review-Hinweis wurde behoben: Nach einer über Mitternacht reichenden Speicherpause werden fehlende Tagespartitionen jetzt vor Freigabe der Writer angelegt, statt erst am nächsten Zehn-Minuten-Tick. Der Entscheidungsweg ist separat getestet. Bestehende Daten werden dabei nie gekürzt.

## Weitere Hinweise

Der Betrieb ist ausdrücklich auf den geprüften lokalen PostgreSQL-Host zugeschnitten; die echte Zugänglichkeit von `/var/lib/postgresql` wird unter `twitchcollector` bei der Installation geprüft. Bei DB-Ausfall bleiben Antworten gesperrt bzw. startet der Sammler über systemd neu, statt Daten oder Berechtigungen zu erraten.

Die Header- und Auto-Unraid-Änderungen im Merge stammen aus dem inzwischen weiterentwickelten main. Sie wurden übernommen, um keinen bestehenden Fortschritt zurückzurollen, und nicht als zusätzliches Collector-Feature umgebaut. Der Kompatibilitäts-Stub gegen alte Rust-Retention-Aufrufe bleibt absichtlich ohne Löschwirkung.

Die zusätzlich angekündigten API-Prüfungen sind inzwischen bestanden: 2 Handler-Tests und 1 Test gegen die ehemalige Befehlsausnahme für bekannte Bots. Auch der abschließende Clippy-Lauf war erfolgreich; bestehende Hinweise betreffen unveränderten Uplink-/Chat-Code. Live-Nachweise werden erst nach tatsächlichem Deployment eingetragen.
