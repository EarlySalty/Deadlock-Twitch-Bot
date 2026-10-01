# Direkte Twitch-/Steam-Verknüpfung für Zuschauer

Stand: 2026-09-18. Ergänzt den vorherigen Auftrag für @user-Ziele bei Chat-Commands.

## Umsetzung

- `!connect` liefert einen allgemeinen Link nach `/twitch/connect`, niemals ein Login-Token im öffentlichen Chat.
- Separater Zuschauer-Login via bestehendem Twitch-OAuth-Callback. Eigener Session-Typ und eigenes Cookie; kein Partner-/Admin-Zugang und keine Discord-Pflicht.
- Steam OpenID 2.0 über festgelegten Provider, geprüfte signierte Felder, exakte Rücksprungadresse, Browserbindung, kurzlebigen Einmal-State und serverseitige `check_authentication`-Prüfung. Globale Nonce-Sperre verhindert Wiederverwendung.
- Bestätigte Twitch-ID und SteamID64 werden separat von Discord-Verknüpfungen gespeichert. Ein direkter Link hat bei `!rank @user` Vorrang. Öffentliche Rangdaten kommen aus der vorhandenen Deadlock-API-Anbindung.
- `!unconnect` und `!disconnect` arbeiten ausschließlich auf der ID des Absenders; Zielargumente werden abgelehnt. Auf der Website gibt es dieselbe Funktion mit Session-/CSRF-Schutz.
- Beim Trennen wird die direkte Steam-ID gelöscht. Twitch-ID und Abschaltvermerk bleiben bestehen, damit Discord-/Namens-Fallback die Zuordnung nicht erneut herstellt. Alte laufende Steam-Callbacks können ein Trennen nicht rückgängig machen. Auch eine noch laufende direkte Rangabfrage prüft den Zuordnungsstand vor ihrer Antwort erneut.
- Die übrigen Spielerstatistiken bleiben am bisherigen Discord-/Steam-Datenweg. Bei abweichendem direkt verbundenem Konto wird keine Statistik des alten Kontos angezeigt. Watchtime benötigt keine Steam-Verknüpfung.
- Datenbankrechte: Webdienst darf bestätigte Steam-IDs schreiben. Bot darf nur lesen bzw. die eigene Verbindung im Command trennen, aber keine Steam-ID zuweisen. Ein Trigger entfernt beim Deaktivieren die Steam-ID. Nonces sind nur für den Webdienst zugänglich.
- Kontoseite, Command-Katalog und Benutzer-/Technikdokumentation aktualisiert.

## Verifikation

Alle Datenbankprüfungen liefen gegen private temporäre PostgreSQL-16-Prozesse, nicht gegen Produktionsdaten. HTTP-Identitätsanbieter und öffentliche Rangabfragen wurden mit Wiremock getestet.

- Abschließender Chat-/Rank-/Store-/Katalog-Testlauf: 96 bestanden, 0 fehlgeschlagen.
- Abschließender Web-/OpenID-Testlauf: 16 bestanden, 0 fehlgeschlagen.
- Vollständiger frischer Migrationslauf mit Schema-Snapshot-Vergleich: 1 bestanden.
- Rollenprüfung auf der vollständig migrierten privaten Datenbank: erfolgreich; Web schreibt Steam-ID, Bot entfernt sie, Bot kann keine Steam-ID setzen und keine Nonces lesen, Legacy-Rolle kann Zuordnungen nicht lesen.
- `cargo check -p tb-bot -p tb-dashboard-api -j 2`: Exit 0.
- `cargo clippy -p tb-chat -p tb-dashboard-api --all-targets --no-deps -j 2`: Exit 0 mit vorhandenen Warnungen außerhalb der neuen Module.
- `git diff --check`: Exit 0.
- Der vorherige vollständige Chat-Testlauf erreichte in anderen Moderationstests ein Zeitlimit. Keine Aussage, dass die gesamte Workspace-Suite bestanden hätte.

Der Schema-Snapshot enthielt zusätzlich 14 fehlende Spalten der bereits vorhandenen Migration `20260914203000_title_generator_preferences.sql`. Diese nachweislich bestehenden Spalten wurden im Snapshot ergänzt. Keine bereits veröffentlichte Migration wurde verändert.

Private DB-/Rechteprüfung wiederholbar mit `.tasks/player-connect-validate-db.py`. Lokale Protokolle liegen unter `.tasks/player-connect-*.log`.

## Live-Freigabe noch blockiert

Die installierten Laufzeitdienste migrieren absichtlich nicht selbst. Die neuen Tabellen und Rollenrechte müssen über `deadlock-twitch-migrate.service` angelegt werden. Der vorhandene `deploy-twitch-release`-Wrapper aktiviert den Release und startet Laufzeitdienste, ruft aber keinen Migrator auf; `bot-restart` erlaubt den Migrationsdienst nicht. Die root-eigenen Wrapper liegen außerhalb der schreibbaren MCP-Roots.

Daher keine Live-Aktivierung mit fehlendem Schema. Keine Produktions-DB-Schreibzugriffe und keine Service-Neustarts durch diesen Auftrag. Für die Freigabe muss der bestehende scoped Deploy-Ablauf um den geprüften Migrationsdienst vor den Laufzeit-Neustarts ergänzt werden; keine generische sudo-/systemctl-Freigabe. Danach sind ein echter Release-Build, Deployment und Live-Proof erforderlich.

Ein echter persönlicher Twitch-/Steam-Login wurde nicht im Namen des Nutzers durchgeführt. Er ist der verbleibende persönliche End-to-End-Abnahmeschritt nach erfolgreichem Deployment.

Protokollreferenzen: Steamworks User Authentication and Ownership (OpenID), Twitch Authorization Code Grant und OpenID Authentication 2.0.

## Fortsetzungsprüfung 2026-09-18

- Die Implementierung war während der Prüfung bereits als `06acf95e` committed und über `59ae37db` mit `origin/main` zusammengeführt worden. Fremde Arbeitsstände wurden nicht zurückgesetzt.
- Den vollständigen relevanten Command-/Katalog-/Stats-/Rank-/Player-Link-Testlauf erneut ausgeführt: **170 bestanden, 0 fehlgeschlagen, 0 ignoriert**.
- Die wiederholbare private Datenbankprüfung führt jetzt auch die bisherigen Auth-/Session-Regressionen und die neuen Connect-/OpenID-Tests mit `TB_TEST_REQUIRE_DB=1` aus: **86 bestanden, 0 fehlgeschlagen, 0 ignoriert**. Das Tool erreichte beim Übersetzen sein Antwortzeitlimit; das vollständige Protokoll `.tasks/player-connect-auth-regression.log` dokumentiert den abgeschlossenen erfolgreichen Lauf. Der frische Migrations-/Schema-Test und die Rollenassertionen waren ebenfalls erfolgreich.
- `rank_lookup/tests.rs` band bereits `player_tests.rs` ein, die Datei war jedoch noch unversioniert. Sie wird mit den vier geprüften Tests für direkte Zuordnung, Opt-out, Cache und laufende Abfragen aufgenommen, damit ein frischer Checkout keine fehlende Testquelle hat.
- Doppelte Connect-Dokumentation bereinigt; Vorrang der direkten Verknüpfung und die verbleibende Twitch-ID mit Abschaltvermerk ausdrücklich beschrieben.
- Live-Sperre nochmals abgeglichen: `bot-restart` listet keinen Migrationsdienst. Der installierte `/usr/local/bin/deploy-twitch-release` liegt außerhalb der erlaubten MCP-Dateiwurzeln und konnte hier nicht geprüft oder aktualisiert werden. Keine Ausweichroute über generisches sudo/systemctl, keine Live-Schaltung mit ungeprüftem Migrationsablauf.
- Persönlicher Login bei Twitch und Steam weiterhin nicht stellvertretend ausgeführt.
