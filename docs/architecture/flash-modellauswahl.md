# Gemeinsame Flash-Modellauswahl

Der Nutzer hat stabile DeepSeek-Flash-Versionen bei Fireworks einschließlich zukünftiger Familienupdates freigegeben. Pro, Preview, Vision-Sonderversionen, Lite, Thinking, Distilled und fremde Anbieter sind ausgeschlossen. Der Anfangsstand ist V4.1 Flash. Kostenpflichtige Deployments werden niemals automatisch angelegt oder gestartet.

`tb-model-resolver` ist der einzige Katalogleser und Writer. Der systemd-User-Timer prüft täglich um 00:15 UTC (plus höchstens fünf Minuten Verzögerung). `Persistent=true` holt verpasste Termine nach. Ein Prozesslock verhindert parallele Läufe, der persistente Prüfstatus erlaubt genau einen Lauf pro UTC-Kalendertag. Ein später manueller Erstlauf blockiert deshalb nicht den nächsten Kalendertag, auch bei Zeitumstellung.

Der offizielle paginierte Fireworks-Katalog liefert ausschließlich öffentliche, READY und tatsächlich serverlose Kandidaten mit Bild- und Tool-Unterstützung. Numerische Modellversionen haben Vorrang vor Veröffentlichungszeitpunkten; fehlende Zeitstempel dürfen neuere Versionen nicht verdrängen. Uneindeutige gleiche Versionen werden nicht lexikalisch geraten. Nur eine erfolgreiche synthetische JSON-Inferenz mit `denken_aus` und 32 Tokens kann eine Auswahl veröffentlichen. Die Probe verwendet den vorhandenen zentralen tb-llm-Transport mit exakt dem Kandidatenmodell; sie benötigt noch keinen Auswahl-State. Fehler lassen die bisherige Auswahl unverändert. Fehlermeldungen sind auf eine pro Tag und zwei pro Woche begrenzt; die nächste Meldung nennt die unterdrückten Wiederholungen.

## Readervertrag 0.1.0

Alle Consumer verwenden das eigenständige Crate `fireworks-model-selection`. `selected_model() -> Result<String, SelectionError>` liest beim nächsten Request lokal neu; `read_selection(path: &Path, trusted_uid: u32)` ermöglicht normale explizite Config- und Testpfade. Kein Consumer fragt den Modellkatalog ab und keine Auswahl wird über ENV gelesen.

Die gemeinsame normale Datei `/var/lib/deadlock/llm-model-selection.json` enthält ausschließlich öffentliche Metadaten:

```json
{"schema_version":1,"provider":"fireworks","model":"accounts/fireworks/models/deepseek-v4p1-flash","release":null,"probed_at":"2026-10-01T02:00:00Z","checked_at":"2026-10-01T02:00:00Z"}
```

Die Datei ist höchstens 4096 Bytes groß, regulär, kein Symlink oder zusätzlicher Hardlink und nicht gruppen- oder allgemein schreibbar. Zulässige Besitzer sind root und der zentrale Writer mit UID1000; auch die Elternverzeichnisse sind geschützt. Ungültiges Schema, anderer Anbieter, Modelle unter V4.1 oder außerhalb der Familie und Zukunftszeitstempel werden abgewiesen. Der Reader öffnet nicht blockierend, damit eine FIFO keinen Bot anhält. Ein fehlender oder kaputter State führt zu einem klaren Fehler. Vor dem Consumer-Cutover muss der zentrale Writer daher den initialen Stand tatsächlich prüfen.

Der Writer schreibt eine private neue Datei, synchronisiert deren Inhalt und Rechte, ersetzt den State atomar und synchronisiert das Verzeichnis. `llm-model-selection-attempts.json` enthält nur Prüfung/Fehlerentprellung; ein fehlgeschlagener Check kann niemals das `probed_at` des gültigen Modells erneuern.

## Betrieb

Installierbare Unit, Timer und normale TOML-Konfiguration liegen in `ops/model-selection/`. Der bestehende Infisical-Systemcredential wird durch `LoadCredential` wiederverwendet und ausschließlich im RAM über den vorhandenen geschützten Unixtransport eingesetzt. Keine Secrets liegen im Auswahl-State oder in der TOML-Datei. User-Units verwenden keine Mount-/Usernamespace-Sandbox, die den Infisical-Peer als UID65534 erscheinen ließe.

Die bisherigen YAML-/DB-/404- und prozesslokalen Refreshschleifen sind ersetzt. Alle echten tb-llm-Aufrufe lesen den gemeinsamen State; eine 404 startet keine zusätzliche Katalogabfrage. Titelgenerator und Dashboard nutzen ebenfalls diese Auswahl, die frühere GLM-Ausnahme entfällt. Der bestehende zentrale Transport, Fristen, `denken_aus` und das Usage-Ledger bleiben erhalten.
