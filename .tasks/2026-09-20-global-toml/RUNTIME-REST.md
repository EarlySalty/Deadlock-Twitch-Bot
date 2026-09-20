# Integrationsstand und Rollout-Sperren

Stand 20.09.2026. Branch `feat/twitch-toml-rollout-20260920`. **Kein fertiger vollständiger TOML-Rollout. Nicht isoliert deployen.**

## Angeschlossener Kern

Bot und Dashboard laden vor Clients/Jobs dieselbe geprüfte Datei über `--config`. Angeschlossen sind DB-Poolgröße und Zeitlimits, interner Listener, Dashboard-Listener, Broker-Basisadresse sowie im Bot Zielspiel, Sprachfilter, EventSub-Callback und Discord-Ankündigungsziel. Das Logging-Level kommt aus der geprüften Datei. Die Infisical-DSN-Auswahl für getrennte Dienstkonten bleibt im bestehenden Startpfad erhalten. `--check-config` endet vor Secretzugriff und Listenerstart. Der bestehende separate Uplink-FD-/Migrationsvertrag bleibt erhalten.

Die Startskripte erwarten `/var/lib/deadlock-twitch/config/bot.toml`. Es wurde absichtlich keine produktive Datei aus unbelegten Defaults angelegt. Ohne diese Datei endet der Start bereits in der Vorprüfung. Damit darf dieser Zwischenstand noch nicht in einen normalen produktiven Release gelangen.

## Bedienoberfläche

Unter `/twitch/admin/config/operating` können ausschließlich diese tatsächlich angebundenen DB-Werte geändert werden:

- maximale Verbindungen je Dienst;
- Zeitbudget zum Erwerb einer Poolverbindung in Millisekunden;
- Verbindungsaufbau-Zeitbudget in Sekunden.

Die Route `/twitch/api/admin/config/operating` hängt am vorhandenen Admin-/CSRF-Router. Kein neuer Login, kein beliebiger Dateipfad, keine Modelle und keine Zugangsdaten im Payload. Bei Konflikten wird nichts überschrieben. Speichern ist kein Neustart. Der Dashboard-Snapshot und der über die authentifizierte Bot-Health-API gemeldete Fingerprint werden einzeln ausgewiesen; unbekannter Bot-Stand bleibt unbekannt. Steam und Patchnotes werden nicht als angebunden dargestellt.

Der Dateieditor erwartet ein persistentes Verzeichnis außerhalb Git, das dem Dashboard-Schreibkonto gehört. Datei und gemeinsame Lesegruppe müssen beiden Twitch-Diensten Leserechte geben. Besitzer, Gruppe und Modus werden beim Ersetzen erhalten; der Editor erhöht keine Rechte. Diese Betriebsrechte wurden noch nicht installiert.

## Vor Merge/Deploy noch erforderlich

1. Die wirksamen Nicht-Secret-Werte sicher rekonstruieren und abgleichen. Die abgefragten vorhandenen APIs liefern keinen globalen Runtime-Snapshot. Selbst die Kernwerte `database.pool_max`, `database.acquire_timeout_ms`, `database.connect_timeout_seconds` und `logging.level` sind bei möglichen Overrides bisher nicht belegt. Quell-Defaults sind kein Beweis des Live-Stands. Keine Prozess-Environments oder ENV-Dateien lesen, auch nicht gefiltert.
2. `KLASSIFIKATION.md` weiter auflösen: insbesondere 80 dynamische Hilfsleser mit ihren tatsächlichen Aufrufern. Die Ursprungsliste mit 98 direkten Betriebslesern ist vor dem Kernanschluss erfasst und dient als Arbeitsmatrix, nicht als aktueller Restzähler.
3. Fachverbraucher migrieren: Chat-/Raid-/Scout-/Monitoring-Schalter; OAuth-/öffentliche Redirect-/Cookie-/Adminwerte; Engagement-/Transkript-/Last-/Retry-/Retention-Regeln; Social-/VOD-/Coaching-/Archivpfade; Billing-/SMTP-Betriebsparameter. Credential-Leser bleiben getrennt. Modell-/Providerpolitik nicht erweitern oder wechseln.
4. `rust/scripts/run_tb_bot_service.sh` und `run_tb_dashboard_service.sh` enthalten für diese noch offenen Fachverbraucher weiterhin alte Runtime-Quellen und Exporte. Diese Quellen erst nach fachlicher Verbraucher-Migration vollständig entfernen. Das ist derzeit ein echter offener Auftragsteil, kein behaupteter vollständiger ENV-Ausstieg.
5. Coaching, Category-Collector und STT an ihre tatsächlich belegten Werte und Startpfade anschließen. Der übernommene STT-Launcher und Python-Kandidat sind getestet, aber nicht produktiv installiert. Keine automatische Aktivierung externer Transkriptverarbeitung.
6. Berechtigte persistente Betriebsdatei, Rechte und vorhandene Uplink-Argumente gemeinsam prüfen. Kein Deploy darf bestehende Bedienerwerte durch Vorlagen ersetzen.
7. Unabhängiger Rust-/Python-/Security-/Intent-Review und Merge-Gate für den zusammengehörenden Lese-/Schreibpfad. Frontend-Build ist geprüft; echte Browser-Sichtprüfung fehlt, weil keine Browser-Verbindung verfügbar ist.
8. Erst danach koordinierter Release-Build, Migrationen/Deployment, beide Systemdienste neu starten und Fingerprints, Funktionen, Readiness und Heartbeats live messen. Worktree/Branch erst nach tatsächlichem Abschluss bereinigen.

Keine produktive Änderung, kein Modellwechsel und keine Community-Nachricht durchgeführt.
