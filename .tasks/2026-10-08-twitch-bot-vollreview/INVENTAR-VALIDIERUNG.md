# Validierung der Review-Leseabschnitte

Stand: 2026-10-08. Grundlage: `review-slices.json`, Codebasis `0ecae1370f1a80d1a101249b5c932663d69be8af`.

Astra hat die Zuordnung nach Abschluss des Sol-Planers unabhängig rechnerisch geprüft:

- 108 Bereiche, 450 Leseabschnitte und 1745 zugeordnete Primärdateien.
- 1741 nicht leere Textdateien mit zusammen 597180 Zeilen. Je Datei schließen die inklusiven Intervalle lückenlos von Zeile 1 bis zum Dateiende an. Keine doppelten Primärzeilen.
- Zwei leere Dateien verwenden zulässig den Bereich `0/0`: `rust/migrations/.gitkeep` und `ops/highlight-detector/highlight_detector/__init__.py`.
- Zwei Binärreferenzen verwenden ebenfalls `0/0`: `tools/roadmap-history/legacy-history.json.gz` und `website/public/brand/doorman/doorman-tuer.webp`. Diese zählen nicht als textuell geprüfter Quellcode.
- Keine zugewiesenen Pfade mit `ai-coach` oder `.env`-Dateinamen. Dieser Pfadcheck ersetzt keine inhaltliche Secret-Prüfung.

Die Prüfung der Intervalle bestätigt die geplante Abdeckung, nicht die erfolgte Review-Lektüre. Jeder Reviewer muss seine gelesenen Bereiche gesondert nachweisen. Die ersten fünf Defektblicke und die kritisierte Qualitätsbewertung sind bisher nur für R09 abgeschlossen.

Sol-Planer: Workflow `wf_9252ea77-9ce`, Agent `ae14693c3ef7a5b59`, 35 Modellnachrichten ausschließlich `gpt-6.1-sol`. Transcript-SHA256: `e1dc9604e096c0ad5b6df7492936df93ab8ff5954aeba04df3f1cec39de7ede2`.
