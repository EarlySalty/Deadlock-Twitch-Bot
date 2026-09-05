# Plan: Wöchentliche Full-Repo-Security-Scans

status: aktiv
datum: 2026-09-05
klasse: mittel
research: RESEARCH.md

## Ziel

Fertig, wenn auf `main` die Security-Workflows nur montags (plus Handauslöser)
das ganze Repo scannen, Rust mit Clippy, cargo-audit und cargo-deny abgedeckt
ist, OSV/Semgrep/zizmor SARIF liefern, Dependabot Cargo sieht, und derselbe
Satz in rs-relay neu liegt.

## Nicht-Ziele

- Push-Trigger für Security-Jobs
- Findings in Rust-Crates in diesem Auftrag fixen
- cargo-geiger / Miri / Fuzzing

## Milestones

### M1 — Policy-Dateien und Dependabot
Änderungen: `rust/deny.toml`, `osv-scanner.toml`, `.semgrepignore`, `.github/dependabot.yml`
Erwarteter Zwischenzustand: `cargo deny check` im Workspace `rust/` läuft gegen die Policy; Dependabot kennt `/rust`.
Validierung: `cd rust && cargo deny check`
Stop-Regel: Check rot wegen unbekannter License oder Advisory ohne Begründung in audit.toml.

### M2 — Rust-Security-Workflow
Änderungen: `.github/workflows/rust-security.yml`
Erwarteter Zwischenzustand: dritter Job `cargo-deny`, Trigger unverändert schedule+dispatch.
Validierung: YAML enthält `cron`, kein `push:`/`pull_request:`; Job ruft `cargo deny check` in `rust/` auf.
Stop-Regel: Push-Trigger rutscht mit rein.

### M3 — Deep Scan, CodeQL, Trivy-Pin
Änderungen: `.github/workflows/security-deep-scan.yml`, `codeql.yml`, `secret-scanning.yml`
Erwarteter Zwischenzustand: OSV reusable workflow, Semgrep, zizmor; Python in CodeQL; Trivy auf Release-SHA `57a97c7e…` (v0.35.0); Detect-Job weg.
Validierung: kein `push:` in den drei Dateien; `language: python` in codeql.yml; keine `trivy-action@… # master`.
Stop-Regel: neuer Job hängt an Push oder braucht ein Secret.

### M4 — rs-relay analog
Änderungen: komplette `.github/`-Suite in rs-relay (CodeQL rust+actions, rust-security, secrets, deep-scan, dependabot cargo+actions).
Erwarteter Zwischenzustand: rs-relay hat dieselben wöchentlichen Scans, angepasst auf ein-Crate-Layout.
Validierung: gleiche Trigger-Regel; `cargo deny check` im rs-relay-Root.
Stop-Regel: Workflow kopiert TTB-Pfade (`rust/`) 1:1.

### M5 — Review, Merge, Push, Live-Check
Änderungen: keine fachlichen; Merge beider Repos nach `origin/main`, `workflow_dispatch` anstoßen.
Erwarteter Zwischenzustand: Workflows auf GitHub sichtbar, Handlauf gestartet.
Validierung: `gh workflow list` plus Dispatch; Actions-URL.
Stop-Regel: Gate-Deny, dann Fixrunde statt Force.

## Verlauf

- 2026-09-05: Research und Evidence geschrieben
- 2026-09-05: M1 verifiziert (cargo deny check advisories bans sources Exit 0)
- 2026-09-05: M2 und M3 implementiert (Workflows nur schedule plus dispatch)
