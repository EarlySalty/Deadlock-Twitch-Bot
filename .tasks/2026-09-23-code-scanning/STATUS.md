# Code-Scanning: Prüfnachweis vom 23.09.2026

## Stand und Geltungsbereich

Repository: EarlySalty/Deadlock-Twitch-Bot. Sicherheitsänderungen in PR #957 auf `fix/code-scanning-20260923`, getrennt vom CI-Umbau in PR #953. Die Meldungen auf `main` gelten nicht durch einen erfolgreichen Feature-Branch-Scan als behoben. Kein Merge, Produktiv-Deploy oder Dienst-Neustart in dieser Fortsetzung.

Die ursprüngliche Bestandsaufnahme umfasste 104 offene Meldungen. Nach der vorangegangenen Einzelprüfung von Testfällen und kontrollierten Datenflüssen waren auf `main` 62 offen. Erneut per GitHub API bestätigt: 16 CodeQL, 43 OSV, zwei zizmor und eine Scorecard-Meldung. Geprüfte Fehlalarme sind keine Codekorrekturen.

## Implementierte Änderungen

| Bereich | Schutz und Gegenprobe |
| --- | --- |
| Discord-Broker, Stripe, Highlight-Client | Geprüfte Request-Ziele, eingeschränkte Klartext-Ausnahmen für lokale Tests, abgeschaltete Redirects und Proxy-Übernahme; Gegenproben gegen fremde Ziele und Weiterleitungen. |
| Interner OAuth-Callback | Konfigurationsprüfung und erneute Zielprüfung direkt am HTTP-Aufruf. Ein manipulierter Callback erreicht den lokalen Testserver nicht. |
| Instagram | Validierte URL-Werte statt nachträglicher String-Zusammensetzung. Konto- und Medien-IDs werden als Pfadsegmente kodiert. Upload-Ziele müssen zum konfigurierten Ursprung passen. Transportfehler enthalten die Request-URL nicht. |
| Admin-SQL-Konsole | Autorisierter Admin-Pfad mit READ ONLY, 16-KiB-Eingabelimit, 200-Zeilen-Cursor sowie Statement- und Lock-Timeout. Keine Behauptung einer allgemeinen SQL-Sandbox. Gegenproben laufen mit einer isolierten PostgreSQL-Instanz. |
| Python-Detektor | Pillow 12.3.0, idna 3.15 und Pygments 2.20.0; Detektortests mit der isolierten aktualisierten Umgebung erneut ausgeführt. |
| Browser-Tests | Geprüfter Browserstart über Dateideskriptor; begrenzte Felder statt freier Objektübernahme im Social-Studio-Testserver; HTTP-Gegenprobe mit Prototyp-Schlüsseln und unbekannten Konten. |
| Roadmap | Lockfile-basierte Installation. Browser-Test wartet nach dem Schließen des Dialogs auf die abgeschlossene Wiederherstellung von Scrollen und Fokus. |
| Render-Helfer | Generische Datenbank-Verbindungsdiagnose statt Weitergabe des ursprünglichen Fehlers mit möglicher Verbindungsadresse. |

## Nachgewiesene Tests

| Prüfung | Ergebnis |
| --- | --- |
| Stripe-Modul | 8 bestanden |
| Gezielt gefilterte Rust-Sicherheitsprüfungen vor der letzten Ergänzung | 18 bestanden |
| Highlight-Modul | 77 bestanden |
| Discord-Broker | 30 bestanden |
| Instagram-Modul nach URL-Umbau | 18 bestanden |
| OAuth-Zielprüfungen nach Request-Umbau | 3 bestanden |
| Social-Studio-Browsertests einschließlich HTTP-Gegenprobe | 22 bestanden, 0 übersprungen |
| Roadmap-Browserprüfung | 15 lokale Prüfpunkte bestanden; GitHub-Lauf 35911575999 erfolgreich |
| Caddy-Vertragstest | 1 bestanden |
| Python-Detektor | 13 bestanden |
| Render-Helfer | cargo check erfolgreich |

Die Zahlen überlappen teilweise und dürfen nicht zu einer Zahl unabhängiger Tests addiert werden. Gefilterte Tests sind nicht als vollständiger Workspace-Test ausgewiesen.

## GitHub-Nachweise

Produktionscode-Stand der letzten Nachbesserung: `79f4e34b341ad7fc704874d7a37eeb0618d291cc`, davor `28f5491663bb28d3fd66f873ebea9a57ae1c3ba3`.

Deep-Scan-Lauf 35911650934: OSV, Trivy und Semgrep erfolgreich. Die entsprechenden Analysen auf diesem Commit enthalten jeweils null Ergebnisse. zizmor läuft erfolgreich, meldet aber weiterhin die beiden bekannten Workflow-Stellen #998 und #999. Ein erfolgreicher Job ist nicht gleichbedeutend mit null Meldungen. Scorecard schlägt auf dem Feature-Branch fehl, weil es den Default-Branch verlangt. Dieser Lauf ist keine vollständige grüne Sicherheitsabnahme.

CodeQL-Lauf 35911601625: Python, JavaScript/TypeScript und Actions erfolgreich. Die neue JavaScript-Meldung #1078 ist nach der begrenzten Objektübernahme nicht mehr offen. Die Rust-Analyse war zum Zeitpunkt dieses Zwischenstands noch nicht abgeschlossen; die fünf verbleibenden Rust-Meldungen werden nicht vorzeitig als geschlossen gezählt.

## Einzelentscheidung #1079

Die Meldung `js/http-to-file-access` betrifft den isolierten Social-Studio-Browser-Test. Das HTTP-Material wird als JSON-Berichtsinhalt in die feste Datei `browser-state.json` geschrieben. Der Zielpfad stammt aus `import.meta.dirname` und einem literalen relativen Pfad, nicht aus einem Request. Einzelentscheidung am 23.09.2026: Fehlalarm. Die Regel bleibt eingeschaltet; keine dateiweite Ausnahme.

## Offene Abnahme

Rust-SQLx-Lauf 35911575974: Offline-Build erfolgreich, Schema-Prüfung und Callback-Testschritt rot. Der Schema-Prüfung fehlen Tabellen aus dem privaten Brain-Schema. Der erfolgreiche Callback-Pfad verwendet außerdem `twitch_partners.raid_admin_enabled`, das im nachgebildeten Callback-Testschema fehlte. Die Spalte wurde in dieser Fortsetzung im Testschema ergänzt. Danach meldete das Callback-Testprogramm 14 bestandene Tests; der gesamte Paketaufruf endete anschließend beim getrennten Backfill-Testprogramm mit Exit 101. Dieses Ergebnis ist kein grüner Paketlauf.

Die Testergänzung ist zusätzlich gezielt mit `cargo test --locked -j 2 -p tb-bot --bin tb-bot raid_oauth_impl::callback_tests` geprüft: Exit 0, 14 bestanden, 0 fehlgeschlagen, 0 ignoriert. Der getrennte Paketfehler wird dadurch nicht als behoben ausgewiesen. Die neue GitHub-Abnahme des Teststands steht aus.

Die Dependabot-Workflow-Stellen bleiben beim parallelen PR #953. Schutzregeln und Scanner-Grenzen wurden nicht abgeschwächt. Ohne grüne Gesamtprüfung liegt keine Merge- oder Deploy-Abnahme vor.

## Fortsetzung 24.09.2026

Der Branch wurde mit dem aktuellen `origin/main` zusammengeführt. Der bereits gescannte Stand vor dieser Fortsetzung hatte auf `refs/heads/fix/code-scanning-20260923` nur noch die beiden Zizmor-Meldungen #998 und #999 offen; CodeQL, OSV und Scorecard hatten auf diesem Branch keine offenen Meldungen mehr.

Die beiden verbliebenen Dependabot-Befunde werden nicht per Scanner-Ausnahme behandelt. Wegen des verbindlichen PR-first-Testbetriebs ist die Merge-Automation vollständig pausiert: `pull_request_target`, der spoofbare `github.actor`-Check und sämtliche Schreibrechte des Workflows wurden entfernt. Dependabot darf weiter Update-PRs erzeugen; die regelmäßigen Security-Scans liegen in den getrennten Security-Workflows.

Funktional getesteter Code-Stand vor diesem rein dokumentarischen Nachweis-Update: `cd26f61ac5f72a63825ee026ded70051c2db8596`.

Lokale Nachweise auf diesem Stand:

| Prüfung | Ergebnis |
| --- | --- |
| `actionlint .github/workflows/dependabot-auto-merge.yml` | Exit 0 |
| `git diff --check origin/main...HEAD` | Exit 0 |
| Caddy-Vertragstest | 1 bestanden |
| Uplink-Browsertest | 1 bestanden |
| Highlight-Detektor Python | 13 bestanden |
| OSV-Scan `ops/highlight-detector/requirements.txt` | keine Findings |
| Discord-Broker Rust | 30 bestanden |
| Highlight Rust | 77 bestanden |
| Instagram Rust | 18 bestanden |
| Social-Media OAuth Rust | 12 bestanden |
| `render_clips` | `cargo check` erfolgreich |
| Stripe Rust | 8 bestanden |
| Dashboard Auth/OAuth Rust | 33 bestanden |
| Admin-Query Rust | 9 bestanden |

Der workspace-weite Formatter-Check ist weiterhin kein belastbarer Gate-Nachweis, weil er bereits vorhandene Formatabweichungen außerhalb dieses Security-Diffs meldet. Diese fremden Stellen wurden bewusst nicht umformatiert. Die funktionalen Tests oben liefen davon getrennt erfolgreich.

PR #957 bleibt gemäß PR-first-Testbetrieb offen. Kein Merge nach `main`, kein Auto-Merge, kein Deploy und kein Dienst-Neustart in dieser Fortsetzung. Der vollständige finale Head-SHA und die GitHub-Run-URLs werden nach dem Push zusätzlich direkt am PR dokumentiert.

### Zusätzlich geschlossener Dependabot-Fund

Beim Push meldete GitHub außerhalb von Code Scanning den offenen Dependabot-Alert #122 für `@humanfs/node` im Dashboard-Lockfile. ESLint 10.9.1 erlaubt `@humanfs/node ^0.16.6`; deshalb wurde nur die transitive Familie von 0.16.7 auf die gepatchte 0.16.8 aktualisiert. Getesteter Code-SHA: `70a7e6a21282493ad4576098dcf27a946cfefc58`.

Nachweise: `npm run build` erfolgreich; `npm audit --package-lock-only` meldet 0 bekannte Vulnerabilities. Der vollständige Dashboard-Testlauf hat 389 von 393 Tests bestanden. Die vier roten Tests sowie die aktuellen ESLint-Fehler liegen ausschließlich in Dateien, die gegenüber `origin/main` unverändert sind; sie sind damit bestehender Baseline-Zustand und nicht durch den Lockfile-Patch verursacht. Diese fremden Stellen wurden nicht mit repariert.
