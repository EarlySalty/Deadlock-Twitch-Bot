# Befunde: Twitch-Bot Vollreview

Codebasis: `0ecae1370f1a80d1a101249b5c932663d69be8af`. Stand: 2026-10-08.

R09 ist in den fünf Defektblickwinkeln vollständig geprüft. Ein B-Befund ist doppelt bestätigt und als Fixpaket B01 beauftragt. Ein ursprünglich als A vorgeschlagener Befund bleibt nach zwei PLAUSIBEL-Urteilen C. Andere Bereiche sind noch offen.

## Aufnahmebedingungen

- Befund-ID bleibt über Reviewer-, Skeptiker-, Fix- und Gate-Runden stabil.
- A/B-Kandidaten benötigen zwei unabhängige Urteile `BESTÄTIGT` und geprüfte Sol-Transcripts. B benötigt zusätzlich eindeutige Sollbelege.
- Ein fehlender Reviewer oder Skeptiker ist eine Abdeckungslücke, kein negatives Ergebnis und keine Bestätigung.
- Doppelte Kandidaten werden mit ihren Herkunfts-IDs zusammengeführt. Widerlegungen und C-Abstufungen bleiben nachvollziehbar.
- Qualitätskritik und Umbauempfehlungen stehen ergänzend in QUALITAET.md. Sie begründen keine Codeänderung.

## Befundregister

| ID | Ort | Klasse | Szenario und Sollbeleg | Skeptiker 1 | Skeptiker 2 | Status | Commit / Deploy |
|---|---|---|---|---|---|---|---|
| W01-R09-errors-1 | rust/crates/tb-internal-api/src/handlers/streamers.rs:551 | B bestätigt, Eigentümer IA02 | Überlappende identische Anfragen: Owner liefert ursprünglichen Fehlerbody, Waiter erhält pauschal internal_error. Sollbelege: tb-http-core/src/error.rs:205-223; telemetry_routes.rs:384-388; raid_oauth.rs:664-668,780-782. | BESTÄTIGT, Soll belegt, aa07cc50d45eddf83 | BESTÄTIGT, Soll belegt, a6a25344ecabeb67f | Fix B01 beauftragt, nur gemeinsamer Wrapper | noch keiner |
| W01R2-R09-security-1 | rust/crates/tb-dashboard-api/src/auth/csrf.rs:97 | C, A nicht bestätigt | Behaupteter fremder Webseitenzugriff über direkten Loopback benötigt einen Browser auf dem Diensthost oder lokalen Tunnel sowie erlaubten lokalen Netzwerkzugriff. Diese Voraussetzungen sind nicht belegt; öffentlicher Proxy-Loopback allein umgeht die Guards nicht. | PLAUSIBEL, C, a812b3f91673f6b04 | PLAUSIBEL, C, a39627c0abc74ece2 | nur dokumentiert; lokale Vertrauensgrenze bei späterer Produktentscheidung prüfen | kein Fix |

## W02: erste Kandidaten, noch keine weitere Fixfreigabe

Modellnachweise der drei Reviewer stehen in MODELLE-W02.md. Die beiden B-Kandidaten durchlaufen jeweils zwei unabhängige Skeptiker in `wf_3b16d4aa-68e`.

| ID | Ort | Einstufung | Konkretes Szenario und Beleg | Status |
|---|---|---|---|---|
| W02-DA01-S004-concurrency-1 | `rust/crates/tb-dashboard-api/src/obs/ws.rs:484` | B-Kandidat | Erstes Dock beendet Replay, bevor PostgreSQL LISTEN aktiv ist. Ein dazwischen gespeichertes Ereignis wird in den Start-Wasserstand übernommen, aber weder zugestellt noch als Lücke gemeldet. Belege: `obs/bus.rs:393-405,440-458`, Replay-Vertrag `obs/ws.rs:15-19,42-53`. | Zwei Skeptiker laufen; kein Fix. |
| W02-DA01-S003-correctness-1 | `rust/crates/tb-dashboard-api/src/admin_audit.rs:55` | B-Kandidat | Bei altem ersten und gültigem zweiten `master_dash_session` authentifiziert die Route die gültige Sitzung; das Audit prüft nur den ersten Wert und speichert `admin` statt der bekannten Discord-ID. Belege: `auth/level.rs:408-474,872-917`, `admin_audit.rs:70-88,110-130`. | Zwei Skeptiker laufen; kein Fix. |
| W02-DA01-S003-resources-1 | `rust/crates/tb-dashboard-api/src/ai_store.rs:112` | C | Bei deaktivierten SQL-Zeitgrenzen bindet ein gesperrter abgelaufener Datensatz beide Writer-Verbindungen in der vorgeschalteten Bereinigung. Folgereservierungen für andere Gespräche scheitern. Belege: `ai_store.rs:83-126,185-187,445-486`, `lib.rs:2041-2054`. | Nur dokumentiert. Tatsächliche DB-Zeitgrenzen ungeprüft; gewünschtes Zeitbudget ist Produktentscheidung. |
| W02-DA01-S003-resources-2 | `rust/crates/tb-dashboard-api/src/admin_audit.rs:130` | C | Eine exklusive Tabellensperre hält bei deaktivierten SQL-Zeitgrenzen das Audit-INSERT nach einer erfolgreichen Admin-Änderung offen. Die bereits fertige Antwort wird nicht ausgeliefert. Belege: `admin_audit.rs:91-94,118-136`, Router `lib.rs:2245-2248`. | Nur dokumentiert. Laufzeit-Timeouts ungeprüft; Verlust- und Zeitbudget für Audit-Einträge sind nicht festgelegt. |

Die C-Szenarien setzen fehlende PostgreSQL-Zeitgrenzen voraus. Das Live-System wurde dafür nicht abgefragt. Es gibt keine Behauptung eines aktuell eingetretenen Ausfalls und keine Freigabe, Produktionsdaten oder Timeout-Regeln zu ändern.

## Grenzen

R09: fünf Defektblickwinkel, beide Gegenprüfungen je A/B-Kandidat und die frische Qualitätskritik abgeschlossen. Note 3/5 und C-Empfehlungen stehen in QUALITAET.md. Kein fertiger Fix, kein Code-Merge, kein Deploy und kein Live-Nachweis. Der reine Auftragsdokumentationscheckpoint 6937e4a6 ist auf main und ändert keinen Anwendungscode.
