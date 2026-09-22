# Deterministische PR-Prüfungen

## Stand und Geltungsbereich

Implementierungsstand vom 22. September 2026, ausgehend von `83f6753d44867529fb2629d0fab30c7f468b7e3c` auf `main`. Dieser Umbau ist zunächst ein Validierungs-PR, keine Freigabe des bestehenden Produktcodes. Insbesondere sind vorhandene Format-, Lint- und Testfehler nicht ausgenommen oder als Erfolg umgedeutet worden.

`Required PR CI` startet bei jedem Pull Request, einschließlich Dokumentations- und Draft-PRs, bei Merge-Queue-Ereignissen sowie manuell. Es gibt keinen Workflow-Level-Pfadfilter. Der finale Statuscheck heißt exakt **`Required PR Gate`**. Alle Entscheidungen werden durch Programme, Tests und Scanner getroffen; keine LLM-API, kein Copilot-Review und kein Modellurteil werden aufgerufen.

## Prüfungen

| Bereich | Prüfung | Blockierende Policy |
| --- | --- | --- |
| Scope/Verknüpfung | `.github/ci/pr_gate.rs`, rustfmt, rustc, 18 Regressionstests | Unbekannte Resultate, fehlender Scope, Fehler und Abbrüche blockieren. |
| Manifest-Scope | Bestehender Python-Abgleich plus 9 Tests | Alle npm-Projekte müssen in Dependabot und der Frontend-Matrix vorkommen. |
| Rust | `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, Offline-Build | Jeder Format-, Compile- oder Clippy-Fehler blockiert. |
| Rust-Tests/DB | Bestehende Auth-, Broker-, Uplink-, Titel-, Statistik-, Onboarding- und Identitätstests; Workspace-Library/Binary-Tests; `tb-db` Fresh-Schema/hermetic | Fehlgeschlagene Tests und SQLx-Cache-/Migrationsprüfungen blockieren. |
| Frontends | Bestehendes `npm ci`, vorhandenes lint/test-Script, `npm run build` inklusive `tsc -b` | Alle vorhandenen Commands bleiben blockierend. Website/Admin haben kein lint-Script; es wird keines erfunden. |
| Secrets | Gitleaks mit bestehender Konfiguration, volle Checkout-Historie verfügbar | Findings beziehungsweise Scannerfehler blockieren. Keine Schreibrechte oder Repository-Secrets im PR-Job. |
| SAST | Semgrep OSS: allgemeiner Scan plus separater nativer Rust-Scan | Allgemein ERROR, für `p/rust` WARNING und ERROR; beide mit `--error --strict`. Scanfehler werden nicht zu grün. |
| Dependencies | Cargo-Audit, Cargo-Deny, npm audit, Trivy Filesystem | Nicht ausgenommene RustSec-Vulnerabilities, verbotene Quellen und HIGH/CRITICAL bei npm/Trivy blockieren. |
| Actions | actionlint 1.7.12, zizmor 1.30.0 | Syntax-/Shellcheck-Fehler und HIGH-Findings blockieren; zizmor ohne Abhängigkeit von Code-Scanning-Uploads. |

Rust-Änderungen starten die Rust-Prüfungen, Änderungen an einem Frontend starten konservativ alle drei Frontend-Projekte. CI-/Policy-Änderungen sowie unbekannte Quell-/Konfigurationspfade starten beide Bereiche. Nur reine Markdown-Dokumentation außerhalb `.github/` und `LICENSE` dürfen Compile-Jobs auslassen. Manifest- und Security-Prüfungen laufen auch dann. Git-Diffs sind NUL-getrennt, ohne Rename-Zusammenfassung und ohne die Begrenzung einer API-Dateiliste; beide Seiten eines Umzugs werden berücksichtigt.

Der Abschluss-Job verwendet `if: always()` und prüft jedes `needs`-Resultat. Ein absichtlich irrelevanter Compile-Job darf `skipped` sein; ein relevanter oder allgemeiner Job nicht. Auch ein Fehler in einem freiwillig doch ausgeführten, eigentlich irrelevanten Job bleibt rot. Fehlende Tools, Infrastruktur oder Scan-Features sind keine erlaubten Skips.

## Infrastruktur und Grenzen der Abdeckung

SQLx nutzt die vorhandene Offline-Konfiguration. Der Schema-Job provisioniert einen wegwerfbaren TimescaleDB/PostgreSQL-16-Service, ausschließlich auf dem Loopback des GitHub-Runners. `DATABASE_URL`, `TB_TEST_DATABASE_URL` und `TEST_DATABASE_URL` zeigen auf diesen Service. Die bestehenden isolierten Auth-Tests nutzen zusätzlich PostgreSQL-Hostprogramme und private Unixsockets. Es werden keine Produktions-DSNs und keine Bot-/LLM-Schlüssel weitergereicht.

Die bestehende Git-Abhängigkeit auf `Deadlock-Brain` ist öffentlich und auf einen vollständigen Commit gepinnt. Cargo-Deny erlaubt genau diese Git-Quelle und verlangt `rev`; beliebige zusätzliche Git-/Registry-Quellen bleiben verboten.

Nicht jede vorhandene Integrationstestdatei ist schon Teil der ausgewählten Suite. Insbesondere Browser-/visuelle Tests, opt-in Fuzzing, produktionsgebundene Tests und featuregeschaltete Eval-Suites sind hier kein Vollabdeckungsversprechen. Ein erfolgreich gebautes Testprogramm beweist nicht, dass alle seine optionalen/ignorierten Tests ausgeführt wurden. Die weiteren Python-Testbereiche außerhalb des bestehenden Manifest-Checks werden durch SAST/Dependency-Scanning erfasst, aber in dieser ersten Ausbaustufe noch nicht vollständig funktional getestet.

Vulnerability-Feeds und Semgrep-Registry-Regeln können sich unabhängig vom Commit ändern. Das ist gewollte neue Sicherheitsinformation, keine LLM-Entscheidung. Geänderte Findings müssen überprüft werden, nicht pauschal unterdrückt.

## Bewusste Ausnahmen und nicht blockierende Ergebnisse

- `RUSTSEC-2023-0071`: bestehende enge Ausnahme aus `rust/.cargo/audit.toml`, in `rust/deny.toml` gespiegelt. Begründung: rsa hängt am nicht aktivierten SQLx-MySQL-Pfad, gebaut wird PostgreSQL. Bei Aktivierung von MySQL neu bewerten; Wiedervorlage spätestens 22. Dezember 2026.
- Die bisherigen Gitleaks- und Trivy-Ausnahmen bleiben erhalten. Dazu gehören öffentliche Discord-IDs, dokumentierte Testfixtures und historische Pfad-Ausnahmen. Die vorhandenen dateiweiten Gitleaks-Ausnahmen sind keine Aussage, dass dort niemals ein neues Secret auftauchen kann; ihre weitere Verengung bleibt ein Prüfpunkt. Drei neue Gitleaks-Ausnahmen verbinden Pfad UND exakten Fixture-Wert: sechs feste ActionInput-Test-UUIDs, die WebSocket-Beispielnonce und zwei nachweislich identisch kopierte Demo-Chiffrate in `partner-clean/Security.tsx`. Neue Schlüssel in diesen Dateien bleiben scanpflichtig; Wiedervorlage am 22. Dezember 2026.
- Cargo-Deny behandelt parallele Dependency-Versionen und Versions-Wildcards weiterhin als Wartungswarnungen. Die vorhandenen internen Workspace-/Git-Pfadverweise besitzen teilweise keine Registry-Versionsangabe. Das hebt keine RustSec-/Sources-Prüfung auf. Yanked-Versionen werden separat als Warnung gemeldet; der lokale Lauf nennt `chacha20` und `spin` und verlangt Nachprüfung, keine automatische Versionsänderung.
- npm MODERATE/LOW, Trivy unter HIGH, allgemeines Semgrep unter ERROR und Rust-Semgrep unter WARNING liegen unter der dokumentierten ersten Merge-Schwelle. Die INFO-Regeln des Rust-Pakets inventarisieren unter anderem CLI-Argumente, temporäre Verzeichnisse und `unsafe`-Blöcke; nicht jede solche Stelle ist eine Schwachstelle. Die bisherigen allgemeinen ERROR-Filter hatten keine nativen Rust-Regeln ausgeführt; der zusätzliche WARNING/ERROR-Lauf schließt diese konkrete Lücke. zizmor läuft mit HIGH-Schwelle; ein grüner Lauf ist kein Beweis für null niedrigere Findings.
- PR-SARIF-Artefakt-Uploads dürfen fehlschlagen, damit ein Reporting-Ausfall nicht mit einem Scannerbefund verwechselt wird. Der Scanner davor bleibt blockierend. Es werden keine PR-Schreibrechte für Code Scanning benötigt.
- Bestehende wöchentliche CodeQL-/Deep-Security-/Secret-/Frontend-/Rust-Läufe bleiben erhalten. Rust-CodeQLs dokumentierte Extraktionslücke bleibt eine Grenze, kein Sicherheitsnachweis. Der bisherige scheduled zizmor-Advanced-Security-Lauf und der Deep-Summary-Job sind Reporting; der zusätzliche PR-zizmor-Lauf blockiert unabhängig davon. Scorecard ist eine zusätzliche Bestandsaufnahme, kein definierter PR-Score-Grenzwert.
- Einzige neue zizmor-Ausnahme: `dangerous-triggers` unmittelbar am `pull_request_target` des Dependabot-Metadaten-Workflows. Dieser prüft die unveränderliche PR-Autor-ID, denselben Head-Repo, Default-Branch, erlaubten Update-Typ und eine tatsächlich aktive Required-Check-Regel. Er checkt keinerlei PR-Code aus, lädt keine PR-Artefakte und führt ausschließlich gepinnte Actions sowie festes CLI-Kommando aus. Er braucht Schreibrechte allein zum Aktivieren des nativen Auto-Merge. Wiedervorlage am 22. Dezember 2026 und bei jeder neuen Aktion/Berechtigung.

## Dependabot und Merge-Schutz

Bestehende Dependabot-Ecosystems bleiben erhalten: Cargo `/rust`, drei npm-Projekte und GitHub Actions. Auto-Merge ist auf exakt klassifizierte Patch-/Minor-Updates ohne Maintainer-Wechsel beschränkt. Major-, unbekannte und GitHub-Actions-Updates werden nicht automatisch freigegeben. Security-Updates umgehen weder Versionspolicy noch Required Checks.

Der alte unmittelbare Merge-API-Aufruf und die manuelle Merge-all-Reconciliation sind entfernt. Native Auto-Merge wird erst eingeschaltet, wenn die effektiven Branch-Regeln **`Required PR Gate`**, ausschließlich vom GitHub-Actions-App-Kontext `15368`, mit strikter Aktualitätsprüfung verlangen. Ohne diese Regel bleibt Auto-Merge aus. Der erwartete Head-SHA wird beim Aktivieren geprüft. Kein Admin-Bypass, kein Checkout mit erhöhten Rechten.

**Ruleset-Aktivierung erst nach erfolgreicher Validierung:** Unter Repository → Settings → Rules → Rulesets das aktive Default-Branch-Ruleset `14377032` ergänzen: Pull Request verlangen und Required Status Check `Required PR Gate` mit Quelle GitHub Actions sowie aktueller Branch-Basis erzwingen. Vorhandene Lösch-/Force-Push-Sperren und Code Quality erhalten. Erst nach Nachweis eines vollständigen grünen Laufs und roter Gegenproben `copilot_code_review` entfernen. Das bestehende Ruleset wird nicht vorzeitig abgeschwächt. Der vorliegende Implementierungsstand hat es noch nicht verändert.

## Lokale Nachweise und offene Abnahme

- Gate-TDD: zunächst 12 rote/6 grüne Tests mit nicht implementierter Logik, danach 18/18 grün; `rustc -D warnings` und rustfmt der neuen Datei grün.
- Vorhandener Manifest-Scope und seine 9 Tests grün.
- actionlint über alle Workflows grün; zizmor offline bei HIGH grün, mit der oben dokumentierten einzelnen neuen Trigger-Ausnahme.
- `cargo metadata --locked` erfolgreich, 474 Pakete. `cargo audit` und `cargo deny check advisories bans sources` erfolgreich gemäß dokumentierter Policy.
- Gitleaks 8.30.1: saubere Gegenprobe Exit 0, künstlicher PAT Exit 1. Zusätzlich drei künstliche `generic-api-key`-Funde in genau den neu ausgenommenen Dateien: drei Treffer und Exit 1, also keine dateiweite Freigabe. Aktueller Dateibaum nach den begrenzten Fixture-Ausnahmen: null Findings, Exit 0. Die PR-Action war im ersten GitHub-Lauf ebenfalls erfolgreich.
- `npm audit --audit-level=high` in allen drei Projekten Exit 0; im Haupt-Dashboard bleibt ein MODERATE-Fund sichtbar.
- `npm ci` in allen drei Projekten erfolgreich. Website-Tests 48/48, Admin-Tests 10/10. Haupt-Dashboard: Hauptsuite 386/391 erfolgreich, fünf Fehler; Kalender-Vorsuite separat im Log.
- Haupt-Dashboard-ESLint: acht Fehler und sieben Warnungen. `cargo fmt --all --check`: vorhandener Formatierungsbedarf in 264 Dateien. Diese Befunde sind weiterhin blockierend und wurden nicht durch breite Ausnahmen neutralisiert.
- Nach Korrektur der Semgrep-Parsing-Probleme, zusätzlich mit der exakt in CI gepinnten Semgrep-Version 1.176.1 geprüft: allgemeiner Scan über 3003 Dateien und 106 tatsächlich ausgeführte Regeln Exit 0; nativer Rust-Scan über 744 Dateien und fünf WARNING/ERROR-Regeln Exit 0. Beide SARIF-Berichte enthalten null Parsing-/Timeout-Benachrichtigungen. Eine saubere Reqwest-Client-Gegenprobe ergibt Exit 0, eine künstlich abgeschaltete TLS-Zertifikatsprüfung Exit 1 mit dem erwarteten `reqwest-accept-invalid`-Finding. Die Probe wird nur gescannt, nie ausgeführt.
- 13 minimale Ampersand-Escapes in acht TSX-Dateien ermöglichen vollständiges Semgrep-Parsing; nur JSX-Text wurde verändert, nicht JS-Strings oder Operatoren. Die sichtbaren Texte bleiben identisch. Workflow-Statuswerte werden im Deep-Summary über Umgebungsvariablen statt direkt im Shell-Code eingesetzt.

## Tatsächlicher erster GitHub-PR-Lauf

Draft-PR **#953**, Commit `489a4b3d85cb8a7e1e99f197bab8355069948002`, Lauf **35758069681**. GitHub testete den Merge mit dem inzwischen weiterentwickelten `main`-Stand `e25b44b9be2f06caf10aedb7ac8778f9ea57ca94`, nicht nur die ursprüngliche lokale Basis. Der finale **Required PR Gate erschien und scheiterte korrekt**.

Erfolgreich waren Gate-/Scope-Tests, Manifest, Gitleaks, Trivy, Cargo-Audit, Cargo-Deny, kompletter Rust-Offline-Build sowie die Website- und Admin-Frontend-Jobs. Rot waren Formatierung, Clippy (`result_unit_err` in `signup_denylist.rs:71`), Dashboard-Lint, sechs OAuth-Callback-Tests mit HTTP 500 statt 200 und der SQLx-Schema-Check. Letzterem fehlen die vom gepinnten `dbrain-builds` benötigten `brain.*`-Tabellen, unter anderem `brain.hero_catalog`; nachfolgende Workspace-/DB-Tests wurden deshalb noch nicht ausgeführt. Vor einer Freigabe ist das echte versionierte Brain-Schema korrekt zu provisionieren, nicht durch erfundene Tabellen oder übersprungene Checks zu ersetzen.

Zwei zusätzliche CI-Konfigurationsfehler wurden aus diesem Lauf korrigiert: actionlint 1.7.12 wird nicht von der verwendeten Rust-Installer-Action unterstützt und wird nun aus dem offiziellen Release mit festem SHA256 installiert; Semgrep blieb bei unvollständigem JSX-/Workflow-Parsing trotz null Findings korrekt rot, die Parsing-Ursachen wurden ohne Scan-Ausnahmen beseitigt. Die aktualisierte Fassung benötigt ihren eigenen GitHub-Nachweis.

Der lokale normale Push-Hook lässt OSV-Hinweise sowie optionale Deep-Scans ausdrücklich nicht als Merge-Freigabe gelten. Im ersten Push meldete OSV Python-Dependency-Hinweise; sie sind gesondert zu triagieren. Vollständig ignorierte/scannerseitig bekannte Ausnahmen wurden nicht als vollständig abgesicherte Codebereiche umgedeutet.

Vor Abschluss sind die roten Produkt-/Test-Baselines zu bereinigen, sämtliche PR-Jobs tatsächlich auszuführen, Scanner-Gegenproben zu sichern und der finale Gate sowie das Ruleset zu verifizieren. Erst danach ist dieses Repository fertig; die weiteren sechs Repositories werden nicht parallel als angeblich abgesichert geführt.
