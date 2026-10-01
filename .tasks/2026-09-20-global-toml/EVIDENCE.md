# Nachweise zur zentralen TOML-Konfiguration

## Teilstand 1: Lader und Inventar

Stand: 20.09.2026. Dieser Teilstand ist vorbereitet, nicht produktiv angeschlossen.

### Geprüfter Umfang

Der bestehende Crate `tb-config` enthält einen begrenzten TOML-Leser, Schema-Version 1, typisierte Kernbereiche für Twitch-Identität, Datenbankpool, interne API, Dashboard-Listener, Broker und Logging sowie eine unveränderliche gemeinsame Momentaufnahme. `runtime_settings` übergibt die geprüften Pool-, Listener- und Broker-Werte an die bestehenden Verbraucher-Typen. Der zusätzliche Getter wird dabei ausschließlich nach benannten Zugangsdaten gefragt.

Die CLI `tb-config-check --config /absoluter/pfad/config/bot.toml` liest keine Zugangsdaten und startet keine Clients. Sie meldet nur Schema-Version, Fingerabdruck und Neustartpflicht. Unbekannte Felder, fehlende Pflichtwerte, falsche Typen, ungültige IDs, Ports, Zeitgrenzen und Zugangsdaten in URL-Strukturen werden abgelehnt. Fehler speichern keine TOML-Quellausschnitte oder verschachtelten Parserfehler. Debug der bestehenden Settings-Typen gibt keine Zugangsdaten mehr aus.

Relative interne Pfade beziehen sich auf das Verzeichnis der aufgelösten Config-Datei. Es gibt kein Hot-Reload: Ein vollständiger Recheck lässt die aktive Momentaufnahme unverändert und verlangt bei einer gültigen Änderung einen Neustart. Kein automatisches Überschreiben von Konfigurationsdateien.

### Tatsächliche Testläufe

- Unveränderte Baseline: `cargo test -p tb-config -p tb-llm -j 2`, Exit 0. 8 Config-Unit-Tests, 29 LLM-Unit-Tests und 1 LLM-Quellvertrag bestanden. Ein vorhandenes Dokumentationsbeispiel ist ignoriert. Job `j-1789863572-15`.
- Erster Build des neuen Laders: Exit 101 wegen des geänderten Hex-Format-Vertrags von SHA-2 0.11. Fehler korrigiert; kein Bestandsfehler umetikettiert.
- Abschließender Teilstand-Lauf: `cargo test -p tb-config -p tb-llm --features tb-config/inventory -j 2`, Exit 0. Insgesamt 62 Tests bestanden: 8 bestehende Config-Tests, 24 neue Datei-/CLI-/Verbraucher-Tests, 29 bestehende LLM-Tests und 1 LLM-Quellvertrag. Keine fehlgeschlagenen Tests. Das vorhandene ignorierte Dokumentationsbeispiel bleibt ignoriert.
- `cargo clippy -p tb-config --all-targets --features inventory -j 2 -- -D warnings`, Exit 0, einschließlich der neuen Prüf-CLI.

Die 24 neuen Tests prüfen insbesondere echte Kindprozesse mit widersprüchlichen Nicht-Secret-ENV-Werten, fehlenden expliziten Config-Argumenten, unterschiedlichen Arbeitsverzeichnissen und redigierter CLI-Fehlerausgabe. Das ist noch kein Nachweis, dass die alten produktiven Startpfade bereits migriert sind.

### Quellinventar

`tb-config-inventory <Rust-Quellwurzel> <neue JSON-Ausgabedatei>` liest Rust-Syntax, keine Prozessumgebung und keine Betriebsdateien. Erster Lauf: 236 direkte Zugriffsstellen und 534 Schlüsselvorkommen. Nichtliterale Leser werden ausdrücklich als dynamisch markiert. Die Schlüsselvorkommen sind Kandidaten, keine abgeschlossene fachliche Zuordnung. Das lokale Rohinventar `inventory-before.json` ist reproduzierbare Arbeitsausgabe und wird nicht als fertige Migrationsmatrix eingecheckt.

### Noch nicht nachgewiesen

Die Produktionswerte sind noch nicht vollständig übernommen. Der Kernlader ist noch nicht an Bot, Dashboard, Coaching oder STT angeschlossen. Die breite Verbraucher-Migration, die vollständige Schlüssel-/Prioritätsmatrix, der ENV-Regressionsvertrag, der Modell-Resolver, dessen fachliche Proben und Cache-Policy sowie die Gesamtabnahme stehen aus. Kein Main-Merge, kein Deploy, kein Live-Beweis.

## Übernahme im Integrationsworktree am 20.09.2026

Eigenanteil des ursprünglichen Worktrees auf `feat/twitch-toml-rollout-20260920` übernommen; Original-Worktree bleibt unverändert. Der alte Teilcommit ist als `131641bd` übernommen, STT-WIP und dessen Tests wurden kopiert. Keine produktive Konfiguration erfunden oder installiert.

Ergänzt: unveränderlicher Prozess-Snapshot in `runtime.rs` und begrenzter Dateieditor-Unterbau in `editor.rs`. Dieser nimmt nur die drei bereits im Kernschema enthaltenen DB-Betriebswerte an, validiert erneut das gesamte Schema, sperrt konkurrierende Schreiber, prüft den erwarteten Fingerabdruck, lehnt Git-Checkouts ab und speichert mit atomarem Rename und Datei-/Verzeichnis-fsync. Besitzer, Gruppe und Modus bleiben erhalten. Die aktive Momentaufnahme wird durch Speichern nicht geändert. Noch keine HTTP-Route und noch kein Dienstanschluss; dies ist ausdrücklich kein fertiger Runtime-Rollout.

Nachweise dieses Teilstands:

- `cargo test -p tb-config -j 2 --target-dir /home/nathanael/.cache/twitch-toml-rollout-target`: 43 Tests bestanden, Exit 0.
- `cargo clippy -p tb-config --all-targets -j 2 --target-dir /home/nathanael/.cache/twitch-toml-rollout-target -- -D warnings`: Exit 0.
- STT-Tests mit produktiv vorhandenem Interpreter `/home/nathanael/stt-tools/bin/python -B -m unittest discover -s ops/stt-server/tests -v`: 7 Tests bestanden. Keine Modelle heruntergeladen und keine echte Transkription gestartet. Der erste Versuch mit System-Python scheiterte an dessen fehlendem FastAPI; der STT-Interpreter enthält die benötigten Pakete.

Live-Abfrage des bestehenden Dashboards: `/twitch/api/admin/config/overview` und `/twitch/api/admin/system/health` liefern 200, enthalten aber keinen wirksamen globalen Betriebs-Snapshot. Poolgröße, Erwerbs-/Verbindungszeitbudget und Logging-Overrides sind daraus nicht rekonstruierbar. `KLASSIFIKATION.md` trennt das vorhandene reine Quellinventar in 98 direkte Betriebsleser, 49 Credential-Leser, 80 noch aufzulösende dynamische Leser, 6 Test-DSNs, 2 OS-Pfade und einen historischen Token-Dateipfad. Keine Prozessumgebung und keine ENV-Dateiinhalte gelesen.

## Kernanschluss und Adminseite

Bot und Dashboard sind im Integrationskandidaten an den Kern-Snapshot angeschlossen. Eine begrenzte Adminseite und API bearbeiten die drei DB-Betriebswerte. Der aktive Bot-Fingerprint stammt aus dessen Health-API. Die übrigen Verbraucher und produktiven Werte bleiben offen; vollständige Grenzen in `RUNTIME-REST.md`.

- `cargo check -p tb-bot -p tb-dashboard -j 2 --target-dir /home/nathanael/.cache/twitch-toml-rollout-target`: Exit 0. Erste Prüfung meldete zwei durch den Umbau ungenutzte Helfer; entfernt beziehungsweise auf Tests begrenzt, zweite Prüfung ohne Warnungen erfolgreich.
- `npm ci --ignore-scripts --no-audit --no-fund`, danach `npm run build` im eigenen `bot/admin_dashboard`: erfolgreich. TypeScript und Vite-Bundle vollständig gebaut. Vorbestehender Vite-Hinweis zu `__dirname` in der Buildkonfiguration bleibt.
- `npm test` im Admin-Frontend: 10 Tests bestanden.
- `bash -n` für beide Rust-Dienstwrapper und `git diff --check`: erfolgreich.
- Neue API-Tests prüfen Admin-Abweisung ohne Dateizugriff und Zurückweisen fremder Felder einschließlich Modelländerungen; deren tatsächlicher Testlauf wird gesondert nachgetragen.
- Browser-Sichtprüfung nicht möglich: laut Hauptsession kein verbundener Browser. Kein Screenshot oder tatsächliches Durchklicken behauptet.

## Review-Nacharbeit

- `cargo test -p tb-config --features inventory`: 45 Tests erfolgreich; einschließlich Kommentar-CAS und realem STT-Unterprozess aus zwei Arbeitsverzeichnissen.
- Admin-API: zwei Auth-/Schema-Tests erfolgreich.
- Admin-Frontend: TypeScript/Vite-Build erfolgreich.
- Visuelle Browserabnahme weiterhin offen: kein Browser verbindbar.
- Nach den Folgekorrekturen: `cargo check -p tb-bot -p tb-dashboard` erfolgreich.

Gate f8245c59 blockierte zu Recht die Python-Validierung aufgelöster lokaler Modellpfade. Folgefix unterscheidet vorhandene absolute Modellverzeichnisse von Hub-Bezeichnern. Rust-Unterprozesstest läuft jetzt aus einem Verzeichnis mit Leerzeichen/Umlaut gegen den echten Python-Argumentparser (6 STT-Rusttests grün). Python-Suite: 8 Tests grün, einschließlich lokalem Pfad mit mehr als 512 Zeichen und fehlendem Verzeichnis. Bestehende Framework-Deprecation-Warnungen unverändert.
