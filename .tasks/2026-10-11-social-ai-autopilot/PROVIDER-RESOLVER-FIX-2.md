# Provider M0 rebase and resolver cause repair, round 2

## Goal and preserved contract

Complete the bounded provider handover for M1, not a resolver redesign. Read COMMON.md and AUFTRAG.md in `/home/nathanael/.worktrees/tb-social-coord/.tasks/2026-10-11-social-ai-autopilot/`, PROVIDER-RESOLVER-CHECK.md here, and the actual relevant source. The genuine orchestrator now reports M0 live; parent fetched and verified main `33656f2c2d2154a8ce888ed1a39180935209ea6e`. Rebase this owned, clean, unpushed branch onto that exact ref now. Preserve the independently accepted native provider/accounting/consent implementation at `9e344cfcba96cf556764b8bca82dbb9e7dc7e040`, including incomplete historical replay rejection, higher known debit, unknown holds, convergent dashboard-writer/bot-reader grants, YAML model/rates and the shared $2 cap. Resolve only actual rebase conflicts minimally.

## Ownership and current state

Owned worktree `/home/nathanael/.worktrees/tb-social-ai-provider`, branch `feat/social-ai-provider-20261011`, last parent-observed clean HEAD `2a290d201cd154a80cce1f1283bdd0a1898a9ab4`. All earlier workers/reviewers are complete and must not be resumed. You are the only writer. After rebase, source edits are restricted to daily_model_resolver.rs tests and directly necessary synthetic fixture support. No resolver/provider/accounting/model/pricing/budget/consent production changes or new dependencies. Parent owns feature backup and integration. No force push or hook bypass.

## Actual rejected patch and diagnostics

Original parallel failure: failed_refresh_then_regular_retry_records_only_success recorded 0 requests rather than 2. Existing run_at returns Ok(()) for a held file lock or an already verified day; neither is a demonstrated production defect. The previous worker added a 128-yield run_due helper and an explicit held-lock assertion, reported three green parallel runs, but did not identify the unexpected holder. Its independent reviewer BLOCKED: 53 passed, 2 failed, 1 ignored; retry fixture encountered WouldBlock, HTTP-error fixture returned Ok(()) instead of its expected error. Holder/cause and production impact remain unverified.

Diagnose the real isolation/synchronization cause before changing code. Graphify first, then inspect exact fixture names, file handles, global state and locking lifetimes. Use only owned synthetic files/loopback servers. Never dump global lock/process information. Process inspection output is pid/comm only; no arguments, cmdline, environments, secret files or excluded reviewer log. A precise unresolved blocker is preferable to an invented cause.

The 128-yield workaround is not an accepted repair. Remove or replace only after establishing the cause, preserving original success/failure/day/held-lock behavior, the exact two-request assertion and expected-error assertions. Do not blanket-serialize tests, weaken assertions, add random sleeps or change production semantics. If production edits appear necessary, demonstrate the cause and hand over before expanding scope.

## Proof and handover

Run the bounded affected parallel cluster and one matching library run, package formatter, strict all-targets lint and release build through cargo-slot release/jobs3/shared sccache/artifacts. Wait on flocks normally; no build/process interruption, bypass, cargo clean or copied caches. Justified small repeats may expose this measured nondeterminism, not endless full suites. Distinguish executed disposable-PG tests from legacy DB tests which self-skip. No production DB/data or paid calls.

M0 already includes P1's four Social lint repairs: adopt them by rebase, do not duplicate. The inherited tb-raid transitive lint is not a source-review blocker; establish main-baseline parity and review the owned diff. Do not touch tb-raid unless the actual parent merge gate blocks on it.

Commit only verified own files, ending `Co-Authored-By: Claude Code <noreply@anthropic.com>`. Report final clean SHA, precise cause or blocker, original/current reproduction, exact checks/skips and any conflict resolutions. No push/main/gate/deploy, services/root changes, agents/workflows/T3/SendMessage coordination or publication. English work and report; German product text, no added code comments or em dashes. Main orchestrator `eddfce29-0b2a-4f47-931f-8b5387672f94`; direct coordinator P2 session `78e43a4b-d7c4-4910-adb3-26be78165791`.

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min | Worktree: /home/nathanael/.worktrees/tb-social-ai-provider
