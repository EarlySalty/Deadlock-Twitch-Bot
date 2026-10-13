[Orchestrator]
BRIEFING[WB-1]: Required parts 5/5 | Timer 20 min | Worktree: /home/nathanael/.worktrees/ttb-multiserver-sol-review-20261013

# Full roadmap review, fixes, integration and live completion

## Goal and authority
The user explicitly requests GPT 6.1 Sol agents to finish ALL of https://github.com/users/EarlySalty/projects/3/views/1. The existing implementations are built drafts, NOT reviewed. They explicitly authorize the agents to review, fix and merge the PRs themselves. This new authorization supersedes the old review-only/no-merge/no-deploy handover in PR #1077. It does not authorize bypassing gates or inventing acceptance evidence.

You are the responsible domain sub-orchestrator AND sole integration owner for the entire coupled Multi-Server/English roadmap, issues #1060 through #1070, PRs #1071 through #1077. Use GPT 6.1 Sol native subagents for independently scoped review and implementation in owned worktrees, and fresh independent acceptance reviewers. No other model/provider substitution without approval. Start by verifying native workflow availability under ultracode; fail honestly if unavailable. No extra T3 threads or coordination with foreign sessions. Native subagents within your own session are expressly authorized.

Source specification: /home/nathanael/.worktrees/ttb-multiserver-plan/.tasks/2026-10-12-multiserver-englisch/PLAN.md (read-only), also on remote docs/multiserver-plan-20261012. Fetch latest project, issue and PR facts. English product text is explicitly required by this task, preserving German behavior and natural German translations. Task-specific language specification supersedes the normal German-only product default for these translations.

## Ownership and current state
Own worktree: /home/nathanael/.worktrees/ttb-multiserver-sol-review-20261013
Own branch: integration/multiserver-sol-review-20261013
Initial HEAD: a3e7a40cf631e4f9ab3e24fb58128bc0a6c21378, based on origin/feat/multiserver-board-20261013.
Clean at creation except this task record. Coordinator owns REGISTER.md; you own AUFTRAG.md only for necessary appended decisions and your implementation artifacts. Keep task records on feature branches, not main.

You own all roadmap changes in Deadlock-Twitch-Bot, not unrelated features/Dependabot. Partition exact file ownership among native workers, serialize overlaps, especially shared migrations, Cargo files, API, i18n and integration. Preserve foreign dirty worktrees and all prior branches; never edit or delete them. The user specifically authorizes integrating these existing roadmap PRs, so existing-branch rules do not prohibit incorporating their committed changes. Do not blindly merge every draft: PR #1077 already includes H/O/A/K/W/R. Check ancestry and actual content to avoid duplicate integration.

Known existing read-only candidates, verify latest committed state yourself:
- #1077 origin/feat/multiserver-board-20261013, initial a3e7a40cf. Old body says G/D/C still landing; confirm actual current content, not just body.
- D: /home/nathanael/.worktrees/ttb-multiserver-d-pr branch feat/dashboard-englisch-pr-20261013, initial 304f92bc4. Original /home/nathanael/.worktrees/ttb-dashboard-englisch at 9ac8e3929; do not duplicate already included translation commits.
- G: /home/nathanael/.worktrees/ttb-multiserver-g, branch feat/multiserver-g-guilds-20261013, initial 598a3ab29.
- C: /home/nathanael/.worktrees/ttb-multiserver-c, branch feat/multiserver-c-texts-20261013, initial 93e55ce38.
- L issue #1069 marked Done: independently check relevant actual behavior, do not rebuild it.
Do not take ownership of these existing checkouts or their sessions. Incorporate committed content into your own branches. If missing work is genuinely uncommitted, preserve it and report the exact blocker rather than destroying it.

## Work and proof contract
Read /home/nathanael/.claude/CLAUDE.md and /home/nathanael/Documents/claude-config/orchestrierung/ABLAUF.md; pass applicable rules to every native worker. Load code-suche and Graphify before code search, plus review, test, UI, merge and deploy skills on demand.
1. Inventory all ten roadmap packages against the actual plan and draft diffs. Treat all prior built/tested claims as unreviewed. Identify missing wiring, conflicts and regressions, not just compile errors.
2. Review security/correctness independently, including tenant isolation, Discord permissions, trusted OAuth host/state/redirect/cookies, migration ordering/backfill/compatibility, partner-state shadow comparison, home-server ads, bidirectional cross-server raid preferences and language, central AI guard with no foreign-server leakage, translations across UI/chat/Discord/DM, and ledger completeness.
3. Fix concrete defects and missing required functionality with isolated native Sol workers. No unrequested refactors. Production Rust only, existing frontend toolchain allowed, no new code comments, no production unwrap, no editing applied migrations. No new SQLite.
4. Run matching formatter/build/lint and affected existing tests using documented cargo-slot/build rules. Read /home/nathanael/Documents/claude-config/machines/v50671/RUST-BUILDS.md and ionos-builds.md. No competing release builds. Real runtime evidence matters; self-written green tests alone do not prove the contract. Browser checks only Moli, read agent-browser.md first, no Brave or preview browser fallback. UI must be functionally and visually checked in German and English at 390/1440.
5. Independent combined acceptance on exact integration SHA, then local gate_hook.py --review through rolle-merge-schleuse. For BLOCK, fresh native fixer per round and rerun, maximum five failed rounds before concrete blocker. The same author cannot approve their own changes. Do not bypass hooks.
6. After ALLOW and acceptance, YOU merge locally, push, deploy verified origin/main via the existing deploy wrapper, restart only affected services, and prove the live behavior. No waiting for coordinator Go. Serialize main updates and release verification using existing hooks/wrappers/locks. Preserve German production operation and live streams.
7. Keep PRs and board truthfully reconciled with integration. Link every PR you work on in your T3 thread. Do not mark a package Done merely because built or merged. Preserve traceability for superseded PRs; never merge duplicate diffs just to close them. Commit verified intermediates and push promptly. Git attribution: Co-Authored-By: Claude Code <noreply@anthropic.com>; PR bodies end with the required Generated with Claude Code attribution.

Secrets only via Infisical export helper --list, then eval of the immediately needed named secret and unset. Never read/print secret files or raw credentials, no unredacted DB output. No user/community data to external AI. No real-account unlink/rotation or unsolicited community posts/test messages. Use disposable authorized accounts and test guild for live mutation tests. New Discord app/credentials, redirect allowlist registration, test-server permission or legal/product decisions may require user action: prove exact blockers, finish all independently safe work, never fabricate them. Existing plan lists dashboard AI for foreign servers as unresolved recommendation off: preserve fail-closed existing implementation while flagging final product choice if still unresolved; do not enable paid/AI behavior. Do not touch ai-coach or Deadlock-Bots; this plan expressly leaves Deadlock-Bots unchanged.

## Routing and completion
Direct coordinator: Claude Code session 6bde3a2b-67e8-4a24-985f-bbbd48c1732b, task multiserver-sol-review-20261013. Report through this own thread and git, not ListAgents/SendMessage to foreign sessions. You may use native parent/child tools internally. Keep native raw reports local and track their ownership/evidence in your own domain register distinct from coordinator REGISTER.md.
Continue autonomously until complete or concrete authorization/resource blocker. Do not stop after a plan or after launching workers. At actual quota exhaustion save TODO.md per global rule; no stored quota locks. Final report at most five lines: exact SHAs, gate verdict/evidence, live yes/no with URL/UI location and actual proof, unresolved items. Settle your own thread only after completion. Clean up only own merged worktrees after live proof, preserving coordinator task records until collected.
