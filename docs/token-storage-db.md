# Datenbankgebundene Tokenablage

Der eigene Chatbot speichert Access-/Refresh-Zugänge in twitch_bot_tokens. Ein Infisical-Seed ist nur die einmalige Erstübernahme; vorhandene oder widerrufene DB-Zeilen gewinnen. Der Dateifallback wird nicht gelesen. VOD-Resume-Adressen werden mit tb-crypto verschlüsselt und nach Abschluss entfernt. Der bestehende Social-Media-CredentialManager und beide YouTube-Verbindungen bleiben erhalten.

## Verbindliche Speichergrenze

Kontozugänge und wieder benötigte Geheimwerte werden nur feldverschlüsselt in der Datenbank gespeichert. Für reine Bearer-Prüfungen wird der Rohwert nicht zurückgewonnen, sondern ein irreversibler Lookup-Schlüssel verglichen. Die vorhandene tb-crypto-Implementierung bleibt die gemeinsame Feldkrypto. Konto- und Feldkontext müssen authentifiziert sein.

Keine neuen Token-JSONs, Refresh-Dateien, dotenv-Zugänge oder persistenten Browserprofile. Kein stiller Datei-, Klartext- oder Fremdkonto-Fallback. Keine Tokenwerte, verschlüsselten Blobs, Session-URIs oder Browserzustände in Logs, Berichten oder Shellargumenten.

Master-Key, OAuth-Anwendungssecrets, Datenbankverbindung und Infisical-Bootstrap bleiben außerhalb der Anwendungstabellen im bestehenden Secret-Manager. Sie sind keine zweite Ablage der rotierenden Kontotokens. Ein Verschlüsselungsschlüssel wird nicht neben seine eigenen Ciphertexte gelegt. TradingBot gehört nicht zu dieser Umstellung.

Bot und Dashboard lesen vor Clients genau einen privaten Infisical-Snapshot
über FD3. Die normale `infisical.json` liegt neben der unveränderten bot.toml;
sie enthält ausschließlich Quellenmetadaten und `secret_values_fd:3`.
Bot und Dashboard verwenden ausdrücklich ihren dienstbezogenen DSN, keinen
gemeinsamen DSN-Fallback. Der Masterkey wird einmal validiert und danach von
allen aktiven Cipher-Factories aus demselben RAM-Cache verwendet. Ein erneutes
Initialisieren oder ein fehlender Wert bleibt gesperrt. Botseed, Helix/Webhook,
Session-, OAuth-/Broker-Login und bestehende LLM-Schlüsselgetter verwenden
diesen Snapshot; der Modellresolver und Betriebsentscheidungen bleiben erhalten.
Dashboard-Uplink verwendet dieselbe Momentaufnahme und prüft die Gleichheit der
normalen Quellenmetadaten, ohne zweiten Credentialread oder Connectoraufruf.
`--check-private-config` prüft den Startvertrag vor Pool/Jobs/Providerzugriffen;
mit `--wait-for-stop` wartet die Offlineprüfung nach ihren secretfreien Flags
auf SIGTERM, damit Identität, Sandbox und normale Stopweitergabe messbar sind.
Clip-Kontexternte initialisiert ebenfalls ihren vorhandenen gemeinsamen
Snapshot. Categorycollector und Streamaudit verwenden keine Mastercipher;
der unveränderte Legacy-Auditstart bleibt ausdrücklich außerhalb dieses
Startfixes. Nach privater LLM-Getterregistrierung gibt es keinen ENV-Rückfall.
Die zwei vorhandenen unterschiedlichen Infisical-Transport-Vendorbäume bleiben
unverändert im Code. Twitchs Packageversion0.1.1 unterscheidet sie im Lockfile
von Bots0.1.0; keine neue Transportimplementierung oder Providerverbindung.

## Lokaler Arbeitsstand

Die zusammengehörigen Worktrees liegen als Geschwister unter /home/nathanael/.worktrees/token-db-local-20260930/. Die vorhandenen relativen Abhängigkeiten auf Deadlock-Bots und tb-crypto werden dort wiederverwendet. Quellkopien, ein zweites Kryptopaket und Änderungen an geteilten Checkouts sind nicht nötig. Alle Rust-Prüfungen verwenden den vorhandenen sccache und die gemeinsame Zwischenablage /home/nathanael/.cache/rust-build/{workspace-path-hash}.

Produktive Datenbanken und Konten wurden für diesen Arbeitsstand nicht migriert. Änderungen sind keine Aussage über die laufenden Dienste. Tests dürfen nur explizite Wegwerf-Datenbanken und synthetische Konten verwenden.

## Späterer koordinierter Cutover

1. Zugehörige neue Leser und Writer gemeinsam bereitstellen, vorhandene verschlüsselte Sicherung und Wiederherstellungsweg prüfen. Keine angewandte Migration ändern.
2. Alte Writer anhalten. Neue zentrale Schema-Migrationen anwenden. Gehashte Session-IDs nicht mit einem alten Consumer mischen. Bestehende Restore-/ETL-Werkzeuge vor einem Import auf das neue Tokenformat abstimmen; die Constraints lehnen Rohwerte ab.
3. Bestehende Steam-Guard-Werte ausdrücklich per privater Pipe an das Beispielprogramm import_guard im Steam-Core geben. Es nutzt dieselbe Kontokonfiguration und denselben Secret-Launcher wie der Dienst. Gleiche Freigaben dürfen erneut importiert werden, andere bestehende Werte und Widerrufe werden nicht überschrieben. Keine alten Dateien durch den Agenten öffnen.
4. VOD-Resume-Werte vor dem Start über den vorhandenen Bot-Secret-Launcher mit `--migrate-token-storage --apply` und der normalen Bot-Betriebsdatei transaktional umstellen. Der Wartungsmodus startet keine Writer oder Hintergrundjobs. Beim separaten Archiv gilt `--migrate-token-storage`. Danach den NOT-VALID-Constraint der Twitch-Tabelle validieren. Ein Fehler lässt den jeweiligen Migrationsbestand unverändert. Abgeschlossene Uploads behalten ihre Video-ID.
5. Kontozuordnung, Entschlüsselung und Neustart-Wiederaufnahme prüfen. Erst danach alte Credential-Dateien oder Bootstrap-Kontotokens kontrolliert außer Betrieb nehmen. Die vorhandenen Dateien werden hier weder gelöscht noch als Backup verdoppelt.

Ein Code-Rollback allein reicht nach einem irreversiblen Hash-Cutover nicht. Entweder die neuen Lookup-Verträge beibehalten oder gemeinsam auf einen zuvor geprüften Datenbankstand zurückgehen. Ein nicht durchgeführter Restore-Test ist keine bestätigte Rollback-Fähigkeit.
