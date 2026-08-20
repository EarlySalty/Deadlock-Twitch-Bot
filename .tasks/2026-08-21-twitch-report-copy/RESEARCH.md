# Research: Twitch-Meldegrund in der Audit-DM

status: erledigt
datum: 2026-08-21
klasse: kritisch

## Auftrag

Die private Audit-DM soll pro meldewürdigem Fund Originalwortlaut, absolute ungefähre Uhrzeit, Stream-Zeitfenster und einen kopierfertigen Twitch-Meldegrund enthalten.

## Beobachtungen (belegt, Datei:Zeile)

- `rust/bin/tb-stream-audit/src/main.rs:2552-2608` filtert bereits deterministisch auf Twitch-relevante Funde und sendet danach `report::dm_text`; es gibt keine automatische Twitch-Meldung.
- `rust/crates/tb-stream-audit/src/report.rs:173-260` zeigt bisher Rohzitat, Kategorie und Sekundenoffset, aber kein absolutes Ereignisdatum und keinen fertigen Meldesatz.
- `rust/crates/tb-stream-audit/src/report.rs:20-46` verwendet `Bericht.erstellt_am` als Erstellzeit des Prüfberichts. Das ist nicht die Uhrzeit der Äußerung.
- `rust/bin/tb-stream-audit/src/main.rs:807-817` liest `HelixStream.started_at` bereits aus Twitch und berechnet daraus die Sendungssekunden. `rust/bin/tb-stream-audit/src/main.rs:1221-1241` verwirft den absoluten Startzeitpunkt anschließend.
- `rust/crates/tb-stream-audit/src/plan.rs:32-65, 213-235, 296-325` schreibt Kanal, Lauf und Streamoffset in den Zettel, aber keinen Streambeginn. Der Wiederaufnahmepfad kann deshalb derzeit kein absolutes Ereignisdatum rekonstruieren.
- `rust/bin/tb-stream-audit/src/main.rs:2131-2235` verwendet für `stream_audit` bereits einen OpenAI-kompatiblen Fireworks-Aufruf mit der zentralen Providerauswahl. Die laufende Unit setzt `FIREWORKS_MODEL=accounts/fireworks/models/deepseek-v4-flash-0731` in `ops/systemd/deadlock-twitch-stream-coaching-watch.service:42-55`.
- `rust/bin/tb-stream-audit/src/main.rs:2295-2337` persistiert JSON vor dem DM-Versand und lädt es bei Zustellfehlern erneut. `Fund.zitat_roh` bleibt wegen `skip_serializing` flüchtig; der kopierfertige Satz muss deshalb im Bericht liegen, damit Wiederholungen nicht vom Modellaufruf abhängen.
- `docs/funktionsweise/stream-coaching-audit.md:27-30, 65-67, 77-81` dokumentiert die private DM und den VOD-Offset, ist aber gegenüber dem bereits begonnenen Rohzitat-DM-Stand nicht vollständig aktuell.
- Die Live-Unit lief am 2026-08-21 mit PID 3921822, und ihr Journal zeigte erfolgreiche `stream_audit`-Aufrufe mit `deepseek-v4-flash-0731`; `journalctl -p err` war leer. Das bestätigt den bestehenden Modellpfad, nicht die neue Meldegrund-Aufbereitung.

## Hypothesen (unbelegt, mit Prüfweg)

- `started_at + Fund.start_sekunden` liefert die für Twitch brauchbare absolute Näherung. Ein Unit-Test mit bekanntem RFC3339-Start prüft die Berechnung.
- Ein zusätzlicher Modellschritt für die bereits deterministisch gefilterten Funde ist ausreichend, wenn er nur geschwärzte Belege erhält und sein Ergebnis im JSON gespeichert wird. HTTP-, Parse- und Leerantworten werden mit einem sichtbaren Fallback behandelt.

## Wahrscheinlich zu ändernde Dateien

- `rust/crates/tb-stream-audit/src/plan.rs` und `rust/bin/tb-stream-audit/src/main.rs` für Streambeginn, Aufnahmebeginn und Wiederaufnahme.
- `rust/crates/tb-stream-audit/src/lib.rs` für den persistierten kopierfertigen Meldesatz und rückwärtskompatible Default-Felder.
- `rust/crates/tb-stream-audit/src/llm.rs` und `rust/bin/tb-stream-audit/src/main.rs` für den geschwärzten DeepSeek-Aufruf.
- `rust/crates/tb-stream-audit/src/report.rs` für absolute Zeit, Zeitfenster und die Copy-Paste-Struktur der DM.
- `docs/funktionsweise/stream-coaching-audit.md` für die interne Funktionsbeschreibung.

## Risiken / Seiteneffekte

- Twitch erhält weiterhin keine automatische Meldung. Die Wirkung bleibt auf die private Admin-DM begrenzt.
- Ein alter JSON-Bericht ohne neue Felder muss weiter lesbar bleiben. Bei fehlendem Streambeginn wird die Zeitbasis ausdrücklich als unsicher markiert.
- Rohzitate werden nicht persistiert und nicht an das entfernte LLM gesendet. Das LLM erhält geschwärzte Ausschnitte und Kategorieinformationen.
- Eine LLM-Fehlfunktion darf weder eine falsche Uhrzeit noch einen leeren kopierfertigen Grund erzeugen. Der Fallback wird als nicht aufbereitet markiert.

## Offene Fragen

- Keine für die Implementierung. Das konkrete VOD und Twitch selbst bleiben die manuelle Gegenprüfung, weil Transkriptzeiten nur Näherungen sind.

## Abschlussnachweis

- Die synthetische DeepSeek-V4-Flash-Anfrage lief über den Infisical-Wrapper gegen Fireworks. HTTP 200, erwartetes `reasons`-JSON und kein Rohwortlaut in der Modellantwort wurden geprüft.
- Die Zieltests liefen mit 98 bestandenen Bibliothekstests und 16 bestandenen Binärtests; Clippy mit `-D warnings` war im gesamten Workspace grün.
- Der laufende Dienst wurde nach dem letzten Release-Build neu gestartet. Die neue PID war 2705947, die Executable war nicht als gelöscht markiert, das Fehlerjournal seit dem Neustart blieb leer.
- Die sieben Workspace-Testfehler betreffen `tb-chat`: sechs fehlende `TB_TEST_DATABASE_URL`-Voraussetzungen und ein bereits vorhandener Katalog-Befund. Kein Fehler stammt aus `tb-stream-audit`.
