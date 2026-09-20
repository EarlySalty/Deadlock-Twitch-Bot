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
