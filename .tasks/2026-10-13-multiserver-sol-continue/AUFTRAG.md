[Orchestrator]

# Multi-Server roadmap: continue review, fix, merge and go live

## Goal
Finish GitHub project https://github.com/users/EarlySalty/projects/3 (roadmap #1070, packages D/H/O/G/A/R/K/C/W/L, PRs #1071 to #1077). The user explicitly wants GPT 6.1 Sol agents to review, fix and merge these built but unreviewed drafts themselves, so this part of the bot gets finished. You are the sole sub-orchestrator and integration owner. Use native Sol subagents for scoped writers and fresh independent reviewers. No other model or provider.

Acceptance: every package either merged to main through local gate ALLOW and live-verified, or reported with a concrete, proven blocker (credentials, redirect registration, test guild access, legal or product decision). Board and PRs reconciled truthfully.

## Why this is a continuation
A previous Sol orchestrator (thread e65ed91f-a9b6-42c5-8452-18e66ed6f614, Claude session 5c00dffd-02da-4a1f-a6ee-019ccc515fca) did most of the review and part of the fixes, then died at 08:52 UTC from GPT proxy ECONNREFUSED. Its native workflows are gone. Do not resume or write into that thread or its process. Its authority and rules carry over unchanged:

- Original briefing (binding, read first): /home/nathanael/.worktrees/ttb-multiserver-sol-review-20261013/.tasks/2026-10-13-multiserver-sol-review/AUFTRAG.md
- Its domain register with every finding, ownership split, accepted and blocked SHA (read only, includes 10 uncommitted appended lines in that worktree): /home/nathanael/.worktrees/ttb-multiserver-sol-review-20261013/.tasks/2026-10-13-multiserver-sol-review/DOMAIN-REGISTER.md
- Spec: /home/nathanael/.worktrees/ttb-multiserver-plan/.tasks/2026-10-12-multiserver-englisch/PLAN.md

## State to pick up
- Latest verified integration: branch integration/multiserver-sol-verified-20261013 at e150dc69b (worktree /home/nathanael/.worktrees/ttb-sol-integrate-v2-20261013, clean). Assembly base 7a15931f3. origin/main has moved 41 commits since; rebase or merge main in deliberately.
- Many ttb-sol-* worktrees under /home/nathanael/.worktrees hold uncommitted, uncompiled WIP from dead writers (ai, auth, chat, chat-r1, hub-effects-v2, language-data, membership-r1, guild-roles, denylist, social-i18n-r1, uplink-i18n-r1, web, ...). Treat them as read-only drafts: salvage by reading the diff into fresh worktrees and branches owned by a fresh writer, then independent review. Never edit, reset or delete those original worktrees.
- Open blockers from the register: hub credentials not in Infisical, hub unit not installed, domain redirects, migration activation defects with shadow parity, foreign dashboard AI stays fail-closed (unresolved product choice), English privacy copy needs legal approval, L issue #1069 live proof currently impossible because channel_profile calls fail HTTP 412.

## Worktree and branch
- Your worktree: /home/nathanael/.worktrees/ttb-multiserver-sol-continue-20261013
- Your branch: integration/multiserver-sol-continue-20261013, created from cbed6fa86 (task-record commit of the previous run)
- Your register: .tasks/2026-10-13-multiserver-sol-continue/DOMAIN-REGISTER.md in this worktree. Commit and push it regularly so a further restart can pick up from git, not from memory.

## Process (unchanged from the original briefing, short form)
1. Re-verify the current state of each package against the register and the actual branches before building anything.
2. Fresh native Sol writer per scope with exclusive file ownership, fresh independent reviewer per finished branch; the author never approves own changes.
3. Matching fmt, build, clippy, affected existing tests per RUST-BUILDS.md and cargo-slot; UI checked in Moli in German and English at 390 and 1440.
4. Combined acceptance on the exact integration SHA, then gate_hook.py --review via rolle-merge-schleuse. On BLOCK a fresh fixer per round, max five rounds.
5. After ALLOW: merge to main, push, deploy verified origin/main via the existing deploy wrapper, restart only affected services, prove live behaviour. German production behaviour and live streams stay undisturbed.
6. Reconcile PRs and board; link every PR you touch to your T3 thread with link_pull_request.

All rules from /home/nathanael/.claude/CLAUDE.md apply and must be passed to workers: no code comments, no production unwrap, no edits to applied migrations, secrets only via Infisical helper, no real-account unlink or rotation, no community posts, no model or cost changes, Moli only for browsers, no Sonnet.

If the GPT proxy fails again (ECONNREFUSED), save state to .tasks/2026-10-13-multiserver-sol-continue/TODO.md and push before stopping.

## Reporting
Direct coordinator: Claude Code session 51e1498f-f4ca-4c6f-8900-6a73abd53c2a. Report only via your own thread and git. Final report at most five lines: exact SHAs, gate verdict, live yes/no with URL or UI location and proof, unresolved blockers. Settle your own thread only after completion.
