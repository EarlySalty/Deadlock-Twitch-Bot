# Research: Wöchentliche Full-Repo-Security-Scans

status: aktiv
datum: 2026-09-05
klasse: mittel

## Auftrag

Wöchentlicher Full-Repo-Security-Scan auf GitHub für Deadlock-Twitch-Bot, ohne
Push-Trigger, mit vollständigem Rust-Teil.

## Beobachtungen (belegt, Datei:Zeile)

- `.github/workflows/codeql.yml:6-9` — CodeQL läuft nur `schedule` (Montag 10:44 UTC) plus `workflow_dispatch`; Matrix: javascript-typescript, actions, rust; kein Python.
- `.github/workflows/codeql.yml:36-45` — Rust-Extraktor löst nur 42 Prozent der Aufrufziele auf; Clippy und cargo-audit sind die wirksame Rust-Prüfung.
- `.github/workflows/rust-security.yml:9-12` — Clippy (`-D warnings`) und `cargo audit` wöchentlich Montag 05:38 UTC, Arbeitsverzeichnis `rust/`.
- `.github/workflows/rust-security.yml:57-62` — cargo-audit bewusst nicht über `rustsec/audit-check`, weil die Action `rust/.cargo/audit.toml` nicht liest.
- `.github/workflows/secret-scanning.yml:4-7` — Gitleaks (volle Historie) und Trivy-Secrets wöchentlich, kein Push.
- `.github/workflows/security-deep-scan.yml:4-7` — Trivy-Filesystem HIGH/CRITICAL plus OSSF Scorecard wöchentlich.
- `.github/workflows/security-deep-scan.yml:17-41` — Job `detect-languages` schreibt Outputs, die kein Folgetrigger liest (toter Job).
- `.github/workflows/security-deep-scan.yml:53` — Trivy-Action ist auf `master` gepinnt (`ed142fd0…`), nicht auf ein Release.
- `.github/dependabot.yml` — github-actions und drei npm-Verzeichnisse täglich; kein `package-ecosystem: cargo`.
- `rust/.cargo/audit.toml:3-8` — einzige Advisory-Ausnahme `RUSTSEC-2023-0071` (sqlx-mysql, ungenutzt).
- `rust/Cargo.toml:1-32` — Workspace mit 25 Crates plus 3 Bins; Lockfile unter `rust/Cargo.lock`.
- Sprachen im Baum: 637 `.rs`, 335 TS/JS, 17 `.py`, kein Go-Produktcode.

## Hypothesen (unbelegt — nie als Fakt weiterreichen)

- cargo-deny auf dem aktuellen Lockfile braucht eine License-Allowlist plus dieselbe RUSTSEC-Ausnahme wie audit.toml; das ist nach dem ersten lokalen `cargo deny check` zu belegen.
- Semgrep `p/rust` plus `p/security-audit` liefert auf diesem Workspace ERROR-Funde; der Job soll nur bei Severity ERROR rot werden, damit WARNING-Lärm den Wochenlauf nicht dauerhaft unbrauchbar macht.
- zizmor auf den bestehenden, SHA-gepinnten Workflows bleibt bei `min-severity: medium` grün, sobald Trivy nicht mehr auf `master` zeigt.

## Wahrscheinlich zu ändernde Dateien

- `.github/workflows/rust-security.yml` — Job cargo-deny.
- `.github/workflows/security-deep-scan.yml` — OSV, Semgrep, zizmor; toten Detect-Job ersetzen; Trivy pinnen.
- `.github/workflows/codeql.yml` — Python in die Matrix.
- `.github/dependabot.yml` — cargo `/rust` wöchentlich.
- `rust/deny.toml` — Policy für cargo-deny.
- `osv-scanner.toml` — gleiche Advisory-Ausnahme.
- `.semgrepignore` — target, node_modules, dist.

## Risiken / Seiteneffekte

- Actions-Minuten: neue Jobs laufen nur montags, gestaffelt wie bisher. Semgrep und cargo-deny sind billig (kein Full-Compile); Clippy bleibt der teure Job.
- Ein dauerhaft roter Wochenjob wird ignoriert. Deshalb ERROR-Schwelle bei Semgrep, HIGH/CRITICAL bei Trivy, und License-Allowlist so, dass der aktuelle Baum durchkommt.
- OSV und cargo-audit überlappen bei RustSec; das ist Absicht (OSV sieht zusätzlich npm).

## Offene Fragen

- keine produktiven; License-Menge folgt aus lokalem `cargo deny check`.
