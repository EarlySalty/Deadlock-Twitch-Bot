# Wiederaufnahme des Vollreviews

## Aktueller Wiederaufnahmestand nach der Zwölf-Stunden-Auskunft

Am 2026-10-08 erneut per ls-remote bestätigt: main f04c0ef03d47ce4893ee4d17cb98eeb4c2f6c473 und Artefaktbranch 1adb7e802ffe89a5597073f92481bdae8383dfb1. B02 und A01 bleiben die zwei nachgewiesenen Anwendungscode-Integrationen. Kein Deploy, Neustart oder Live-Nachweis. Die folgende aktuelle Tabelle ersetzt abweichende Aktivmeldungen im historischen Stand darunter.

| Zweck | Run-ID | Task-ID | Zustand bei Journalprüfung |
|---|---|---|---|
| Serielle Integration B05, danach B09, danach B08 | wf_beeac761-2c8 | wpx113kdx | B05 gestartet, kein Ergebnis; B09/B08 noch nicht gestartet |
| A02 frischer Fix Runde 2 und Kritik | wf_8d15f5fe-eff | wx57xab14 | Fixer gestartet, kein Ergebnis |
| B03 frischer Fix Runde 4 und Kritik | wf_e6ba5fe6-44e | wgefq8ej9 | Fixer gestartet, kein Ergebnis |
| B01/B04/B06/B07/B10 Integrationsvorbereitung | wf_f7342e70-084 | wq0ls3kxm | Fünf Rollen gestartet, keine Ergebnisse |
| Zwei fehlende Urteile ergänzen | wf_287d5302-dbb | w3c9l4lav | SM04-Kritik und zweiter Skeptiker zu W03-DA06-S001-errors-1 gestartet, keine Ergebnisse |
| Q04-Teilberichte 1, 2 und 4 | wf_874d1dcd-995 | wg0cyy3ty | Drei Rollen gestartet, keine Ergebnisse; Teil 3 noch nicht gestartet |
| W03-Restgegenprüfung | wf_16dfb9cb-031 | wc1nrzzig | 36 Ergebnisse von 180 geplanten Skeptikern im Journal |
| W04 | wf_b01ff274-9fa | wyiyzgmff | 109 Ergebnisse von 230 geplanten Reviews im Journal |
| W05 | wf_a9db9baa-10f | wanhv3338 | 80 Ergebnisse von 485 geplanten Reviews im Journal |
| W06 | wf_d11f7416-b1a | wa8zdhmi2 | 51 Ergebnisse von 440 geplanten Reviews im Journal |
| W07 | wf_7417fa2e-5e9 | wi8qh46vu | 45 Ergebnisse von 835 geplanten Reviews und ein Ausfall im Journal |

Die Journalzählungen belegen vorhandene Rückgaben, weder fachliche Abnahmen noch den gegenwärtigen Prozesszustand. Vor Wiederaufnahme erhaltene Arbeit und Journal prüfen; keine Doppelstarts. Astra führt während der seriellen Integrationskette keinen anderen eigenen Main-Push aus. BRIEFING-SERIELLE-INTEGRATION-02.md bindet die drei Integrationsrollen. Artefaktsicherungen auf dem Auditbranch bleiben davon unabhängig. Die drei neuen Integrationsberichte liegen bei dieser Prüfung noch nicht vor.

B08-Prüfabschluss und echte frische Kritik sind beendet und auf Sol geprüft: ABSCHLUESSE-13.json und B08-PRUEFABSCHLUSS.md, Head 70d26a8b00189d3212af53337935757eb576a50e auf e98b7f01. Standard-Suite 1337/31/6 ignoriert gegen 1332/31/6, 13 Parserfälle bestanden, Clippy beidseitig Exit 0. Der optionale Research-Test liefert beim vorzeitigen Rücksprung keinen HTTP-/DB-Nachweis. Keine neue Main-Integration daraus ableiten.

Q04-ORIGINALE.json enthält 191 geprüfte Originale: 96 Bewertungen und 95 Kritiken, 9432 echte Sol-Datensätze und sieben getrennte synthetische Datensätze. 43 Kritiken BESTÄTIGT, 52 KORRIGIERT; SM04 fehlt nach API 403. Q04-SM04-BEWERTUNG.json erhält die unveränderte Eingabe für die Ersatzrolle. Noch keine globale Gesamtnote oder fünf abschließenden Empfehlungen. Exakte Teilberichtseingaben in WORKFLOW-ARGS.json; Teil 3 wartet auf die fehlende SM04-Kritik.

W03-DA04-DA06-URTEILE-01.json enthält den neutralen Extraktor und 133 Skeptiker, Modelle/Hashes/Originale geprüft: 5873 echte Sol-Datensätze und zwei getrennte synthetische Datensätze. Ein zweites Urteil fehlt nach API 403. W03-DA04-DA06-PAARE-01.json ordnet die vorhandenen Rückgaben mechanisch zu: fünf A, 57 B, vier C und ein offenes Paar. Diese Zuordnung ist noch keine abschließende Unabhängigkeitsabnahme, neue Fixfreigabe oder aktualisierte Gesamtbefundzahl. Bestehende C-Gruppen bleiben ausgeschlossen.

Status 8 wf_63f902dc-aae ist beendet und jetzt abgenommen: Agent a297458851a9ba920, 15 echte Sol-Datensätze, fertiger Transcript-SHA256 9b384e9517c3080e50b5e39f3f5d5920cc39018ee524741a1a949843ed3812f5. Letzte StructuredOutput-Rückgabe stimmt exakt mit dem Journal überein. Der vorherige TODO-Inhalt ist wortgleich erhalten. Ereignis 8 bleibt historisch; spätere Abschlüsse gehören in Ereignis 9.

Der Nutzer erhielt eine Statusauskunft, keine Abschlussmeldung. Die Deployfrage wurde erneut gestellt; weiterhin keine menschliche Freigabe. Die Stop-Hook-Meldung über offene Branches ersetzt keine fehlenden Reviews oder Integrationsnachweise. Aktive Fixdateien, benötigte Belegworktrees und der fremde Hauptcheckout bleiben erhalten; keine unvollständig geprüfte Arbeit zum Erzwingen eines Abschlusses mergen oder löschen.

Nächster Schritt: diese abgeschlossenen Metadaten gezielt sichern, erhaltene Rückgaben auswerten und die bestehenden Fixketten unter ihren Schreibgrenzen weiterführen.

## Historischer Stand vor diesen Nachträgen

Stand: 2026-10-08, nach A01-Integration und serieller B03-Freigabe. 108 Defektbereiche beauftragt, zwölf Bereichskonsolidierungen technisch abgenommen. B02 und A01 integriert, kein Deploy oder Live-Nachweis. Fertige Rückgaben nach diesem Stand sind gesondert abzunehmen.

## Auftrag und feste Orte

Astra orchestriert, schreibt keinen Anwendungscode. T3 c88f4057-c6b3-4c54-8750-addd08b24b42, native Session f61905e7-f7ff-405b-a6d7-090dec371fcb. AUFTRAG.md ist verbindlich. Keine Sessionnachrichten oder weiteren T3-Threads.

Taskordner: `/home/nathanael/.worktrees/tb-vollreview-artefakte/.tasks/2026-10-08-twitch-bot-vollreview/`, Branch `audit/tb-vollreview-20261008`. Letzte erneut remote bestätigte Sicherung `b47669f408ba5f00e7d9eec9703bc006ef05aa5d`. Neuere Artefakte gezielt sichern, laufend geschriebene Teildateien nicht als fertig committen.

Transcript-Basis: `/home/nathanael/.claude/projects/-home-nathanael-repos-Deadlock-Twitch-Bot/f61905e7-f7ff-405b-a6d7-090dec371fcb/subagents/workflows/`.

Script-Basis: `/home/nathanael/.claude/projects/-home-nathanael-repos-Deadlock-Twitch-Bot/f61905e7-f7ff-405b-a6d7-090dec371fcb/workflows/scripts/`.

Feste Reviewbasis `0ecae1370f1a80d1a101249b5c932663d69be8af`, 108 Bereiche, 450 Abschnitte, 597180 Primärzeilen. Hauptcheckout und fremde Arbeit unangetastet. Aktuelles Remote-main nach A01-Push als `f04c0ef03d47ce4893ee4d17cb98eeb4c2f6c473` bestätigt. Tracking-Zeiger wird geteilt; vor Integration frisch holen und tatsächlichen Diff prüfen.

Reviewer, Skeptiker, Kritiker und Fixer ausschließlich gpt-6.1-sol. Echte message.model-Felder, fertige Transcript-Hashes und Originalgleichheit prüfen. Synthetische Meldungen getrennt zählen. Ohne zwei unabhängige BESTÄTIGT kein Fix, B zusätzlich belegtes Soll. C-Sperren erhalten. Bei fachlichem BLOCK frischer Fixer, nach fünf erfolglosen Runden eskalieren. Keine Kritik leerer oder unsauberer Diffs.

## Aktive Ausführungen

| Zweck | Run-ID | Task-ID | Script |
|---|---|---|---|
| B03 frischer Fix nach A01 und Kritik | wf_e6ba5fe6-44e | wgefq8ej9 | tb-vollreview-b03-fixrunde-4-wf_e6ba5fe6-44e.js |
| A02 frische Fixrunde 2 und Kritik | wf_8d15f5fe-eff | wx57xab14 | tb-vollreview-a02-fixrunde-2-wf_8d15f5fe-eff.js |
| W03 Rest: 90 Behauptungen gegenprüfen | wf_16dfb9cb-031 | wc1nrzzig | tb-vollreview-w03-rest-gegenpruefung-wf_16dfb9cb-031.js |
| B01/B04/B06/B07/B10 Integrationsvorbereitung | wf_f7342e70-084 | wq0ls3kxm | tb-vollreview-integrationsvorbereitung-03-wf_f7342e70-084.js |
| B08 erhaltener Abschluss und Kritik | wf_58a43069-334 | w4qi2a6tt | tb-vollreview-b08-r2-abschluss-wf_58a43069-334.js |
| W04 Defektreviews | wf_b01ff274-9fa | wyiyzgmff | tb-vollreview-defektwelle-w04-wf_b01ff274-9fa.js |
| W05 Defektreviews | wf_a9db9baa-10f | wanhv3338 | tb-vollreview-defektwelle-w05-wf_a9db9baa-10f.js |
| W06 Defektreviews | wf_d11f7416-b1a | wa8zdhmi2 | tb-vollreview-restliche-defektbereiche-wf_d11f7416-b1a.js |
| W07 Defektreviews | wf_7417fa2e-5e9 | wi8qh46vu | tb-vollreview-restliche-defektbereiche-wf_d11f7416-b1a.js |
DA04/DA05/DA06-Gegenprüfung wf_655679bf-2b7 ist gerade beendet: 134 von 135 Rollen fertig, darunter neutraler Extraktor und 133 Skeptiker. W03-DA06-S001-errors-1:skeptic-2 nach API 403 ohne Urteil. Journal und Modelle noch abnehmen, genau die ausgefallene Rolle ergänzen; fertige Urteile nicht wiederholen und vor Abnahme keine neuen bestätigten Zahlen.

Q04 wf_e3b6f95d-94c ist inzwischen beendet: 191 von 192 Rollen fertig, SM04:quality-critic nach API 403 ohne Urteil. Ergebnisse zuerst am Journal prüfen, Modelle/Originale abnehmen und genau die fehlende Kritik ergänzen. Kein Wiederholen der 191 erhaltenen Rollen, noch keine Gesamtnote.

Exakte args in WORKFLOW-ARGS.json. W06 und W07 nutzen dasselbe Script mit verschiedenen args und Run-IDs. A02 Runde 2, B03 Runde 4 und B08 ohne args. Nicht doppelt starten. Nach Abbruch zunächst Journal und erhaltene lokale Arbeit prüfen. Integrationsprüfrollen dürfen keinen Anwendungscode korrigieren oder Main-Push ausführen; die ausdrücklich beauftragten frischen Fixer haben ihre engen Briefinggrenzen.

## Neu beendet und abgenommen

ABSCHLUESSE-10.json: zehn Originalrückgaben, 824 echte Sol-Datensätze, keine synthetischen. Modelle, fertige Hashes und StructuredOutput-/Journalgleichheit geprüft. SHA256 d61fe798fd465c8430dec5a83a4659586e3cf3ace5351f01453991adf13126c1. NACHWEIS-ABSCHLUESSE-10.md und BRIEFING-INTEGRATIONSVORBEREITUNG-03.md beschreiben echte Prüfgrenzen.

B01-Prüfbindung wf_e099cf2a-fc9 beendet und abgenommen. B01-PRUEFBINDUNG.md rekonstruiert tatsächlichen historischen Fixhead 307c09e6; derselbe Rust-Baum wie aktueller Head c3aa3cc9. Neue Regression bestanden, historische Suite 336/24 gegen 335/24. Neun Loghashes erneut geprüft. Jüngstes Clippy im unveränderten tb-chat blockiert, passende vollständige Baseline-Clippybindung offen. Neue Integrationsvorbereitung gestartet, kein B01-Merge.

B04/B06/B07 wf_961bca08-8d7 beendet, sechs Rollen samt echten Kritiken abgenommen. B04 d760300d und B06 46c52a92 jeweils ALLOW, vollständige Suite 1336/35 gegen 1333/35, Clippy beidseitig Exit 0. B04 neun Routerfälle, B06 14 Proxyfälle bestanden. B06 hat 255 fremde Formatblöcke, nicht 265. B07 9ec607b3 ALLOW, Fokusziel 11/1 gegen 3/9. Verbliebener getrennt fehlender TikTok-Testinput ist kein Nebenfixauftrag; vollständige Crate-Suite fehlt noch.

B10 wf_b1cabe6f-009 beendet und abgenommen: 9bce62f1, Kritik ALLOW, 13 Fokusfälle bestanden. Tatsächlicher Paketaufruf ohne --no-fail-fast brach nach dem Bibliotheksziel ab. 1321/21/3 ignoriert gegen 1317/21/3 ist kein Vollsuitennachweis; Integrationstests und Doctests fehlen. C-gesperrte HTTP-Cachepolitik unverändert. Neue Integrationsvorbereitung beauftragt, keine zusätzliche Codekorrektur.

Status 7 wf_f1a93c64-6ee beendet, Modell/Hash und TODO-Diff geprüft. Historie erhalten. Ereignis 7 ist historisch und wird nicht umgeschrieben; neue Abschlüsse und W06/W07 benötigen ein neues Ereignis. B02 wurde durch Astra integriert, nicht durch den zuvor ohne Push beendeten Worker.

## W03-Restkonsolidierung und Claimabgleich

DA07, DA15, DA17, MO01 und IA01 technisch abgenommen. 105 Reviewertranscripts, 5001 echte Sol-Datensätze, 166 Rohbefunde exakt erhalten, jede Roh-ID genau einmal zugeordnet. Vollständige erwartete Labels und deklarierte Primärintervalle. Fünf Konsolidierer ebenso geprüft. Keine tatsächliche Volllektüre aus bloßen Intervallangaben ableiten. Fehlender IA01-Legacy-Kontext bot/internal_api/app.py ist keine Primärlücke.

Nachweise: W03-NACHWEIS-03.md und W03-REST-VERIFIKATION.json, letzterer SHA256 ba4d94919a4f57273592a402aa80f05c07d72092d4e45f8e12df07ab5e9f6468. Fünf ursprüngliche Kandidatenartefakte unverändert. 126 neue Gruppen und fünf bestehende Verknüpfungen; konservativ 90 reine A/B-Vorschläge und 36 C-Gruppen. Vier gemischte B/C-Gruppen bleiben C: DA15-S001-concurrency-1, DA17-S001-correctness-4, MO01-S001-concurrency-2 und IA01-S002-errors-1, jeweils mit W03-Präfix.

Restclaimabgleich wf_3c0cb7ee-5b4 ist beendet und abgenommen. 58 echte Sol-Datensätze, Hash und Originalrückgabe in ABSCHLUESSE-11.json. Astra prüfte zwölf Quellhashes, die 126 Gruppen/166 Originalmitglieder, 90 exakte neutrale Behauptungen und sämtliche C-Ausschlüsse. Keine zusätzlichen vollständigen Duplikate belegt, sechs offene partielle Überlappungen dokumentiert. W03-REST-CLAIMS.json SHA256 3c338827a8d28b9e0091a4f3ba7ef70dcc4d8d64b9728319af9258329d4c4357. Neue blind begrenzte Eingabe W03-REST-NEUTRALE-CLAIMS.json mit exakt vier neutralen Feldern, SHA256 0f3c3298add523c126fe5ed55c1d239d20d3668caf841b3a8dac5bb6ab8f8420. Neue Gegenprüfung wf_16dfb9cb-031 gestartet: 90 Behauptungen, je zwei unabhängige Skeptiker; args gespeichert. Index nullbasiert auf die hashgebundene neutrale Liste beziehen. Keine neuen Bestätigungen aus dem Start. Alte 67-Claim-Gegenprüfung bleibt separat.

## Übrige Fixstände und Eigentum

Worktrees unter `/home/nathanael/.worktrees/`. Pfade relativ zu rust/crates/.

| Paket | Worktree | Ursprüngliche exklusive Schreibpfade |
|---|---|---|
| B01 | tb-vollreview-idempotenz | tb-internal-api/src/handlers/streamers.rs |
| B02 | tb-vollreview-obs-start | tb-dashboard-api/src/obs/bus.rs, obs/ws.rs; integriert, erhalten |
| B03 | tb-vollreview-audit-akteur | tb-dashboard-api/src/admin_audit.rs und auth/level.rs; frischer Fixer Runde 4 |
| A01 | tb-vollreview-session-widerruf | Integriert, Writer beendet; auth/level.rs seriell an B03 übergeben |
| A02 | tb-vollreview-affiliate-eigentuemer | tb-dashboard-api/src/handlers/affiliate.rs, handlers/affiliate_portal.rs |
| B04 | tb-vollreview-router-vertraege | tb-dashboard-api/src/lib.rs |
| B05 | tb-vollreview-plattform-refresh | tb-dashboard-api/src/handlers/platform_token.rs, handlers/platform_store.rs, handlers/plattform_oauth.rs |
| B06 | tb-vollreview-proxy-antwort | tb-dashboard-api/src/proxy.rs |
| B07 | tb-vollreview-plan-fixture | tb-dashboard-api/tests/plan_stufen_gates.rs |
| B08 | tb-vollreview-query-grenzen | tb-dashboard-api/src/query_int.rs, handlers/admin_research.rs |
| B09 | tb-vollreview-idor-fixture | tb-dashboard-api/src/auth/idor_e2e_tests.rs |
| B10 | tb-vollreview-authstatus | tb-dashboard-api/src/handlers/auth_status.rs |

A01 ist als f04c0ef03d47ce4893ee4d17cb98eeb4c2f6c473 integriert. Vorbereitungsrolle a55303cd25a62b63a, 130 Sol-Datensätze, fertiger Transcript-Hash c9515afe465bd141352d39d20895580184b7bcfd9d9f5da4f23118ded68dcc2b. ABSCHLUESSE-12.json, SHA256 b487d36512e722139e3f4fde82d8a202087831f29408c5fbb54c768de4508d13. Zehn Loghashes durch Astra geprüft; vollständige finale Suite 1341/35 gegen 1334/35, sieben Regressionen und 50 OBS-Tests bestanden, sieben sichere Skips sowie sieben erwartete aktive Setupfehler. Unveränderter Patchhash 96f93acb45671d66431172fb225213a64664c895dc22fb3058d647c328d1e013. Sieben Git-Schritte einzeln, aktueller Sol-Gate bestätigt, regulärer Push b9iarfbzz Exit 0, Remote-Ref bestätigt. Kein Deploy/Neustart/Aufräumen.

B03 Runde 2 fachlich BLOCK, Runde 3 reiner Abhängigkeitsblocker. Nach A01-Integration ist auth/level.rs jetzt seriell zusätzlich zu admin_audit.rs freigegeben. Frischer Fixer Runde 4 und Kritiker wf_e6ba5fe6-44e aktiv, BRIEFING-B03-R4.md. Ausgangspunkt aa4509784c0e689dfc4e45b892222a6e01a5f8cc auf B02-Basis; regulärer Abgleich und request-lokale Übertragung der tatsächlichen Identität beauftragt. Andere Auth-Dateien und lib.rs bleiben ausgeschlossen.

A02/B05/B09-Vorbereitung wf_a21871df-c4d beendet und Modelle/Originale abgenommen, ABSCHLUESSE-11.json (SHA256 164d36ec1298efc3cdab551edc7821b0ac96ec6897d97c2f8fa8a160ab0b075c). Drei saubere Heads auf B02-Basis, Originalpatches unverändert. A02 e6765a5dd5440fe32dbd4896d1c85db1af3a54ef hat jetzt aktuellen Gate-BLOCK: drei neue Tests erwarten ohne freiwillige DB-Aktivierung bedingungslos einen Pool. Frühere ALLOWs heben dies nicht auf. Frischer Fixer Runde 2 und anschließender Kritiker wf_8d15f5fe-eff, Task wx57xab14, BRIEFING-A02-R2.md; ausschließlich die beiden Affiliate-Dateien. Kein neuer Produktauftrag.

B05 6383a00f031750095a38ff25809aee75a95cf25d: passende historische Baselinebindung geschlossen, neue finale Vollsuite 1337/35 gegen aktuelle 1334/35, vollständige Fehlerkörper gleich, 40 Fokusfälle und 50 OBS-Fälle bestanden, Clippy beidseitig Exit 0, Sol-Gate ALLOW. Noch kein Main-Push. B09 fa01de7b12f51bde4a9af922ae3e3bcf7ee72598: neue Vollsuite mit 29 gegen 31 Baselinefehlern, genau zwei IDOR-Fehler entfernt, passende Körper, zwei DB-Fokusfälle bestanden, Clippy Exit 0, Sol-Gate ALLOW. Standardflags lassen vier gewöhnliche Tests und zwei Doctests ignoriert; keinen positiven Doctestnachweis behaupten. Vor Integration eigene Dokumente, tatsächliche Logs, frische Basis und exakten Gatezustand prüfen. Kein B05-/B09-Merge bislang.

B08 Runde 2 nach Kontextlimit erhalten: 0e3ea42398c9fd54e7af1349aabf217ec6916b27 und /tmp/tb-b08-r2-evidence/. Neuer Abschluss ohne Codeänderung, anschließend frische Kritik aktiv. Vorhandenes Gate-ALLOW braucht korrekte Basisbindung. BRIEFING-B08-R2-ABSCHLUSS.md.

## Bereits integriertes Paket und Sicherungsgrenzen

B02 e98b7f016dbab373a5a8dd9490d158b136c97fec auf Basis a8b5b5e986a1de0b8e2f981651f83bda9cf400dd. Unveränderter Diffhash 7c951fc71c506ed50f93ad037d4c2b092faa6726b251da41d91a176f7e90a3e5, echte Kritik und passender Sol-Gate. Astra führte sieben Git-Schritte einzeln aus, ein regulärer Push, Exit 0, Remote bestätigt. NACHWEIS-ABSCHLUESSE-07.md, Pushoutput bxw7q3ncn.output. 50 OBS-Tests bestanden, Suite 1334/35, gleiche Baselinefehler. Kein Deploy, Neustart oder Aufräumen.

Gitleaks-Blocker von 416851e6 ist erledigt. Zwei Workflowreferenzen waren Fehlalarme; Metadatenfelder *_key zu *_workflow_id präzisiert, Werte/Urteile unverändert. Kein Hook-/Allowlistumbau. SICHERUNG-02.md. Aktueller DA03-Exporthash 46a472cf3d741405fff02cfcc0e0617fb9da0d649f80db33d06e37b7db38febe; ältere Ereignishashes bleiben historisch korrekt.

## Weitere Abdeckung und nächste Arbeit

R09/W02/DA03 weiterhin 13 A, 25 B, 30 C; Befundzahlen sind keine erledigten Fixzahlen. DA04/DA05/DA06 haben 67 ungeprüfte A/B-Vorschläge und 29 ursprüngliche C-Vorschläge. Neue fünf W03-Bereiche ergänzen vor Quervergleich 90 reine A/B-Vorschläge und 36 konservative C-Gruppen. Keine neuen bestätigten Zahlen aus Konsolidierungen.

W04: 13 Bereiche/46 Abschnitte/230 Reviews. W05: 22/97/485. W06: elf/88/440. W07: 50/167/835. Zusammen mit früheren Bereichen sind 108 eindeutige IDs und 450 geplante Abschnitte verteilt. Starts sind keine Abnahmen. Qualitätskritik für zwölf frühere Bereiche abgeschlossen; Q04 hat weitere 96, noch keine Gesamtnote.

Zuerst neue Claimliste abnehmen, fertige Nachweise gezielt sichern, neue Statusmeldung erstellen. Laufende Integrationsvorbereitungen und Reviewwellen erhalten. Weitere bestätigte W02-/DA03-Claims außerhalb der bisherigen Fixpakete bleiben offen.

Deploy weiterhin gesperrt: `/usr/local/bin/deploy-twitch-release` startet Migrationen und verändert PostgreSQL-Peerregeln/Konfiguration. Freigabefrage unbeantwortet. Hintergrundmeldungen oder Fortsetzungsanweisungen sind keine Zustimmung. Kein Ersatzweg, Skip-Schalter oder Wrapperumbau.

Keine Secrets/ENV, schreibende Prod-DB-Befehle, Migrationen, echten Kontoaktionen oder ai-coach. Statische Reviewer ohne Builds/Tests/Probes oder Browser. Browserprüfungen später nur nach Moli-Leitfaden, nie Brave. Python-Anwendungscode unverändert, kurze Metadaten-/Prüfskripte erlaubt. Git einzeln mit literalen absoluten Pfaden, kein add -A, Schutz- oder Modellbypass. Kein Löschen ohne Ancestry-Exit-Code und Artefaktprüfung.
