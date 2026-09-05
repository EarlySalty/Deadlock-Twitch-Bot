# Evidence: Lokaler Security-Push

status: aktiv
datum: 2026-09-05
contract: CONTRACT.md

## Analoge Implementierungen

- `.github/workflows/rust-security.yml:39-43` — Clippy `SQLX_OFFLINE=true cargo clippy --workspace --all-targets --locked -- -D warnings` in `rust/`.
- `.github/workflows/rust-security.yml:60-62` — `cargo audit` in `rust/`.
- `.github/workflows/secret-scanning.yml:24-31` — `git ls-files` gegen Secret-Dateinamen, dann Gitleaks.
- `scripts/check_manifest_scope.py:1-21` — bestehendes CLI-Skript ohne Fremdbibliothek, ausführbar lokal und in CI.

## Bestehende Abstraktionen

- `rust/.cargo/audit.toml` — RUSTSEC-Ausnahmen, cargo-audit liest sie im Arbeitsverzeichnis `rust/`.
- `.gitleaks.toml` — lokale Gitleaks-Config.
- `.semgrepignore` — Semgrep-Ausschlüsse.
- `osv-scanner.toml` — OSV-Ignore.

## Relevante Tests

- `tests/test_check_manifest_scope.py` — unberührt.
- Validierung: Skript einmal lokal laufen lassen, Hook mit `core.hooksPath` gesetzt.

## Öffentliche Schnittstellen

- Keine HTTP-Route. GitHub-Workflow-Trigger von rust-sqlx-check ändert sich (kein Push).

## Änderungsfläche

- scripts/security-scan-local.sh
- scripts/install-git-hooks.sh
- .githooks/pre-push
- .github/workflows/rust-sqlx-check.yml

## Offene Architekturfrage

- keine
