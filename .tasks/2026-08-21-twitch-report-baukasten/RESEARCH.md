status: erledigt
datum: 2026-08-21

# Research

## Auftrag

Der DM-Text soll pro meldewürdigem Fund ein direkt kopierbares Twitch-Formular
erzeugen: persönlicher Satz, Originalzitat, absoluter Zeitpunkt und ungefähres
VOD-Zeitfenster. Der Text soll deterministisch entstehen, ohne zweiten LLM-Aufruf.

## Befunde

- `rust/crates/tb-stream-audit/src/report.rs:226` baut bereits das DM-Zitat aus
  dem Rohtranskript und fällt bei alten Berichten auf die geschwärzte Fassung zurück.
- `rust/crates/tb-stream-audit/src/report.rs:255` berechnet den absoluten Zeitpunkt
  aus Stream-Start plus Fund-Offset und nutzt sonst den Aufnahmebeginn.
- `rust/crates/tb-stream-audit/src/report.rs:264` baut das ungefähre
  Stream-Zeitfenster und kennzeichnet unbekannten Stream-Start ehrlich.
- `rust/bin/tb-stream-audit/src/main.rs:2003` ruft derzeit einen zweiten
  Meldegrund-LLM-Schritt auf. Dieser schreibt `Fund.twitch_meldegrund` und ist für
  die DM nicht erforderlich, weil der Bericht bereits Zitat und Zeitdaten besitzt.
- `rust/crates/tb-stream-audit/src/llm.rs:60` enthält den ausschließlich für diesen
  zweiten Schritt verwendeten Prompt und die Antworttypen.
- Die DM wird in `rust/bin/tb-stream-audit/src/main.rs:2715` auf echte
  Twitch-meldewürdige Funde begrenzt und bleibt ein menschlich ausgelöster
  Copy-Paste-Schritt.

## Entscheidung

Der zweite LLM-Aufruf entfällt. Der Bot setzt den Satz deterministisch aus dem
Rohzitat, dem absoluten Zeitpunkt und dem bestehenden Zeitfenster zusammen.
Historische JSON-Felder werden beim Lesen weiter toleriert; neue Berichte tragen
keine LLM-Meldegrunddaten mehr.
