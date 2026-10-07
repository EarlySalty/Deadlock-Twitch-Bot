# Umsetzung

1. Den bestehenden Rust-Archivworker und PostgreSQL-Datenbestand weiterverwenden. Standalone-Funktionen und reale SQLite-Einträge abgleichen, den Nebenbestand nicht erneut importieren.
2. Listen- und Aktionsroute im vorhandenen Social-Media-API ergänzen. Den Kanal serverseitig aus der Sitzung begrenzen und aktive Bearbeitung mit demselben PostgreSQL-Advisory-Lock wie der Worker schützen.
3. Den Archivtab in die bestehenden Partner- und Verwaltungsseiten einsetzen. Kanalzustand, Teile und bestätigte Ziellinks anzeigen; erneuten Versuch, ausdrückliche Drive-Wahl und Ausblenden anbieten.
4. Betriebsoptionen typisieren, Playlist-Zuordnung ergänzen und temporäre Dateien nach bestätigtem Zielstand entfernen. Den gemergten Verbindungszustand aus Aufgabe A wiederverwenden; den separaten EventSub-Export nicht mehr aufbauen.
5. Nach Build-, Datenbank-, Sicht- und Gate-Prüfung integrieren, migrieren, über den vorhandenen Wrapper ausliefern und live prüfen. Den alten Dienst und alte Python-Anwendungsdateien entfernen, das Nebenrepo bis zur eigenen Löschfreigabe erhalten.
