# Befunde: Twitch-Bot Vollreview

Codebasis: `0ecae1370f1a80d1a101249b5c932663d69be8af`. Stand: 2026-10-08.

R09 ist in den fünf Defektblickwinkeln vollständig geprüft. W02 hat 70 vollständige Reviews für DA01 und DA02 zurückgegeben; die deklarierten Leseintervalle sind rechnerisch geprüft und 59 Rohmeldungen zu 40 Gruppen zusammengeführt. Drei B-Befunde und zwei A-Befunde sind jeweils doppelt bestätigt und in vier Fixpaketen. Der frühere Loopback-Kandidat bleibt nach zwei PLAUSIBEL-Urteilen C. Die übrigen Bereiche sind noch offen.

## Aufnahmebedingungen

- Befund-ID bleibt über Reviewer-, Skeptiker-, Fix- und Gate-Runden stabil.
- A/B-Kandidaten benötigen zwei unabhängige Urteile `BESTÄTIGT` und geprüfte Sol-Transcripts. B benötigt zusätzlich eindeutige Sollbelege.
- Ein fehlender Reviewer oder Skeptiker ist eine Abdeckungslücke, kein negatives Ergebnis und keine Bestätigung.
- Doppelte Kandidaten werden mit ihren Herkunfts-IDs zusammengeführt. Widerlegungen und C-Abstufungen bleiben nachvollziehbar.
- Qualitätskritik und Umbauempfehlungen stehen ergänzend in QUALITAET.md. Sie begründen keine Codeänderung.

## Befundregister

| ID | Ort | Klasse | Szenario und Sollbeleg | Skeptiker 1 | Skeptiker 2 | Status | Commit / Deploy |
|---|---|---|---|---|---|---|---|
| W01-R09-errors-1 | rust/crates/tb-internal-api/src/handlers/streamers.rs:551 | B bestätigt, Eigentümer IA02 | Überlappende identische Anfragen: Owner liefert ursprünglichen Fehlerbody, Waiter erhält pauschal internal_error. Sollbelege: tb-http-core/src/error.rs:205-223; telemetry_routes.rs:384-388; raid_oauth.rs:664-668,780-782. | BESTÄTIGT, Soll belegt, aa07cc50d45eddf83 | BESTÄTIGT, Soll belegt, a6a25344ecabeb67f | erster lokaler Fix; Basisabgleich und Nachweise laufen, kein Merge | 73d7d50232a2d97a2b5b198ead42ee31694cb6bc / kein Deploy |
| W01R2-R09-security-1 | rust/crates/tb-dashboard-api/src/auth/csrf.rs:97 | C, A nicht bestätigt | Behaupteter fremder Webseitenzugriff über direkten Loopback benötigt einen Browser auf dem Diensthost oder lokalen Tunnel sowie erlaubten lokalen Netzwerkzugriff. Diese Voraussetzungen sind nicht belegt; öffentlicher Proxy-Loopback allein umgeht die Guards nicht. | PLAUSIBEL, C, a812b3f91673f6b04 | PLAUSIBEL, C, a39627c0abc74ece2 | nur dokumentiert; lokale Vertrauensgrenze bei späterer Produktentscheidung prüfen | kein Fix |

## W02: Befundketten

Modellnachweise stehen in MODELLE-W02.md. Die beiden ersten B-Kandidaten erhielten in `wf_3b16d4aa-68e` je zwei unabhängige BESTÄTIGT-Urteile mit eindeutigen Sollbelegen. Die frischen Fixer B02 und B03 arbeiten in getrennten Worktrees; Details und vollständige Freigabeketten stehen in BRIEFING-B02.md und BRIEFING-B03.md.

| ID | Ort | Einstufung | Konkretes Szenario und Beleg | Status |
|---|---|---|---|---|
| W02-DA01-S004-concurrency-1 | `rust/crates/tb-dashboard-api/src/obs/ws.rs:484` | B bestätigt, Fix B02 | Bei `obs_docks.enabled=true` beendet das erste Dock Replay, bevor PostgreSQL LISTEN aktiv ist. Ein dazwischen gespeichertes Ereignis wird in den Start-Wasserstand übernommen, aber weder zugestellt noch als Lücke gemeldet. Belege: `obs/bus.rs:393-405,440-458`, Replay-Vertrag `obs/ws.rs:15-19,42-53`. | a2cc748228287326d und aaf00c9fc2e9ce972: BESTÄTIGT, Soll belegt. Fixkette läuft. |
| W02-DA01-S003-correctness-1 | `rust/crates/tb-dashboard-api/src/admin_audit.rs:55` | B bestätigt, Fix B03 | Bei altem ersten und gültigem zweiten `master_dash_session` authentifiziert die Route die gültige Sitzung; das Audit prüft nur den ersten Wert und speichert `admin` statt der bekannten Discord-ID. Belege: `auth/level.rs:408-474,872-917`, `admin_audit.rs:70-88,110-130`. | a21bacec28c4e4452 und ac5c34d004bd0f275: BESTÄTIGT, Soll belegt. Fixkette läuft. |
| W02-DA01-S003-resources-1 | `rust/crates/tb-dashboard-api/src/ai_store.rs:112` | C | Bei deaktivierten SQL-Zeitgrenzen bindet ein gesperrter abgelaufener Datensatz beide Writer-Verbindungen in der vorgeschalteten Bereinigung. Folgereservierungen für andere Gespräche scheitern. Belege: `ai_store.rs:83-126,185-187,445-486`, `lib.rs:2041-2054`. | Nur dokumentiert. Tatsächliche DB-Zeitgrenzen ungeprüft; gewünschtes Zeitbudget ist Produktentscheidung. |
| W02-DA01-S003-resources-2 | `rust/crates/tb-dashboard-api/src/admin_audit.rs:130` | C | Eine exklusive Tabellensperre hält bei deaktivierten SQL-Zeitgrenzen das Audit-INSERT nach einer erfolgreichen Admin-Änderung offen. Die bereits fertige Antwort wird nicht ausgeliefert. Belege: `admin_audit.rs:91-94,118-136`, Router `lib.rs:2245-2248`. | Nur dokumentiert. Laufzeit-Timeouts ungeprüft; Verlust- und Zeitbudget für Audit-Einträge sind nicht festgelegt. |

Die C-Szenarien setzen fehlende PostgreSQL-Zeitgrenzen voraus. Das Live-System wurde dafür nicht abgefragt. Es gibt keine Behauptung eines aktuell eingetretenen Ausfalls und keine Freigabe, Produktionsdaten oder Timeout-Regeln zu ändern.

### Bestätigte Sicherheitsbefunde aus DA02

Beide Befunde erhielten in `wf_9eb7b672-f97` jeweils zwei unabhängige BESTÄTIGT-Urteile, Klasse A und belegtes Soll. Die Sol-Transcripts aller vier Skeptiker sind geprüft. Der gemeinsame Fixer A01 besitzt die drei betroffenen Auth-Dateien; danach folgt ein frischer Fix-Kritiker. Briefing und vollständige Modellnachweise stehen in BRIEFING-A01.md.

| ID | Ort | Bestätigtes Szenario | Beleg | Status |
|---|---|---|---|---|
| W02-DA02-S001-concurrency-1 | `rust/crates/tb-dashboard-api/src/auth/session.rs:2101` | Ein paralleler gleitender Refresh liest eine gültige Sitzung vor Logout und legt dieselbe ID danach per INSERT ON CONFLICT erneut an. Ein bereits kopierter Cookie-Wert wird serverseitig wieder gültig. Weder Cookie-Diebstahl noch automatische Browserwiederherstellung ist nachgewiesen. | Lesen `session.rs:1993-2014`, Refresh `2072-2115`, Invalidierung `1511-1527,2043-2050`, Logout-Vertrag `handlers/auth_login.rs:724-753` | A bestätigt; a56f4e255006e7291 und ac61a921899bce902 jeweils BESTÄTIGT. Fix A01a in Prüfung, kein Merge. |
| W02-DA02-S002-errors-1 | `rust/crates/tb-dashboard-api/src/auth/level.rs:444` | Eine ausdrücklich zentral abgelehnte Admin-Sitzung erhält nach `valid=false` über ihre lokale Kopie weiter Adminrechte. Der technische Ausfallfallback verdeckt den Widerruf. | Fehlerzweig `level.rs:421-457`, zentrale Ablehnung `discord_admin_login.rs:250-262`, lokale Verlängerung `session.rs:1593-1614`, Ablehnungstest `discord_admin_login.rs:1715-1746` | A bestätigt; a636373646c18fc85 und a2d35f684e4064cfc jeweils BESTÄTIGT. Fix A01b in Prüfung, kein Merge. |

W02 ist als Reviewausführung beendet: 70 von 70 Rückgaben melden complete, keine partial- oder missing-Rückgabe. Die 59 Rohkandidaten verteilen sich auf 17 A-, 28 B- und 14 C-Vorschläge. Diese Zahlen enthalten Duplikate und unbestätigte Behauptungen. Die Konsolidierung in `wf_83c5b894-715` ist abgenommen; W02-KANDIDATEN.json und W02-NACHWEIS.md trennen Herkunft, deklarierte Abdeckung und Bestätigungsstände. Noch keine Abschlusszahl bestätigter W02-Defekte daraus ableiten.

A01 erster Fixstand `3718481e9d58c94d1864012fd6c6d6acc55cb10c`: Gate BLOCK wegen erneuter lokaler Zulassung nach ausdrücklicher zentraler Ablehnung und anschließendem technischem Brokerausfall. Frische Runde 2 läuft in `wf_6b030f84-2a4`. Keine Integrationsfreigabe, Details in REVIEW.md.

## Weitere kanonische W02-Kandidaten

Die folgenden 22 Behauptungen sind ungeprüfte Vorschläge. Je zwei frische Skeptiker laufen in `wf_f9b737d9-fe8`. Ihre Urteile stehen noch aus; kein Fix ist freigegeben. Die vier bereits bestätigten Hauptbefunde stehen oben. Vollständige Szenarien, Reviewerbelege und die Zuordnung der 59 Roh-IDs zu 40 Gruppen sind in W02-KANDIDATEN.json erhalten. Modell- und Abdeckungsnachweis: W02-NACHWEIS.md.

Fundorte in den Tabellen sind relativ zu `rust/crates/tb-dashboard-api/` und beziehen sich auf den festen Review-SHA.

| Kanonische ID | Ort | Vorschlag | Zu prüfendes Szenario |
|---|---|---|---|
| W02-DA02-S002-concurrency-1 | `src/auth/level.rs:427` | A | Ein laufender zentraler Admin-Sitzungsimport stellt die lokale Sitzung nach erfolgreichem Logout wieder her. |
| W02-DA02-S004-concurrency-2 | `src/auth/session.rs:1857` | A | Ein noch laufendes Cache-Füllen gewährt nach Logout erneut Zugriff. Eigenständig vom bereits bestätigten Datenbank-Refresh zu prüfen. |
| W02-DA02-S004-correctness-1 | `src/handlers/affiliate.rs:521` | A | Ein neu vergebener Twitch-Login öffnet das Affiliate-Konto der vorherigen Person, falls eine unveränderliche Eigentümer-ID nicht geprüft wird. |
| W02-DA02-S004-security-4 | `src/auth/session.rs:1113` | A | Affiliate-OAuth-State ist nicht an den anfragenden Browser gebunden; behaupteter Login-CSRF-Pfad. |
| W02-DA02-S005-correctness-1 | `src/auth/streamer_scope.rs:155` | A | Wiedervergabe eines Twitch-Logins liefert historische Analysedaten des früheren Kontos. |
| W02-DA02-S005-security-1 | `src/auth/streamer_scope.rs:163` | A | Wiedervergabe eines Twitch-Logins öffnet die KI-Analysehistorie des früheren Kontos. Vom allgemeinen Analysepfad getrennt prüfen. |
| W02-DA01-S004-correctness-1 | `src/lib.rs:1663` | B | Die Kündigungsroute mit literalem ü passt nicht zur browserseitigen URL mit `%C3%BC`. |
| W02-DA01-S004-errors-1 | `src/lib.rs:2276` | B | Ein übergroßer OAuth-Ablaufwert beendet den nicht überwachten periodischen Refresh-Task. |
| W02-DA01-S004-errors-2 | `src/lib.rs:2289` | B | Fehlender Anbieterclient wird im Refreshloop als erfolgreiche Erneuerung behandelt. |
| W02-DA01-S004-security-1 | `src/lib.rs:1015` | B | Die gemeinsame CSRF-Schicht weist gültige Sitzungen aus einmaligen Partner-Anmeldelinks zurück. Sieben Rohmeldungen derselben behaupteten Ursache. |
| W02-DA01-S004-security-2 | `src/lib.rs:1485` | B | Die Partner-Link-Route berücksichtigt gültige konfigurierte interne Admin-Zugänge nicht. |
| W02-DA01-S005-correctness-1 | `src/query_int.rs:51` | B | Ganzzahlen außerhalb des i64-Bereichs ergeben 400 statt der dokumentierten Begrenzung auf erlaubte Werte. |
| W02-DA01-S005-errors-1 | `src/proxy.rs:202` | B | Die gepufferte Fallback-Antwort umgeht die behauptete 16-MiB-Antwortgrenze. Deren Geltung ist unabhängig zu prüfen. |
| W02-DA01-S007-correctness-1 | `tests/plan_stufen_gates.rs:141` | B | Die leere Twitch-ID der Partnerfixture lässt Akzeptanztests vor dem eigentlich zu prüfenden Plan-Gate scheitern. |
| W02-DA02-S001-correctness-2 | `src/auth/idor_e2e_tests.rs:233` | B | Zwei IDOR-Testfixtures konfigurieren die benötigte Operator-Twitch-ID nicht. |
| W02-DA02-S001-errors-2 | `src/auth/csrf.rs:472` | B | Einrichtungsfehler bei konfigurierter Testdatenbank lassen vier CSRF-Regressionstests erfolgreich zurückkehren. |
| W02-DA02-S003-resources-1 | `src/auth/security.rs:108` | B | Eine Datenbanksperre bindet im Anmelde-Ratelimiter die gemeinsam genutzten Verbindungen ohne Ende. Tatsächliche Zeitgrenzen und Sollvertrag ungeprüft. |
| W02-DA02-S003-resources-2 | `src/auth/security.rs:133` | B | Abgelaufene Ratelimit-Treffer werden nicht entfernt und wachsen dauerhaft an. |
| W02-DA02-S004-concurrency-3 | `src/auth/session.rs:2105` | B | Ein überlappender gleitender Refresh überschreibt eine inzwischen abgeschlossene Admin-Fingerprint-Aktualisierung. |
| W02-DA02-S004-correctness-4 | `src/handlers/auth_login.rs:738` | B | Dashboard-Logout beendet eine Sitzung aus dem einmaligen Partner-Anmeldelink nicht. |
| W02-DA02-S004-resources-1 | `src/auth/session.rs:2007` | B | Abgelaufene Dashboardsitzungen und verlassene OAuth-States bleiben dauerhaft in der Datenbank. |
| W02-DA02-S005-errors-1 | `src/auth/session.rs:2617` | B | Aktivierte Auth-Sitzungstests melden Erfolg trotz Fehler bei der Einrichtung ihrer Datenbank. |

## Weitere C-Vorschläge aus W02

Diese zwölf Gruppen ergänzen die beiden bereits dokumentierten Timeout-Szenarien. Damit sind 14 kanonische C-Vorschläge erfasst. Es sind keine bestätigten A/B-Defekte und keine Änderungsaufträge. Laufzeitvoraussetzungen wurden nicht am produktiven Dienst geprüft.

| Kanonische ID | Ort | Szenario und begrenzte Empfehlung |
|---|---|---|
| W02-DA01-S004-correctness-2 | `src/lib.rs:2007` | `/streamer` verweist im nativen Router auf nicht registriertes `/streamer/`. Vertrag der vorgeschalteten Website-Auslieferung klären. |
| W02-DA01-S004-errors-3 | `src/lib.rs:2291` | Vorübergehende OAuth-Refreshfehler verschwinden im periodischen Loop. Gewünschte Diagnose festlegen, keine Wiederholungsregeln nebenbei ändern. |
| W02-DA01-S005-resources-2 | `src/proxy.rs:147` | Ein stockender eingehender Body bindet den Fallback-Aufruf vor Beginn des Upstream-Timeouts. Vorhandene Proxy-Zeitgrenzen prüfen, bevor ein neues Zeitbudget festgelegt wird. |
| W02-DA01-S007-concurrency-1 | `tests/plan_stufen_gates.rs:108` | Parallele Testläufe mit derselben Datenbank löschen einander fest benannte Schemata. Testisolation getrennt verbessern. |
| W02-DA02-S001-correctness-3 | `src/auth/csrf.rs:208` | Der Same-Origin-Vergleich weist passende IPv6-Hosts zurück. Unterstützte Bereitstellungsformen vor einer Änderung klären. |
| W02-DA02-S001-errors-3 | `src/auth/csrf.rs:134` | Fehler des Sitzungsspeichers erscheinen als invalid_csrf. Fehlervertrag gesondert klären. |
| W02-DA02-S001-resources-1 | `src/auth/csrf.rs:130` | Mehrere Sitzungscookies multiplizieren Datenbankwartezeiten. Erlaubte Cookie- und Zeitbudgets festlegen, Schutzschichten erhalten. |
| W02-DA02-S002-resources-1 | `src/auth/level.rs:414` | Wiederholte ungültige Admin-Cookies vervielfachen Brokeranfragen und Wartezeit. Grenzen erst nach Prüfung der bisherigen Mehrfachcookie-Semantik definieren. |
| W02-DA02-S003-errors-1 | `src/auth/security.rs:459` | Aktivierte Ratelimit-Integrationstests können bei fehlgeschlagener Datenbankeinrichtung erfolgreich enden. Einrichtungs- und Skip-Vertrag prüfen. |
| W02-DA02-S004-errors-1 | `src/auth/session.rs:2045` | Verschlucktes DELETE-Versagen beim Logout lässt den alten Sitzungswert nach Datenbankerholung verwendbar. Gewünschtes Verhalten bei Speicherausfall gesondert entscheiden. |
| W02-DA02-S004-resources-2 | `src/auth/session.rs:2115` | Ein Refresh wartet ohne wirksame PostgreSQL-Zeitgrenze auf einer Zeilensperre. Reale Grenzen zuerst belegen, keine produktive DB-Konfiguration ändern. |
| W02-DA02-S005-resources-1 | `src/auth/session.rs:2618` | Sitzungs-Testfixtures behalten erzeugte Datenbankschemata. Aufräumstrategie als separaten Testinfrastruktur-Umbau behandeln. |

## Grenzen

R09: fünf Defektblickwinkel, beide Gegenprüfungen je A/B-Kandidat und die frische Qualitätskritik abgeschlossen. Note 3/5 und C-Empfehlungen stehen in QUALITAET.md. Kein fertiger Fix, kein Code-Merge, kein Deploy und kein Live-Nachweis. Der reine Auftragsdokumentationscheckpoint 6937e4a6 ist auf main und ändert keinen Anwendungscode.
