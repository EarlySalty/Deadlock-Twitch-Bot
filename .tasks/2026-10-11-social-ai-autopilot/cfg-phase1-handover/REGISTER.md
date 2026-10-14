# CFG handover register

Date: 2026-10-14. Status: handed back at the user's request; implementation stopped.
Orchestrator: `6e3faead-77a5-4642-97ca-be9e870a68f2`.
User instruction: do not spawn new agents, subagents, workflows or threads; await their new directions.
Worktree: `/home/nathanael/.worktrees/tb-social-cfg`; branch: `feat/social-cfg-bot-toml-20261014`.
HEAD: `9b0c13723bc52540e52c5b8ddde92014d21aafe0`; upstream tracking ref is gone. Do not recreate it automatically.
Latest locally observed `origin/main`: `6cac863acfaa394970ef40b8efc01c441a8ff128`.
Binding briefs: coordinator `.tasks/2026-10-11-social-ai-autopilot/briefs-20261014/{COMMON,BRIEF-CFG}.md`.

## Completed

- Phase 1 is merged and live on `3250999ca9e1488c6ed33bf2c9c68956b051a327`; formal gate `bzhtud06f` returned ALLOW.
- Exact previous Social AI values were migrated through the hash-checked editor with a verified backup. Social Brain remains absent. Do not repeat the migration.
- Live worker startup was positively proved; four public admin routes returned 401 JSON. No paid AI/STT, publishing or streamer-setting changes were performed.
- Handover-time wrapper check: exit 0; bot/dashboard/coaching-watch PIDs 306451/306764/308315, same release, nondeleted executables, NRestarts=0.
- Check for tb-bot/tb-dashboard-api and selected tb-config/tb-llm/tb-social-media Clippy passed. Installer regression suite: 35 passed, 2 existing skips. Do not describe every historical full Rust suite as green: global_file has a measured baseline failure; the earlier social-media full run had 470 passed/1 failed, followed by an isolated preview pass and 14 media-runtime passes.

## Open, awaiting the user's new direction

- All phase 2 code: explicit nonsecret typed catalog in tb-config, bounds/choices/lists/nullable service defaults, full candidate validation including existing Social AI pricing and runtime checks, existing locked/hash-checked editor persistence and backup.
- Existing admin operating route: preserve auth-before-file-access, CSRF/origin and database-connection handling; add catalog changes and actual active service values, never infer active bot values from mismatching saved fingerprints.
- Existing Betriebseinstellungen page: grouped searchable cards, typed inputs, validation, German text, service values and restart notices. No new dashboard. No phase 2 source changes or checks exist.
- Remaining acceptance: matching formatter/build/lint/existing tests, functional and visual Moli checks at 390/1440, gate, integration and live verification if phase 2 is commissioned. The new no-agent instruction supersedes earlier reviewer/fixer spawning permissions.
- Cleanup was deliberately not performed during handover: assigned worktree/local branch, owned detached integration worktree `/home/nathanael/.worktrees/tb-social-cfg-baseline-20261014`, unused release clone `/home/nathanael/repos/twitch-release-9b0c13723bc52540e52c5b8ddde92014d21aafe0`. Own throwaway DB was already removed. No own build/test job remains running.

This register supersedes stale checkpoints in TODO.md, EVIDENCE.md, PLAN.md and REVIEW.md. Handover documentation is local and uncommitted; no new source edit, commit, main push, build, deploy or cleanup was performed for this handover. Preserve artifacts until the orchestrator decides what to keep.
Deployment rules remain: check live ancestry first, reuse a containing release or wait for its pending deploy, do not merge origin/main into the feature branch without a gate/conflict need, and skip a Moli capture after two failures without debugging Moli.
