# W03: fünf weitere Bereichskonsolidierungen geprüft

Stand: 2026-10-08. DA07, DA15, DA17, MO01 und IA01 besitzen abgeschlossene Konsolidierungsartefakte mit unabhängig geprüfter Herkunft. Zusammen mit den sieben früheren Bereichen sind damit zwölf Bereichskonsolidierungen technisch abgenommen. Globale semantische Quervergleiche und die Gegenprüfung der neuen Behauptungen sind damit nicht abgeschlossen.

## Herkunft und vollständige Zuordnung

Astra prüfte 105 fertige Reviewertranscripts mit 5001 echten Modellfeldern gpt-6.1-sol. Keine synthetischen Datensätze in diesen 105 Transcripts. Die fertigen Transcript-Hashes stimmen mit den eingebetteten Nachweisen überein; letzte StructuredOutput-Rückgaben entsprechen den Journalresultaten. Die fünf Konsolidierer wurden ebenso auf Modelle, Hashes und originale Rückgaben geprüft.

166 eingebettete Rohbefunde entsprechen exakt ihren Originalobjekten. Jede Roh-ID gehört genau einmal zu einer neuen Gruppe oder einer bestehenden Verknüpfung. Erwartete Abschnitt-/Linsenlabels sind vollständig vorhanden. Die deklarierten Primärintervalle decken die Manifestintervalle ab; keine unparsebaren Angaben oder gemeldeten Primärlücken. Das beweist weder tatsächliche vollständige Werkzeuglektüre noch Fehlerfreiheit.

IA01 meldet eine fehlende Kontextreferenz bot/internal_api/app.py am festen Review-SHA 0ecae1370f1a80d1a101249b5c932663d69be8af. Diese Referenz gehört nicht zum Primärumfang. Verhalten aus einer nicht vorhandenen Legacy-Datei wurde nicht unterstellt.

| Bereich | Reviews | Echte Sol-Felder | Originalbefunde | Neue Gruppen | Bestehende Verknüpfungen | Reine A/B-Vorschläge | Konservativ C |
|---|---:|---:|---:|---:|---:|---:|---:|
| DA07 | 20 | 1060 | 37 | 29 | 2 | 20 | 9 |
| DA15 | 15 | 693 | 11 | 10 | 0 | 5 | 5 |
| DA17 | 20 | 1024 | 47 | 37 | 3 | 30 | 7 |
| MO01 | 30 | 1265 | 36 | 25 | 0 | 18 | 7 |
| IA01 | 20 | 959 | 35 | 25 | 0 | 17 | 8 |
| Summe | 105 | 5001 | 166 | 126 | 5 | 90 | 36 |

Die 90 reinen A/B-Vorschläge gelten vor dem weiteren Quervergleich und sind keine bestätigten Fehler. Die bisherigen bestätigten Zahlen 13 A, 25 B und 30 C für R09/W02/DA03 ändern sich nicht.

## Konservative Freigabeentscheidung

Vier Gruppen enthalten sowohl B- als auch C-Vorschläge und bleiben C. Originalklassifikationen werden nicht überschrieben:

- W03-DA15-S001-concurrency-1
- W03-DA17-S001-correctness-4
- W03-MO01-S001-concurrency-2
- W03-IA01-S002-errors-1

Die gesonderte Astra-Entscheidung reduziert die von einzelnen Konsolidierern gemeldete A/B-Zahl. Kein Fix ohne zwei unabhängige BESTÄTIGT und bei B zusätzlich eindeutig belegtes Sollverhalten.

## Dauerhafte Nachweise

Die unveränderte technische Zusammenfassung liegt in W03-REST-VERIFIKATION.json, SHA256 ba4d94919a4f57273592a402aa80f05c07d72092d4e45f8e12df07ab5e9f6468. Sie stammt aus /tmp/tb-w03-restkonsolidierung-astra-verification.json. Vor Übernahme wurden die fünf Artefakthashes erneut gegen den Bericht geprüft.

| Artefakt | SHA256 |
|---|---|
| W03-DA07-KANDIDATEN.json | 7fb0a4b3a289725429298e3809aa352b5753b925e3726a73e779137a74ced65b |
| W03-DA15-KANDIDATEN.json | 85c8621304dae39e06bb6d35f8e0809b6bebbba9ad7cfd8ffb2dbbbfdb62a134 |
| W03-DA17-KANDIDATEN.json | 504bbe737dfb90790a5e1040bde13588aa6ad75654bc418a44d0d7bf0705d7d9 |
| W03-MO01-KANDIDATEN.json | b3fe6388ad7f878fce80227b301ea3739de694467553f2b6d6781bf25684ad33 |
| W03-IA01-KANDIDATEN.json | 583973f5b66adee48e47d98d59e0ed0be0b928a4fb67a2350f83367aac53c585 |

Quervergleich wf_3c0cb7ee-5b4, Task wzydtulh7, ist beendet und abgenommen. Rolle a8c9640c27fd10cb0: 58 echte Sol-Datensätze, Transcript-SHA256 65bb959665d2ea8f28da74cc16c0366cb0d03a6f1c28bec11638323aa345eda5. Originalrückgabe in ABSCHLUESSE-11.json. Keine weitere vollständige Gleichwertigkeit nachgewiesen; 90 Behauptungen bleiben getrennt, sechs partielle oder ungeklärte Überlappungen sind ausdrücklich dokumentiert. Keine globale Einzigartigkeit oder neue fachliche Bestätigung daraus ableiten.

W03-REST-CLAIMS.json hat SHA256 3c338827a8d28b9e0091a4f3ba7ef70dcc4d8d64b9728319af9258329d4c4357. Astra prüfte selbst zwölf Quellhashes, 126 Originalgruppen, fünf bestehende Verknüpfungen, 166 einmalig erhaltene Mitglieder und exakte neutrale Feldwerte. Vier gemischte Gruppen, 36 neue C-Gruppen sowie die referenzierten bestehenden C-Sperren sind von der neutralen Liste ausgeschlossen. Der erste Metadatenprüfaufruf brach wegen eines nicht im Rohobjekt gespeicherten id-Feldes ab; der korrigierte Aufruf bindet die ID an den unveränderten Dictionary-Schlüssel und bestand vollständig. Vor dem Abbruch war keine Datei geschrieben worden.

W03-REST-NEUTRALE-CLAIMS.json enthält 90 Objekte mit exakt id, claim, file, line, ohne Klassen oder Begründungen. SHA256 0f3c3298add523c126fe5ed55c1d239d20d3668caf841b3a8dac5bb6ab8f8420. Neue Gegenprüfung gestartet: wf_16dfb9cb-031, Task wc1nrzzig, Script tb-vollreview-w03-rest-gegenpruefung-wf_16dfb9cb-031.js. Zwei frische unabhängige Skeptiker je Behauptung. Jeder lädt anhand des nullbasierten Index genau sein neutrales Objekt und prüft vorher den Dateihash. Args in WORKFLOW-ARGS.json. Keine neuen bestätigten Zahlen, solange Urteile und Modellbeweise nicht abgenommen sind. Bestehende DA04-/DA05-/DA06-Gegenprüfung nicht duplizieren.
