# Nachweis W02

Codebasis: `0ecae1370f1a80d1a101249b5c932663d69be8af`. Reviewworkflow: `wf_cdc4c5ac-9bb`. Zwei Bereiche, DA01 und DA02, mit 14 Abschnitten und fünf unabhängigen Defektblickwinkeln.

## Ergebnis und Abdeckung

70 aktuelle Reviewer lieferten jeweils complete, ohne gemeldete ungelesene Bereiche oder Blocker. Die deklarierten Leseintervalle decken die zugewiesenen 16372 Primärzeilen in jedem der fünf Blickwinkel ab: 81860 Zeilen-/Blickwinkelpaare. Keine uneindeutig zugeordneten Intervalle und keine rechnerische Lücke.

Der Nachweis prüft die Vereinigung deklarierter Intervalle gegen das Manifest. Er beweist weder die tatsächliche vollständige Werkzeuglektüre noch Fehlerfreiheit oder Vollständigkeit der Defektsuche. 455 Referenz-Leseintervalle außerhalb der Primärzuordnung wurden nicht als Primärabdeckung gezählt. Manche Referenzbelege betreffen externe Proxy-Konfiguration; deren Bindung an die feste Codebasis wurde bei dieser Metadatenprüfung nicht unabhängig bestätigt.

Das Journal enthält frühere Fehlversuche. Ausgewählt wurden die 70 nach `2026-10-08T02:02:26Z` gestarteten `general-purpose`-Reviewer, je Abschnitt und Linse genau ein finales Resultat. 52 frühere Starts und 24 frühere Resultate blieben ausgeschlossen. Die letzte StructuredOutput-Eingabe stimmt laut Konsolidierer jeweils mit dem endgültigen Journalresultat überein.

## Modellprüfung

Der Konsolidierer überprüfte 70 Transcripts. Die Hauptsession wiederholte die SHA256-, Modellfeld- und Nachrichtenzählung unabhängig: 70 passende Hashes, 2694 echte Datensätze mit `message.model=gpt-6.1-sol`, keine Abweichung. Diese Datensätze enthalten 1810 verschiedene Message-IDs; die Datensatzzahl ist nicht die Zahl unabhängiger API-Aufrufe. In den ausgewählten Transcripts gibt es keine synthetischen API-Fehlermeldungen.

Die 70 Einzelbelege stehen strukturiert in `W02-KANDIDATEN.json` unter `model_proofs.reviews`: Abschnitt, Blickwinkel, Agent-ID, Transcript-SHA256, Datensatzzahl, verschiedene Message-IDs sowie Journal-/Transcript-Ergebniszeile. Vollständige native Transcripts verbleiben außerhalb des Repositorys.

Konsolidierer: `a1e800f1adc3bca82`, Workflow `wf_83c5b894-715`, 57 echte Sol-Datensätze, von Astra am fertigen Transcript geprüft. Transcript-SHA256:
`d784b90ecac5fe8f124d1a1fcc677de59635362fbff93f923c7bd0ea24237a5f`.

Kandidatenartefakt bei Abnahme, 94682 Bytes, SHA256:
`07b829884269be94562d3cca780db4a18f0a235b3cdec0b4ef90b64a2fa72ce6`.

Der Konsolidierer durfte aufgrund seiner Rollenregeln keine Markdown-Berichtdatei anlegen. Seine vollständigen Nachweise liegen in der JSON-Datei. Diese Zusammenfassung schreibt Astra; dafür war kein zweiter Agent erforderlich.

## Kandidaten und Gegenprüfung

| Stand | A-Vorschläge | B-Vorschläge | C-Vorschläge | Gesamt |
|---|---:|---:|---:|---:|
| Rohmeldungen | 17 | 28 | 14 | 59 |
| Kanonische Gruppen | 8 | 18 | 14 | 40 |
| Bereits doppelt bestätigte Gruppen | 2 | 2 | 0 | 4 |
| Noch unabhängig zu prüfen | 6 | 16 | 0 | 22 |

Astra prüfte die Herkunftszuordnung rechnerisch: 59 eindeutige Roh-IDs, jede genau einmal enthalten. 19 doppelte Meldungen wurden zusammengeführt. Gleiche Dateien allein begründen keine Zusammenführung; unklare oder unterschiedliche Ursachen bleiben getrennt.

Die vier vorhandenen Bestätigungen gelten für die kanonischen Claims von A01a, A01b, B02 und B03. Zusätzliche Szenarien aus Duplikaten, andere Sitzungstypen und eigenständige Ablauf-/Ablehnungsregeln erhalten dadurch keine Fixfreigabe.

Die 22 übrigen A/B-Kandidaten sind in `wf_f9b737d9-fe8`, Task `wqip1tlk0`, mit je zwei frischen Sol-Skeptikern beauftragt. Deren Prompts enthalten ausschließlich Behauptung und Ort, keine ursprüngliche Klassifikation, Begründung oder fremden Urteile. Die 14 C-Gruppen bleiben dokumentiert. Vollständige Szenarien, Sollbelege, Gegenmaßnahmen und Herkunft stehen in W02-KANDIDATEN.json.

Ein bestätigter Befund ist noch kein geprüfter Fix. Kein Anwendungscode-Merge, Deploy oder Live-Nachweis wird aus diesem Reviewabschluss abgeleitet.
