# Weitere Fixabnahmen und nachgewiesene Prüfgrenzen

Stand: 2026-10-08. B02 bleibt das einzige nachgewiesen integrierte Anwendungscodepaket. Kein Deploy, Neustart oder Live-Nachweis. Remote-Abfrage bestätigte main e98b7f016dbab373a5a8dd9490d158b136c97fec und Artefaktcheckpoint b47669f408ba5f00e7d9eec9703bc006ef05aa5d.

## Originale und Modellbelege

ABSCHLUESSE-10.json enthält zehn finale Rückgaben aus wf_e099cf2a-fc9, wf_f1a93c64-6ee, wf_961bca08-8d7 und wf_b1cabe6f-009. Astra prüfte die echten message.model-Felder, fertige Transcript-Hashes und die exakte Gleichheit der jeweils letzten StructuredOutput-Rückgabe mit dem Journalresultat. 824 echte Sol-Datensätze, kein synthetischer Datensatz. Das sind Modelldatensätze, keine gezählten API-Aufrufe.

Artefakt-SHA256: d61fe798fd465c8430dec5a83a4659586e3cf3ace5351f01453991adf13126c1. Einzelne Transcriptpfade, Hashes und unveränderte Originalurteile stehen im JSON; keine Rohtranscripts wurden übernommen.

| Rolle | Agent | Sol-Datensätze | Ergebnis |
|---|---|---:|---|
| B01 Prüfbindung | a15610373434996f3 | 79 | Konkrete Bindungslücke geschlossen, keine Quelländerung |
| Statusereignis 7 | a0cc0b43c7d37e191 | 17 | Ereignis übernommen, keine Schemakonflikte gemeldet |
| B07 Prüfabschluss | a5b9b153cdc8c3794 | 82 | Fokusprüfungen gebunden, keine vollständige Crate-Suite |
| B06 Prüfabschluss | a81e0946686b17121 | 152 | Vollständige passende Suite und Clippy belegt |
| B07 Kritik | a9c07a6300a45762a | 72 | ALLOW, verbleibender getrennter TikTok-Testfehler |
| B04 Prüfabschluss | a44d7746bcf0182bc | 142 | Tatsächlicher nichtleerer Patch, passende Prüfungen |
| B06 Kritik | a63540e27e22e47a2 | 44 | ALLOW |
| B04 Kritik | a8f33093b29ca2add | 75 | ALLOW |
| B10 Fixer | aa2dbe7b4f28508b4 | 122 | Zwei Authstatusfehler behoben, Prüfumfang eingeschränkt |
| B10 Kritik | a7a778931a5022056 | 39 | ALLOW |

## B01: historische Quelle statt Logname

B01-PRUEFBINDUNG.md und neun dort genannte, durch Astra erneut gehashte Logs belegen die neue Regression am unveränderten Head c3aa3cc94fec60aa365dcc9ca9f54b4bd3f1539a sowie die historische vollständige Suite. Tatsächlicher historischer Fixhead war 307c09e64528699364036780eb9f6623192be1d8, nicht 3341098f aus dem Lognamen. Damaliger und jetziger Fix haben denselben Rust-Baum 67c189f417131aacbb77fa899d861e91bf5e8ce8. Die Baselines haben Rust-Baum da48b906659042b04e9166f184868bd85e72bb2e. Die damalige Werkzeugfolge verbindet die konkreten Quellen mit abgeschlossenen Tests.

Neue Regression: ein Test bestanden, 359 gefiltert. Historische Suite: 336 bestanden/24 rot gegen 335/24 mit identischen Fehlernamen. 34 Paketformatabweichungen unverändert. Clippy weiterhin im unveränderten tb-chat blockiert; leerer jüngster Baseline-Clippylog und eine andere historische Rust-Basis belegen keine vollständige passende Clippy-Baseline. Diese Grenze bleibt in der Integrationsvorbereitung ausdrücklich offen. Keine neue fachliche Kritik erforderlich, solange der bereits abgenommene Patch exakt erhalten bleibt.

## B04 und B06: echte Kritiken statt fehlender oder leerer Vergleiche

B04 d760300d8100b8bf283fee2888c758e6934f3bad: echter Kritiker ALLOW, ursprünglicher Patchhash c6bec73321afa74d3235e84068b38042b82e1a4cd23dc83a7347933f0bd5c4c4. Vollständige Suite 1336 bestanden/35 rot gegen 1333/35; neun Routerfälle bestanden. Clippy vor und nach Fix Exit 0, gleiche Warnungen. Paketformat hat 265 gleiche Bestandsabweichungen. Positive Linkregression endet nach passierter Adminprüfung bei auth_unavailable; sie belegt keine echte Linkausstellung. Kündigung wird mit unangemeldeten Requests geprüft.

B06 46c52a928b47444b10364d024a632ca00226d6c4: echter Kritiker ALLOW, ursprünglicher Patchhash 43b42f2f4d7a9a520c5629db352c333e6cab1f2307b53ccd1f5548b648e51fa6. Neue passende vollständige Suite 1336 bestanden/35 rot gegen 1333/35, 14 Proxytests bestanden. Clippy beidseitig Exit 0. Eigene zehn alte Formatblöcke entfallen, 255 fremde bleiben gleich. Der erste vollständige Testlauf endete durch einen zwischenzeitlich geänderten vorhandenen Wrapper mit Exit 70; der reguläre Folgeversuch lieferte Exit 101. Kein Wrapperumbau durch diesen Auftrag.

Astra prüfte reale Testzusammenfassungen und Loghashes beider Pakete. Originale, genaue Kommandos und Hashes stehen in ABSCHLUESSE-10.json beziehungsweise BRIEFING-INTEGRATIONSVORBEREITUNG-03.md. Die historischen Abgaben beziehen sich auf a8b5b5e9, nicht das seit B02 fortgeschrittene main.

## B07 und B10: fachliches ALLOW mit begrenzten Testläufen

B07 9ec607b31a68d2721f6c8905d4268fd5d4290ca6: echte Kritik ALLOW. Das Integrationstestziel plan_stufen_gates ergibt 11 bestanden/1 rot statt 3/9. Acht zuvor rote Identitätsfälle bestehen; drei vorher grüne bleiben grün. Der verbliebene Test erreicht jetzt einen anderen vorhandenen Guard, weil seinen Eingaben tiktok_options fehlen. Kein zusätzlicher bestätigter Befund und kein Nebenfixauftrag daraus. Clippy beidseitig Exit 0, Paketformat unverändert rot. Eine vollständige Crate-Suite wurde bisher nicht ausgeführt.

B10 9bce62f1daf52006bb543edc6192769f5971b364: Fixer und echter Kritiker ALLOW. Der gemeinsame Cookieparser erhält die erste passende Auswahl über mehrere Header; Zeitmessung erfolgt nach Übernahme des Cachelocks. 13 Fokustests bestanden. Astra las die tatsächlichen Testkommandos und Logs: Ohne --no-fail-fast brach der Paketaufruf nach dem Bibliotheksziel ab. 1321 bestanden/21 rot/3 ignoriert gegen 1317/21/3 ist deshalb ausdrücklich keine vollständige Crate-Suite. Integrationstests und Doctests fehlen in diesen Protokollen. Clippy scheitert mit -D warnings an derselben fremden Diagnose. HTTP-Cachepolitik bleibt C-gesperrt; rückwärts springende Systemzeit liegt außerhalb des behobenen Überholungsfehlers.

## Status und Folgeauftrag

Der TODO-Diff von Statusereignis 7 erhält die ältere Historie und bildet den damaligen Stand ab. Spätere Abschlüsse und W06/W07 gehören in ein neues Ereignis, nicht in das historische Ereignis 7. B02 wurde mechanisch durch Astra integriert; der frühere Integrationsworker selbst hatte keinen Push ausgeführt.

Neue Integrationsvorbereitung ohne Codekorrektur: wf_f7342e70-084, Task wq0ls3kxm, Script tb-vollreview-integrationsvorbereitung-03-wf_f7342e70-084.js. Args [B01, B04, B06, B07, B10] in WORKFLOW-ARGS.json. BRIEFING-INTEGRATIONSVORBEREITUNG-03.md verlangt frische Basis, unveränderten Patch, gebundene vollständige Crate-Prüfungen einschließlich der konkreten B07-/B10-Lücken und passenden Sol-Gate. Je Rolle exklusiv eine eigene Nachweisdatei und der eigene Worktree. Kein Main-Push oder produktiver Eingriff durch diese Rollen.

TESTNACHWEIS[TW-1]: B04 und B06 jeweils 1336 bestanden, 35 rot | B01 336 bestanden, 24 rot | B07 und B10 bisher Teilprüfungen | Keine grüne Gesamtsuite

TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 0 belegt | Senke: Tasknachweis
