# Plan: Lokaler Security-Push

status: aktiv
datum: 2026-09-05
klasse: mittel
research: RESEARCH.md

## Ziel

Fertig, wenn `git push` lokal den Security-Satz fährt und Remote-Security weiter nur montags läuft.

## Nicht-Ziele

- act, CodeQL lokal, Sonar

## Milestones

### M1 — Skript und Hook
Änderungen: scripts/security-scan-local.sh, .githooks/pre-push, scripts/install-git-hooks.sh
Validierung: bash -n auf die Dateien; Skript startet und nennt fehlende Tools oder läuft durch
Stop-Regel: Skript braucht ein Secret oder eine Action

### M2 — SQLx-Push raus
Änderungen: rust-sqlx-check.yml
Validierung: Dateikopf hat pull_request und workflow_dispatch, kein push
Stop-Regel: Security-Workflows bekommen einen Push-Trigger

### M3 — Hook aktiv, Merge
Validierung: git config core.hooksPath in beiden Repos ist .githooks; Merge nach main
Stop-Regel: Gate-Deny

## Verlauf

- 2026-09-05: Research und Evidence
