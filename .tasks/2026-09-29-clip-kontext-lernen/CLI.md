# Clip-Kontext-CLI

`clip_context_learn` gehört zum vorhandenen `tb-dashboard`-Paket, damit das
Werkzeug dieselbe normale Bot-Konfiguration und denselben geschützten
Infisical-FD-Bootstrap wie der Dashboard-CLI-Einstieg nutzt. Es liest keine
Config- oder Zugangsdaten aus Environment-Variablen und schreibt keine
Secrets in Argumente oder Dateien.

Der Aufruf braucht beide bestehenden Konfigurationspfade und muss wie der
Dashboard-Prozess mit dem vorhandenen Credential-FD gestartet werden:

```text
clip_context_learn --config <absoluter-bot-config-pfad> --uplink-config <absoluter-uplink-config-pfad> [--limit N] [--clip-id ID] [--backfill] [--force] [--learn-only] [--recommend ID]
```

Die bestehende Startverdrahtung muss dabei den von `credential_fd` benannten
Infisical-FD vererben. Das Werkzeug öffnet den FD selbst nicht neu und liest
keinen Token aus einer Environment-Variablen.

Die Uplink-Datei wird über `tb-dashboard-api::uplink_config` geöffnet. Die
schmale Laufzeitfunktion baut getrennte Lese- und Schreibpools aus dem
vorhandenen Twitch-Analytics-DSN und den normalen Pool-Zeitlimits. Jede neue
Quellverbindung erhält `default_transaction_read_only = on`; gelernt wird über
den getrennten Schreibpool. Helix wird nur für `--backfill` benötigt. Die
lokale Spracherkennung nutzt Endpoint, Modell und
Timeout aus `stt` in derselben Bot-Konfiguration; ein gesetzter entfernter
STT-Endpunkt wird abgelehnt.

`--learn-only` schreibt eine neue gelernte Vorlage aus bereits gespeicherten
Messungen. `--recommend ID` liest eine Empfehlung. Ohne diese Optionen erntet
das Werkzeug Messungen und aktualisiert anschließend die Vorlage. Dieses
Werkzeug schaltet keine Upload-Pipeline um und veröffentlicht keine Clips.
