# Contract: Security-Scans bei jedem lokalen Push, Remote nur wöchentlich

status: aktiv
datum: 2026-09-05
klasse: mittel
repo: Deadlock-Twitch-Bot

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu.

## Ziel

Jeder lokale `git push` in diesem Repo läuft denselben Security-Satz wie die
wöchentlichen GitHub-Jobs (CLI, ohne Lizenz), ohne GitHub-Actions-Minuten zu
verbrauchen. Remote bleiben die Security-Workflows wöchentlich.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01: `scripts/security-scan-local.sh` führt lokal Gitleaks, cargo-audit, cargo-deny, Trivy, OSV-Scanner, Semgrep und bei Rust-Änderungen Clippy aus. Keine GitHub-Action, kein Lizenz-Token.
- REQ-02: `.githooks/pre-push` ruft das Skript auf. Nach `git config core.hooksPath .githooks` blockt ein Fund in Gitleaks, cargo-audit oder cargo-deny den Push.
- REQ-03: Die Security-Workflows unter `.github/workflows/` behalten `schedule` plus `workflow_dispatch` und bekommen keinen `push:`-Trigger.
- REQ-04: `rust-sqlx-check.yml` läuft nicht mehr bei `push`, nur noch bei `pull_request` und `workflow_dispatch`.
- REQ-05: `scripts/install-git-hooks.sh` setzt `core.hooksPath` auf `.githooks`.

## Invarianten (darf sich nicht ändern)

- INV-01: Kein Anwendungs- oder Bot-Code, keine Migration.
- INV-02: Bestehende Tests werden nicht gelöscht oder abgeschwächt.
- INV-03: Keine Code-Kommentare; bestehende Kommentare in angefassten Dateien dürfen entfallen.
- INV-04: Kein Secret im Klartext.

## Nicht-Ziele

- `act` als Replay der GitHub-Runner.
- CodeQL und Scorecard lokal (brauchen GitHub).
- SonarQube Cloud umbauen.

## Erlaubter Änderungsbereich

- scripts/security-scan-local.sh
- scripts/install-git-hooks.sh
- .githooks/pre-push
- .github/workflows/rust-sqlx-check.yml
- .tasks/2026-09-05-local-security-push/

## Verbotene Änderungen

- rust/crates/
- rust/bin/
- bot/
- website/
- .github/workflows/codeql.yml
- .github/workflows/rust-security.yml
- .github/workflows/secret-scanning.yml
- .github/workflows/security-deep-scan.yml

## Offene Produktfragen

- keine

## Amendments
