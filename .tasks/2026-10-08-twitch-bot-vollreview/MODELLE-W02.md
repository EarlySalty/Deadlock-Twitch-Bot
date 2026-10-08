# Modellnachweise W02

Session: `f61905e7-f7ff-405b-a6d7-090dec371fcb`. Transcript-Basis wie in MODELLE.md. Diese Liste enthält vorerst nur die abgeschlossenen Reviewer, deren erste Kandidaten bereits weitergegeben oder erfasst wurden.

| Rolle | Workflow | Agent | Modell | Nachrichten | Transcript-SHA256 |
|---|---|---|---|---:|---|
| DA01-S004 Nebenläufigkeit | wf_cdc4c5ac-9bb | af1906e0e42b5f7a4 | gpt-6.1-sol | 42 | 71d7637553660d13912ab3100e3a4979b62a4f23cc23796d6288b70e3c64d36e |
| DA01-S003 Ressourcen | wf_cdc4c5ac-9bb | a38ff8d6152375fd2 | gpt-6.1-sol | 49 | c4cc0825e31bad7977fab6dbd30b09f2f6fd2c2d47714cd0d3214ebd8c6d874c |
| DA01-S003 Korrektheit | wf_cdc4c5ac-9bb | a5d8a2b501d115ffc | gpt-6.1-sol | 61 | 4ee266e1a9504da20f5e2516fec933d6768d9acebfd9c2823a6dc3d5c134fba1 |
| OBS-Kandidat, Skeptiker 1 | wf_3b16d4aa-68e | a2cc748228287326d | gpt-6.1-sol | 45 | 6646a49fb2ccd981edebda298448a4eeef0730460ba07b3073f10bd756375976 |
| OBS-Kandidat, Skeptiker 2 | wf_3b16d4aa-68e | aaf00c9fc2e9ce972 | gpt-6.1-sol | 32 | 065482fc7583cbb081ddab4e05a58add0ca600c680d3ba3dc6b0ced46a1c3ffc |
| Audit-Kandidat, Skeptiker 1 | wf_3b16d4aa-68e | a21bacec28c4e4452 | gpt-6.1-sol | 31 | 48cf694987bc9d8d0920ab48efe05f2a91e21cde4723c3d2ce613400823ee801 |
| Audit-Kandidat, Skeptiker 2 | wf_3b16d4aa-68e | ac5c34d004bd0f275 | gpt-6.1-sol | 37 | ee85e0effd88b0d645c8b38267d4bbf4e2da81df58ba20cc07a7cd53971d188a |
| DA02-S001 Nebenläufigkeit | wf_cdc4c5ac-9bb | acbdd6c84fd00b590 | gpt-6.1-sol | 51 | a594c1b20b7cd17824976ebd8d878b4d1b6fb065c703220faaea69170aede86e |
| DA02-S002 Fehlerbehandlung | wf_cdc4c5ac-9bb | aee0aa071af5d50b8 | gpt-6.1-sol | 51 | 7f45e9e7db7a99d0d532269526bfad94b925da9176da85c8395afde770018503 |
| DA01 Bauqualität | wf_67858741-57f | a817daeec624cc91f | gpt-6.1-sol | 84 | 101c798b031a7e34464df23bc8195ab3823a3ae0280a4f379345f5ce85623ee1 |
| DA02 Bauqualität | wf_67858741-57f | a49d3c978a85ec7f3 | gpt-6.1-sol | 66 | 7836dc6dc7ef1412dccc7ca8c2433d981cc201293e524bdd12e0d9eb5fe868c8 |
| DA01 Qualitätskritik | wf_67858741-57f | a8a7d2ae5acb95b6b | gpt-6.1-sol | 39 | 190ff5ada14b9cbf0ea54e448688c87e55b5b773f40e59bbfd777f9f627d0abc |
| DA02 Qualitätskritik | wf_67858741-57f | a69cce616175f3ffb | gpt-6.1-sol | 45 | a25adf968f23cf70058fbbb8721feb25cb02a9ab7ccbe7a66fc3ceac9c17a364 |
| A01a Skeptiker 1 | wf_9eb7b672-f97 | a56f4e255006e7291 | gpt-6.1-sol | 43 | 800ecaa7f09b6f0b7b03ecef47483d51d3dbf60df1de4ad597a81b503bddc67b |
| A01a Skeptiker 2 | wf_9eb7b672-f97 | ac61a921899bce902 | gpt-6.1-sol | 35 | 4a99090ed9a1789d9f431fc2d043cc91fec6b56553bfc1b7fe9fdb8c35eefadd |
| A01b Skeptiker 1 | wf_9eb7b672-f97 | a636373646c18fc85 | gpt-6.1-sol | 45 | 6bae0e5abd1a7217e518e2abea16e128dc3b8d2efe03eec5c76303b53be1a758 |
| A01b Skeptiker 2 | wf_9eb7b672-f97 | a2d35f684e4064cfc | gpt-6.1-sol | 41 | 55a771e4d05156b16b61be970267f42caaf61fb0a28ebb829787ce79577283d9 |

Die abgeschlossenen Transcripts wurden vollständig nach `message.model` ausgewertet. Andere echte Modelle wurden darin nicht gefunden. Je eine synthetische API-Fehlermeldung der beiden Qualitätsreviewer wurde gesondert erkannt und nicht als Modellaufruf gezählt. B02/B03 erhielten in `wf_3b16d4aa-68e` jeweils zwei BESTÄTIGT-Urteile mit eindeutigem Soll. Beide A01-Teilbefunde erhielten in `wf_9eb7b672-f97` ebenfalls je zwei BESTÄTIGT-Urteile, Klasse A und Soll belegt. Die drei Fixpakete wurden beauftragt; ihre Implementierung und Kritik haben eigene Nachweise.

W02 ist abgeschlossen: 70 erfolgreiche aktuelle Reviewer, 2694 echte Sol-Modellnachrichten. Die konsolidierte Liste mit 70 Transcript-Hashes steht in W02-KANDIDATEN.json unter model_proofs.reviews; W02-NACHWEIS.md beschreibt die Abdeckung und Grenzen. Astra wiederholte die Hash- und Modellfeldprüfung ohne Abweichung. Die obige Tabelle ist eine ergänzende Auswahl einschließlich Skeptikern und Qualitätsrollen, keine Hochrechnung auf 70 Agents.

Konsolidierer `a1e800f1adc3bca82`, Workflow `wf_83c5b894-715`: 57 Sol-Datensätze, SHA256 `d784b90ecac5fe8f124d1a1fcc677de59635362fbff93f923c7bd0ea24237a5f`, vollständig durch Astra geprüft.

## Weitere 44 Skeptiker

`wf_f9b737d9-fe8` ist abgeschlossen. 22 kanonische Claims, je zwei frische Sol-Skeptiker, 1703 echte Modell-Datensätze und 1025 verschiedene Message-IDs innerhalb der Transcripts. Keine synthetischen oder fremden Modelldatensätze. Astra prüfte sämtliche 44 Transcript-Hashes, Modellfelder und die exakte Übereinstimmung des letzten StructuredOutput mit dem jeweiligen Journalresultat erneut; keine Abweichung.

Die vollständigen Einzelbelege stehen in W02-GEGENPRUEFUNG-03.json unter model_proofs.transcripts. results enthält 22 echte Paare und die 44 unveränderten Urteile samt Herkunft. Artefakt-SHA256 bei Abnahme: `3fff60d3d22e7af33e929c2f5aedb3a11d1ffc44daf327d981a0a7b2006bee7f`. Astra bestätigte zusätzlich 22 eindeutige IDs und exakte JSON-Gleichheit aller eingebetteten Urteile zum Originaljournal.

Der erste Exportagent `ac6f7abe268912f3f` in wf_54f7b776-e4b brach im Kontextlimit ab; sein Teilstand mit zehn Platzhaltern wurde nicht abgenommen. Frischer Metadatenagent `af3a6b9cdc02d658a` in wf_670191de-48b ergänzte diese zehn Paare und validierte den Datensatz. 34 echte Sol-Datensätze, SHA256 `2210024ef83808e3c43962e29d7474c6f2f9319fe5067abb88fcee86520be16e`, durch Astra geprüft. Keine erneute Verhaltensprüfung oder Änderung eines Urteils.

Sieben zusätzliche A-Paare, elf B-Paare mit belegtem Soll, vier C-Sperren. Die begrenzte Prüfung expliziter Werkzeugeingaben fand keinen benannten Zugriff auf fremde Befunddateien, beweist aber keine vollständige Unabhängigkeit von indirekten Inhalten. Diese Grenze ist im JSON erhalten. Modelle/Hashes sind Transcript-Belege, keine darüber hinausgehende Anbieterattestierung.

## Abgrenzung der Wiederaufnahmen

Der Workflow wurde mehrfach fortgesetzt. Sein Journal enthält deshalb auch frühere, blockierte Rollenversuche. Seit der Scriptänderung `2026-10-08T02:02:26Z` startet jeder Reviewer als `general-purpose`. Die Kontrollauswertung um 02:19 UTC fand 36 neue Sol-Agenten, darunter 22 vollständige Rückgaben. In diesen neuen Transcripts wurde kein Cargo-Prüfaufruf gefunden. Alte `rust-reviewer`-Fehlversuche zählen weder als Abdeckung noch als negatives Review-Ergebnis.

Bei der Endauswertung je Kombination aus Abschnitt und Blickwinkel den erfolgreichen aktuellen Rollenversuch verwenden. Mehrere Versuche nicht addieren. Ein fehlendes oder unvollständiges Resultat bleibt eine Lücke.
