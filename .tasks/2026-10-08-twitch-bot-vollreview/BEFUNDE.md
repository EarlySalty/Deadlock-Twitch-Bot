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

## Grenzen

R09: fünf Defektblickwinkel, beide Gegenprüfungen je A/B-Kandidat und die frische Qualitätskritik abgeschlossen. Note 3/5 und C-Empfehlungen stehen in QUALITAET.md. Kein fertiger Fix, kein Code-Merge, kein Deploy und kein Live-Nachweis. Der reine Auftragsdokumentationscheckpoint 6937e4a6 ist auf main und ändert keinen Anwendungscode.
