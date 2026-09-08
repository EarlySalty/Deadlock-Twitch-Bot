# Uplink im vorhandenen Dashboard

Arbeitsstand, kein Produktions- oder OBS-Abnahmenachweis. Einstieg bleibt `/twitch/uplink`; Proxy, bestehende Twitch-Sitzung und vorhandene OAuth-Flows bleiben im Dashboardprozess. Benutzerkennungen kommen ausschließlich aus der bestätigten Twitch-ID. Ein Anzeigename ersetzt keine Identität.

## Normaler Dienstanschluss

Der bestehende Launcher `rust/scripts/run_tb_dashboard_service.sh` startet den Systemdienst `deadlock-twitch-dashboard-rust` (Nutzer `twitchdash`, Arbeitsverzeichnis `/opt/deadlock/twitch/current`). Er öffnet dessen eigene vorhandene `/run/credentials/deadlock-twitch-dashboard-rust.service/infisical-token` ausschließlich als FD9 und reicht `--uplink-config /etc/deadlock-twitch/uplink.json` an den Ruststart weiter. Ein ausdrücklich übergebener Konfigurationspfad bleibt erhalten; andere Argumente werden weitergereicht und vom Rustvertrag geprüft. Diese normale JSON-Datei enthält keine Zugangsdaten:

```json
{
  "relay_base_url": "http://127.0.0.1:8891",
  "infisical_base_url": "http://127.0.0.1:8080",
  "project_id": "vorhandene-projekt-id",
  "environment": "vorhandene-umgebung",
  "secret_path": "/vorhandener-pfad",
  "credential_fd": 9
}
```

Adressen sind Beispiele für interne Verwaltung, keine neue öffentliche Einrichtung. Optional stehen `kick_redirect_uri` und `youtube_redirect_uri` ebenfalls in dieser Datei; die bisherigen HTTPS-Callbacks sind Vorgaben. Der autorisierte vorhandene Startprozess übergibt seine eigene Infisical-Identität im expliziten FD. Keine geliehene Credential-Datei einer anderen Unit und kein Secret in Argumenten, ENV oder einer neu geschriebenen Datei. Der Leser verlangt eine begrenzte reguläre FD-Quelle, schützt auch den Original-FD mit CLOEXEC und hält Werte nur im RAM. Anonyme memfd-Quellen sind in den Tests tatsächlich geprüft.

Infisical wird über den bestehenden `/api/v4/secrets/`-Vertrag gelesen. Vorhandene API- und Adminzugänge bleiben getrennt; kein Admin-Fallback auf API-Zugang. Lokale Werte haben Vorrang vor importierten Werten. Fehlende Konfiguration, Redirects, zu große oder unvollständige Antworten ergeben sichtbare Fehler. Der Proxy übermittelt weder rohe Fehlertexte noch fremde Antwortkörper mit möglichen Zugangsdaten. Erfolgreiche Antworten müssen gültiges JSON sein. Ein Ziel-DELETE benötigt ein ausdrückliches boolesches `deleted`; eine unbekannte Route beweist keine Entfernung.

## Status und Auth

`service_status=ready` bedeutet Steuerungsbereitschaft. Der öffentliche RTMPS-Server und der private Ingestschlüssel kommen getrennt vom normalen Dienst. `enabled` bleibt ein gespeicherter Wunsch. `sending` zeigt lokalen Medienfluss; öffentliche Live-Schaltung verlangt `publication_confirmed`. Die Headeranzeige `Twitch live/offline` bezieht sich ausdrücklich auf die vorhandene frische Twitch-Beobachtung. `active_profile` bleibt ohne gemessenen Nachweis leer.

Der interne Broker liefert gültige Uplink-Grants mit ihren tatsächlichen Scopes. Er sperrt Titel-/Kanalpunktfunktionen nicht pauschal wegen fehlender Chatrechte. Fehlercodes: `connection_missing`, `reauthorization_required`, `temporarily_unavailable`; keine alte Scope-Liste als Ersatz nach einem Lesefehler.

Vereinbarte Authkopplung: Erst nach bestätigt entferntem Relay-Ziel ruft der Bot `AuthWriter::disconnect_uplink` auf. Das deaktiviert ausschließlich die Uplink-Nutzung. Der gemeinsame Twitch-Grant für Raid-/Botfunktionen bleibt erhalten; kein pauschaler Widerruf. Token, Scopes und Uplink-Intent werden gemeinsam über `load_decrypted_with_scopes` gelesen. Ein neuer bewusster Uplink-OAuth-Dialog kann diesen Intent wieder aktivieren; Base-/Raidcallback und Refresh tun das nicht. Schreibpfad und Migration `20260908210000_twitch_uplink_intent.sql` stammen aus dem gemeinsam integrierten Authblock; die Migration muss vor dem Dienststart angewendet sein.

## Reproduktion

Frontend: `npm test` und `npm run build` in `bot/dashboard_v2`.
Browser: `node --import tsx tests/uplink.browser.test.mjs` nach dem Build. Ausschließlich eigener Loopbackserver, eigener Browserprozess und synthetische Daten; vorhandene Nutzerprofile und echte Cookies werden nicht verwendet. Nachgewiesen sind Kopieren/Verdecken, bestehender OAuth-Link, Profileingabe per Tastatur, gespeicherte Rückmeldung, verspätete Saveantwort und tatsächlich abgewarteter Statuspoll bei offenem ungespeichertem Formular. Bilder und Bericht liegen in `tests/artifacts/uplink/`; sie sind Prüfarbeitsdateien, kein öffentlicher Livetest.

Rust: fokussierte Tests in `tb-dashboard-api`; PostgreSQL16 startet pro Datenbankfall in einem eigenen privaten Unix-Socket-Verzeichnis ohne ENV oder Live-DSN. Die Prüfung darf bei fehlendem PostgreSQL nicht still übersprungen werden. Infisical-FD/HTTP-Prüfungen verwenden ausschließlich synthetische Werte.

## OBS: konkrete offene Zweispur-Einrichtung

Am 8. September 2026 erneut auf OBS-Commit `6b3e550729f125b6c5b3767df88c08f5aef9d264` geprüft:

- [`OBSBasicSettings_Stream.cpp`](https://github.com/obsproject/obs-studio/blob/6b3e550729f125b6c5b3767df88c08f5aef9d264/frontend/settings/OBSBasicSettings_Stream.cpp): Die GUI unterstützt „Twitch“ mit benutzerdefiniertem Server. Beim eigenständigen Dienst „Benutzerdefiniert“ hängt die VOD-Auswahl dagegen an `General/EnableCustomServerVodTrack` aus `GetUserConfig()`, außerhalb des Streamprofils.
- [`services.json`](https://github.com/obsproject/obs-studio/blob/6b3e550729f125b6c5b3767df88c08f5aef9d264/plugins/rtmp-services/data/services.json) und [`rtmp-common.c`](https://github.com/obsproject/obs-studio/blob/6b3e550729f125b6c5b3767df88c08f5aef9d264/plugins/rtmp-services/rtmp-common.c): Der gewöhnliche Twitch-Dienst nennt H.264. Eine frei eingetragene Serveradresse hebt diesen Codecvertrag nicht auf. Daraus folgt kein AV1-Weg mit Twitch-GUI und zwei Audiomischungen.
- [`AdvancedOutput.cpp`](https://github.com/obsproject/obs-studio/blob/6b3e550729f125b6c5b3767df88c08f5aef9d264/frontend/utility/AdvancedOutput.cpp): Der normale Ausgabepfad kann eine zweite Audioencoderrolle anfügen, verlangt dafür unterschiedliche Mixerzuordnungen und prüft beim Customdienst denselben globalen Schalter.

Folge: Ein importiertes Streamprofil allein löst den globalen Schalter nicht. AV1 plus zwei sauber ausgewählte Mischungen ist mit diesen Quellen noch kein rein über das Dashboard abgeschlossener Standard-OBS-Bediennachweis. Keine erfundene GUI-Einstellung und kein verpflichtendes Plugin daraus ableiten. Die unterstützte OBS-Version, konkrete globale Einstellung und tatsächliche Live-/VOD-Ausgabe müssen zusammen nachgewiesen werden. Die VOD-Anforderung bleibt bestehen.

Ausführbare FD9-Probe: `cargo run -j2 -p tb-dashboard-api --example uplink_start_probe` aus `rust`. Sie ruft die tatsächliche Launcherfunktion auf und führt in einem Rust-Kindprozess dieselbe `load_arguments`-Funktion wie `tb-dashboard` aus. Es gibt nur eine normale temporäre JSON-Konfiguration und eine anonyme memfd mit synthetischen Werten. Die restliche Legacy-Konfiguration benachbarter Dashboardfunktionen ist damit nicht pauschal migriert.

## CI und lokale Abschlussprüfung

Der bestehende Workflow `.github/workflows/rust-sqlx-check.yml` enthält jetzt einen Uplink-Auth-Testjob auf Ubuntu24.04. Er installiert ausdrücklich die Hostprogramme aus [postgresql-16](https://packages.ubuntu.com/noble/postgresql-16) und führt Authtransaktionen, Callback-Rennen, Transportvalidierung, Broker, Uplink-Trennung und die FD9-Startprobe aus. Die bisherigen Offlinebuild-/Schemaschritte bleiben erhalten; der neue Job verwendet keine neue ENV-Konfiguration. Actionlint ist lokal grün. Dies ist noch kein ausgeführter GitHub-CI-Nachweis.

Die Vorschau verwendet denselben derzeitigen Caps-Vertrag wie der neue Dienst: Empfehlungen und aktive Profile bleiben ohne Quellen-/Zielnachweis null. Alte max-Werte werden nicht zu Empfehlungen umgedeutet. Eine passende Bitrate wird nicht aus einem gespeicherten Wunsch abgeleitet.

Lokaler Stand: 245 Frontendtests, Produktionsbuild und tatsächlicher synthetischer Browserlauf bestanden. Beide Bitraten-Regressionsfälle wurden zuerst rot gemessen. Der Desktop- und Mobilhilfeknopf steht im normalen Dokumentfluss. Statuspoll und verspätete Saveantwort behalten ungespeicherte Eingaben. API-Library-Clippy und die 18 gekoppelten Brokerfälle bestanden; die Refresh-Fixture enthält nun auch die vom atomaren Blacklist-Cleanup benötigte Partner-Tabelle. Der frühere Refreshfehler war damit ein fehlendes Testschema, kein gelockerter Authvertrag. Weitere Callback-ID-Abschlussprüfungen folgen vor dem Freeze.

## Eingefrorener gekoppelt geprüfter Stand

Der Hintergrundnachlauf erhält jetzt die bestätigte Twitch-ID aus dem bestehenden OAuthCallbackResult über den authentifizierten internen HTTP-Hop. Nur erfolgreich geprüfte und gespeicherte Grants liefern diese ID; der Dashboardempfänger verlangt 2xx und eine positive numerische ID. Der alte Login→ID-Nachschlag ist entfernt. Bei fehlender ID startet kein Hintergrundnachlauf; der vorhandene sichtbare Dashboard-Rückkehrweg verwendet weiterhin seine autorisierte Sitzung.

Abschluss am 8. September 2026: 18 Brokerfälle, 63 Uplinkfälle, 7 Config-/FD-Fälle, 33 interne OAuth-Routentests und 14 echte PostgreSQL-Bot-Callbacktests bestanden. Zusätzlich bestand der neue HTTP-Hoptest mit erfolgreicher ID, Fehlerstatus und fehlender ID sowie die Prüfung ungültiger numerischer Identitäten. Der gefilterte Dashboard-Callbacklauf enthielt außerdem unveränderte Legacy-DB-Tests mit ihrem bisherigen bedingten Datenbankaufbau; daraus wird kein zusätzlicher realer DB-Nachweis abgeleitet.

Die ausführbare FD9-Probe bestand erneut auf diesem Stand. Clippy für Dashboard-API, interne API und Dashboardbinary mit -D warnings ist grün. Der frühere API-Clippy-Lauf mit Tests scheiterte an zwei unveränderten Konstanten-Assertions in Self-Explainer; ein vollständiger Baseline-Nachlauf wurde nicht ausgeführt. Diese konkrete Prüfgrenze bleibt sichtbar. Frontend: 245 Tests, Produktionsbuild, Browserbedienung und erneuerte Desktop-/Mobilbilder grün. Workflow: actionlint grün, GitHubausführung noch ausstehend.

Nachweisdateien lokal: /tmp/uplink-broker-coupled-final.txt, /tmp/uplink-dashboard-uplink-final.txt, /tmp/uplink-dashboard-config-final.txt, /tmp/uplink-callback-id-red.txt, /tmp/uplink-callback-id-green.txt, /tmp/uplink-dashboard-id-final.txt, /tmp/uplink-dashboard-bot-final.txt, /tmp/uplink-launcher-probe-final.txt, /tmp/uplink-dashboard-clippy-coupled-final.txt, /tmp/uplink-dashboard-tests-final.txt, /tmp/uplink-dashboard-build-final.txt und /tmp/uplink-browser-final.txt. Eigener Gate folgt auf dem Integrationscommit; kein Git-/Produktiveingriff durch den Autor.

### Normaler Betriebsanschluss

`rust/deployment/uplink.json.example` ist die gewöhnliche Konfiguration für `/etc/deadlock-twitch/uplink.json`: gleiche Infisical-Identität, eigene Dashboard-Credential FD 9 und bestehende Relay-API auf Port 8891. Keine neue Tokenablage, kein anderer Systemdienst. Die aktive Uplink-Session wird alle fünf Sekunden mit dem vorhandenen `/me`-Aufruf geladen; SourceObservation und laufender Encodergraph sind vom gespeicherten Wunsch und von Twitch-live getrennt. Graph-Bitraten heißen ausdrücklich Zielbitrate. Ein laufender Uplink-Eingang hält private Kopierfelder auch dann verdeckt, wenn Twitch offline ist. Ungespeicherte Profilfelder bleiben bei diesem Poll erhalten.

`trennung_offen` bleibt sichtbar und verwendet erneut denselben bestehenden Trennen-POST. Ein Fehler löst einen Statusabruf aus, keine erfundene Erfolgsmeldung. Der Browsernachweis akzeptiert keine ausführbare CLI-Eingabe; er startet ausschließlich den festen vertrauenswürdigen Chromium-Build nach SHA256-Prüfung. Screenshots verwenden allein synthetische Testwerte, einschließlich des real implementierten `running_graph`-Vertrags. Finale Neuaufnahme nach Zusammenführung der eingefrorenen Zielgenerationskorrektur.
