# Contract: Wöchentliche Full-Repo-Security-Scans

status: aktiv
datum: 2026-09-05
klasse: mittel
repo: Deadlock-Twitch-Bot

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu. Wer ein REQ oder INV ändern will, schreibt ein
Amendment mit Begründung; Produkt-, API- oder Datenänderungen entscheidet der User.

## Ziel

Jeden Montag läuft auf GitHub ein vollständiger Security-Scan über das ganze
Repository (Rust, Frontend, Python, Actions, Secrets, Abhängigkeiten), ohne dass
ein Push den Lauf auslöst. Die bestehenden wöchentlichen Jobs bleiben, die
Lücken im Rust- und Supply-Chain-Teil werden geschlossen.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01: Alle Security-Workflows unter `.github/workflows/` (CodeQL, Rust Security, Secret Scanning, Deep Scan) starten nur per `schedule` (cron wöchentlich, Montag) und `workflow_dispatch`, nicht per `push` und nicht per `pull_request`.
- REQ-02: Der Rust-Scan prüft den gesamten Workspace unter `rust/` mit Clippy, cargo-audit (RustSec) und cargo-deny (Advisories, Lizenzen, Bans, Quellen).
- REQ-03: Ein wöchentlicher Full-Repo-Scan deckt zusätzlich OSV (Lockfiles inklusive Cargo und npm), Semgrep (Rust plus vorhandene JS/TS/Python/Actions) und zizmor (GitHub-Actions-Workflows) ab und lädt SARIF in die Security-Ansicht.
- REQ-04: CodeQL analysiert weiterhin JavaScript/TypeScript, GitHub Actions und Rust und zusätzlich Python (vorhandene Skripte unter `tools/`, `ops/`, `tests/`).
- REQ-05: Dependabot überwacht das Cargo-Ökosystem unter `/rust` mindestens wöchentlich.
- REQ-06: Trivy-Action-Pins zeigen auf eine Release-SHA, nicht auf `master`.
- REQ-07: `workflow_dispatch` bleibt auf jedem Security-Workflow, damit ein Lauf ohne Warten auf Montag auslösbar ist.

## Invarianten (darf sich nicht ändern)

- INV-01: `manifest-scope.yml`, `rust-sqlx-check.yml` und `dependabot-auto-merge.yml` behalten ihre bestehenden Trigger (PR/Push bzw. Dependabot); das sind keine Security-Full-Scans.
- INV-02: Frontend-CI bleibt wöchentlich, ohne Push-Trigger.
- INV-03: Bestehende Gitleaks-Allowlists, `.trivyignore.yaml`, `rust/.cargo/audit.toml` und `.gitguardian.yaml` bleiben inhaltlich gültig; neue Ausnahmen nur mit Begründung.
- INV-04: Kein Anwendungs- oder Bot-Code, keine Migration, keine Caddy-Änderung, kein Secret im Klartext.
- INV-05: Bestehende Tests werden nicht gelöscht oder abgeschwächt.
- INV-06: Keine Code-Kommentare; bestehende Kommentare in angefassten Dateien dürfen entfallen.

## Nicht-Ziele

- Scan bei jedem Push oder PR (Actions-Budget).
- cargo-geiger, Miri, cargo-fuzz, cargo-vet (zu teuer oder menschabhängig).
- GitHub-Org-Einstellungen (native Secret Scanning, Push Protection) umstellen.
- Findings in Anwendungs-Code in diesem Auftrag beheben; der Scan soll sie sichtbar machen.
- Neues Secret oder neue GitHub-App.

## Erlaubter Änderungsbereich

- .github/workflows/codeql.yml
- .github/workflows/rust-security.yml
- .github/workflows/secret-scanning.yml
- .github/workflows/security-deep-scan.yml
- .github/dependabot.yml
- .github/codeql/codeql-config.yml
- rust/deny.toml
- rust/.cargo/audit.toml
- osv-scanner.toml
- .semgrepignore
- .tasks/2026-09-05-weekly-security-scans/

## Verbotene Änderungen

- rust/crates/
- rust/bin/
- rust/migrations/
- bot/
- website/
- ops/
- Caddyfile und Live-Dienste

## Offene Produktfragen

- keine

## Amendments
