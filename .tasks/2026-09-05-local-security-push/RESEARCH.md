# Research: Lokaler Security-Push

status: aktiv
datum: 2026-09-05
klasse: mittel

## Auftrag

Security-Scans bei jedem lokalen Push, Remote nur wöchentlich, ohne Lizenz.

## Beobachtungen (belegt, Datei:Zeile)

- `.github/workflows/rust-security.yml:9-12` — Remote nur `schedule` plus `workflow_dispatch`.
- `.github/workflows/secret-scanning.yml` — Gitleaks per Action (öffentlich) bzw. in rs-relay per CLI.
- `.github/workflows/rust-sqlx-check.yml:3-11` — `push` und `pull_request` auf `rust/**`; Dependabot-Cargo-PRs lösen beides aus und fressen Minuten.
- Auf dieser Maschine liegen CLI-Tools ohne Lizenz: gitleaks 8.30.1, trivy 0.73.0, semgrep 1.173.0, osv-scanner 2.5.0, cargo-audit 0.22.2, cargo-deny 0.20.2.
- Kein `core.hooksPath`, kein `.githooks/` im Repo.

## Hypothesen

- Clippy `-D warnings` auf jedem Push blockt, sobald main nicht clippy-sauber ist (Remote-Job war am 31.08. rot). Deshalb Clippy nur bei Rust-Diff und im Default nicht push-blockend, `--strict` wie Remote.
- Trivy HIGH kann Bestandstreffer haben; Default deshalb nicht push-blockend, Secrets und RustSec schon.

## Wahrscheinlich zu ändernde Dateien

- `scripts/security-scan-local.sh`, `.githooks/pre-push`, `scripts/install-git-hooks.sh`, `rust-sqlx-check.yml`

## Risiken / Seiteneffekte

- Pre-Push braucht Netz für Advisory-DBs (audit, deny, osv, trivy, semgrep).
- Worktrees erben `core.hooksPath` vom Haupt-Repo.

## Offene Fragen

- keine
