# Evidence: Wöchentliche Full-Repo-Security-Scans

status: aktiv
datum: 2026-09-05
contract: CONTRACT.md

Repo-Aufklärung vor dem ersten Edit. Jede Zeile ist eine Fundstelle `pfad:zeile`,
keine Vermutung. Der Hook (R11) gibt Quellcode-Edits erst frei, wenn hier
mindestens 3 Fundstellen stehen. Drei ist die Untergrenze, nicht das Ziel.

## Analoge Implementierungen (wie löst das Repo so etwas schon?)

- `.github/workflows/codeql.yml:6-9` — wöchentlicher Full-Scan statt Push, `workflow_dispatch` als Handauslöser; Muster für alle Security-Jobs.
- `.github/workflows/rust-security.yml:21-62` — Clippy über den ganzen Workspace plus cargo-audit mit `working-directory: rust` und `taiki-e/install-action`; cargo-deny hängt an denselben Install-Weg.
- `.github/workflows/security-deep-scan.yml:43-69` — Trivy fs mit SARIF-Upload in die Security-Ansicht; OSV und Semgrep folgen demselben Upload.
- `.github/workflows/secret-scanning.yml:13-38` — Gitleaks mit `fetch-depth: 0` und `.gitleaks.toml`.
- `.github/dependabot.yml:1-16` — github-actions täglich, CodeQL-Action gruppiert; Cargo-Eintrag analog mit anderem Intervall.

## Bestehende Abstraktionen (werden wiederverwendet, nicht nachgebaut)

- `rust/.cargo/audit.toml:3-8` — kanonische RUSTSEC-Ausnahmeliste; deny.toml und osv-scanner.toml übernehmen dieselbe ID.
- `.github/codeql/codeql-config.yml:3-4` — `queries: security-extended` plus Sprach-Packs inklusive `codeql/rust-queries`.
- `.trivyignore.yaml:1-8` — begründete Trivy-Ausnahme; nicht anfassen.
- `.gitleaks.toml:1-7` — Default-Regeln plus geprüfte Allowlists; nicht anfassen.
- Action-Pins: `actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1` (v7.0.1), `github/codeql-action@cdf488f595d80d6e07e03d4674febd5ab45fa938` (v4), `taiki-e/install-action@0758d235715de2f3551eacc980d9ae8fce9342c3` (v2.87.3).

## Relevante Tests (laufen vorher, laufen nachher)

- `tests/test_check_manifest_scope.py` — prüft Dependabot-Verzeichnisse gegen echte Projekte; neuer Cargo-Eintrag `/rust` muss zum Workspace passen.
- `.github/workflows/manifest-scope.yml` — bleibt Push/PR, unverändert.
- Kein bestehender Test deckt die Security-Workflows selbst ab; Validierung ist YAML-Struktur plus `cargo deny check` lokal.

## Öffentliche Schnittstellen und Verträge (dürfen nicht brechen)

- GitHub Security-Tab (Code Scanning SARIF): bestehende Kategorien `trivy-filesystem`, `trivy-secrets`, `scorecard`, `/language:*` bleiben; neu: `osv-scanner`, `semgrep`, `zizmor`.
- `rust/Cargo.toml:1-32` — Workspace-Mitglieder unverändert.
- Keine HTTP-Route, kein Schema.

## Änderungsfläche (welche Dateien voraussichtlich angefasst werden)

- `.github/workflows/codeql.yml` — Python in die Matrix.
- `.github/workflows/rust-security.yml` — Job cargo-deny.
- `.github/workflows/security-deep-scan.yml` — OSV, Semgrep, zizmor, Trivy-Pin, toten Detect-Job entfernen.
- `.github/workflows/secret-scanning.yml` — Trivy-Pin.
- `.github/dependabot.yml` — cargo `/rust`.
- `rust/deny.toml` — cargo-deny-Policy.
- `osv-scanner.toml` — Ignore für RUSTSEC-2023-0071.
- `.semgrepignore` — Build-Artefakte.

## Offene Architekturfrage

- keine
