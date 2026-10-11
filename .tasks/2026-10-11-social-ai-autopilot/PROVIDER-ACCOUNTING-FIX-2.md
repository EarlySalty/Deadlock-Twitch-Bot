# Central provider accounting repair, round 2

Worktree: `/home/nathanael/.worktrees/tb-social-ai-provider`.
Branch: `feat/social-ai-provider-20261011`.
Preserved clean HEAD: `8a962fe0e9660ba086e025f566c7e5af49b56e91`.
Completed fixer/reviewer must not be resumed.

## Confirmed recovery-accounting defect

`social_editor/mod.rs:588-625` handles failed ledger finalization by journaling a generic `ledger::Completion`. That type at `ledger.rs:55-67` has tokens/status/latency but no native estimated cost, cost basis or cache counters. Recovery at `ledger.rs:115-133` writes only those generic fields. Consequently a known usage-priced estimate above the reservation is lost, and the central budget sum later counts the smaller reservation. The normal over-reservation path exists and is mock-tested; this is not proof of actual billed overspend.

Repair central durable completion/recovery accounting without a second Social budget or store. Preserve token-priced amounts as estimates, not invoices. Close the window before reconciliation as well: a known unresolved finalization cannot allow subsequent reservations against a lower understated debit. The exact conservative fail-closed mechanism is yours to choose; preserve bounded retry/restart behavior and explain how unresolved attempts are reconciled without forgiving unknown usage.

Reuse the existing central recovery path, and keep old non-Social/Fireworks completion behavior and historical recovery payloads compatible. Native accounting must survive DB-finalization failure, recovery and replay, including over-reservation estimates, cache counters and unknown usage. No raw prompts, transcript, frames, response body or credentials in the journal. No new local budget/token files. No model/rate/cap change.

## Refuted reviewer claim

The review also claimed this commit grants `twitchaudit` INSERT/UPDATE on user consent. Parent read of the actual clean committed migration shows `GRANT SELECT ON public.social_ai_user_consent TO twitchaudit` at line 40; INSERT/UPDATE at line 41 apply to `llm_usage`, not consent. That claim is refuted as stated. Do not blindly modify already-correct consent grants. A bounded effective-privilege regression check against the actual migration is useful; if inherited/default grants demonstrate a real problem, report and fix that concrete cause instead.

## Ownership and proof

Own central provider files/tests, provider-owned configuration/migration, and minimum additive generic ledger/caller construction changes needed for compatible accounting. Preserve Fireworks route/model/request behavior. A narrowly necessary repair of the existing unchanged `hub.rs:962` test lint is acceptable if required for matching all-targets checks; no broad refactor or lint suppression.

Required independently checkable proof: synthetic DB finalization failure followed by provider recovery and attempted new reservations; higher known estimate cannot be lost or undercounted, replay is safe, legacy recovery remains compatible, unknown usage stays reserved/unknown, authoritative $2 ceiling persists. Retain existing consent/concurrency/schema/security tests. No paid call.

Earlier handover reports package formatter, release build, production Clippy and 51 tests green; all-targets Clippy has an unchanged test lint. Recheck actual commands/results and report skipped/blocked checks honestly. Use shared cargo-slot release/jobs3 policy, no competing release builds or artifact/cache copies. Slots were observed fully held at 07:15 UTC; no bypass or foreign process stop.

English report and commit, German product errors, no code comments/em dashes. Commit own verified files with required Claude Code attribution. No push/main/gate/deploy, production data, secrets or publication. Parent owns integration, actual merge gate and first paid/live proof. Handover includes full SHA, paths, recovery/debit semantics, exact successful/failed checks and open runtime prerequisites.
