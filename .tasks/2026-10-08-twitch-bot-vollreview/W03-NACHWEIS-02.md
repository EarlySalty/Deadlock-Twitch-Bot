# W03: abgenommene Nachweise für DA03 bis DA06

Stand: 2026-10-08. Feste Codebasis 0ecae1370f1a80d1a101249b5c932663d69be8af. Dieser Nachweis bestätigt Herkunft und abgeschlossene Gegenprüfungen, keine vollständige Fehlerfreiheit, Fixes oder Livewirkung.

## DA03: 19 Gegenprüfungen abgeschlossen

Der erste Workflow wf_97f7401d-6c8 lieferte 34 Urteile, vier Rollen scheiterten mit API 403. Der Ergänzungslauf wf_e359d631-ed4 lieferte genau diese vier fehlenden Stimmen. Vorhandene Urteile wurden nicht ersetzt. Astra prüfte die 38 fertigen Transcripts: 1543 echte gpt-6.1-sol-Datensätze, ein synthetischer Datensatz gesondert, keine fremden echten Modelle. Jede letzte StructuredOutput-Rückgabe stimmt mit ihrem Journalresultat überein.

Vier Paare bestätigen A, elf B mit jeweils belegtem Soll; vier bleiben C/gesperrt. Zusammen mit sieben ursprünglichen C-Gruppen ergibt DA03 unter seinen 26 neuen Gruppen 4 A, 11 B und 11 C. Sechs W02-Verknüpfungen zählen nicht erneut. R09, W02 und DA03 ergeben damit 13 A, 25 B und 30 C. Diese Zahlen bezeichnen Befunde und dokumentierte Gruppen, keine erledigten Fixes.

Die Sperren betreffen Discord-Completion mit state_id allein, next-Steuerzeichen, den HTTP-Cache über Cookiewechsel und Partner-Tokenlogin-CSRF. Fehlende Voraussetzungen beziehungsweise abweichende Urteile bleiben erhalten. Kein WIDERLEGT-Urteil in dieser DA03-Gruppe. Ein späteres günstiges Ersatzurteil ist nicht beauftragt.

Verlustfreier Export: W03-DA03-GEGENPRUEFUNG.json, SHA256 658fea9e12a905451ce2162807b3cdd2e0c34d19734758ac5a09146a948fe32e. Astra prüfte 19 echte Paare, 38 verschiedene Agenten und die exakte Gleichheit jedes exportierten Originalurteils mit dem jeweiligen Journal. Einzelhashes und Modelle stehen unter model_proofs.runs. Vier gescheiterte Versuche mit 54 echten Sol-Datensätzen und vier synthetischen Fehlern stehen separat und zählen nicht als Stimmen.

Exporter wf_fb383924-e58, Agent ab3d17ddfd6108d0c: 38 Sol-Datensätze, SHA256 58d88b95f272d41df4b5866692a05fc0bf78d1375e09e558951f0eccfa33fada. Modell und Export durch Astra geprüft. Die begrenzte Prüfung von 885 ausdrücklichen Toolinputs fand keinen benannten Zugriff auf fremde Urteile. Indirekte Einflüsse sind damit nicht ausgeschlossen. Modell-Datensätze sind keine API-Aufrufzählung.

## DA04, DA05 und DA06: Konsolidierungen abgenommen

Workflow wf_af2d23c2-851 ist beendet. Astra wiederholte die Modell-, Hash-, Journalpositions- und letzte StructuredOutput-Prüfung für sämtliche 50 Reviewertranscripts. Die 118 Originalbefunde wurden exakt mit den ursprünglichen Journalobjekten verglichen. Jede Roh-ID ist genau einer Gruppe zugeordnet. Keine Abweichung festgestellt.

| Bereich | Reviews | Echte Sol-Datensätze | Synthetische Datensätze | Rohmeldungen | Neue Gruppen | W02-Verknüpfungen | Neue A/B-Vorschläge |
|---|---:|---:|---:|---:|---:|---:|---:|
| DA04 | 25 | 1380 | 0 | 52 | 42 | 2 | 29 |
| DA05 | 10 | 612 | 1 | 24 | 21 | 0 | 12 |
| DA06 | 15 | 837 | 1 | 42 | 33 | 2 | 26 |

In diesen drei Bereichen stehen 67 A/B-Vorschläge zur Gegenprüfung und 29 ursprüngliche C-Vorschläge. Noch keine A/B-Bestätigungen daraus. Die dokumentierten Primärintervalle umfassen 38 Dateien und 19481 Zeilen, entsprechend 97405 Zeilen-/Linsenpaaren. Deklarierte Intervalle beweisen weder tatsächliche vollständige Werkzeuglektüre noch Fehlerfreiheit. Der einzige nichtleere DA06-Blockereintrag betrifft ungeprüfte produktive Zeitgrenzen und behauptet ausdrücklich keine fehlende Primärlektüre.

| Artefakt | SHA256 |
|---|---|
| W03-DA04-KANDIDATEN.json | 02e9c9603941985d02c5994a5da044aa4b3cebc549ed28e44ac5d7c6498c6f9c |
| W03-DA05-KANDIDATEN.json | d928d0ad85b8b055a3cbffc5cd4657b71cc35f842e882d4748cf673cdd2244ac |
| W03-DA06-KANDIDATEN.json | b2e6065b7c22af8c389b298071a1b5642e272d8ebb38231a296ee89b11386fa1 |

Konsolidierer, jeweils durch Astra auf Sol und Transcript-Hash geprüft:

- DA04 af807a5678db4c407: 55 Datensätze, SHA256 4a489f36b579a9f12e1502e2c0d2f6c33fe7b3770b999a595d60b6589d3da168.
- DA05 a386abae0f809f5f5: 22 Datensätze, SHA256 1ca600233c9bedf9edc4b0de40eb751a3f79a9b1b3233480493539a151784d1d.
- DA06 aee24d6b28d1ec9ee: 53 Datensätze, SHA256 d526abc9e47d5d24acf9deaed689ccbef755726a44ec7a0f81ffd3aa6ced2638.

Astra verglich die 67 neuen A/B-Claims und die angegebenen Überschneidungskandidaten zwischen diesen drei Bereichen. Identisches Thema oder gemeinsamer Helper allein begründen keine Zusammenlegung verschiedener erreichbarer Verträge. Bestehende W02-/DA03-Verknüpfungen und C-Sperren bleiben bestehen; spätere Fachbereiche können weitere Duplikate liefern. Gegenprüfung in wf_655679bf-2b7: neutrale Claimliste ohne Begründungen, dann je zwei frische unabhängige Sol-Skeptiker. Keine Fixfreigabe aus der Konsolidierung.

## Fortsetzung

Sieben Bereiche sind konsolidiert: R09, DA01, DA02, DA03, DA04, DA05, DA06. Übrige W03-Bereiche laufen beziehungsweise warten auf vollständige Konsolidierung. W04 startet weitere 13 Bereiche mit 46 Abschnitten und 230 geplanten Reviews. Geplante oder laufende Abdeckung zählt nicht als abgeschlossen. Qualität/Kritik ist ein getrennter Prüfweg.
