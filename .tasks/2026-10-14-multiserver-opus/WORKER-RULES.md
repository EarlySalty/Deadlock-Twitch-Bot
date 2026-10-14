# Binding rules for every sol-worker in the multi-server roadmap

Orchestrator: Opus session in `/home/nathanael/.worktrees/ttb-multiserver-opus-20261014` (branch `orchestrator/multiserver-opus-20261014`). Report only to the orchestrator in your final message. Main orchestrator thread 63e21335-695f-4aa3-8366-ad641b0d948a is not yours to contact.

## Role

- You are a worker for exactly one package. No sub-orchestrators, no T3 threads, no ultracode, do not spawn agents.
- Questions the briefing, repo and these rules do not answer: end with `FRAGE AN ORCHESTRATOR:` plus situation, options, recommendation, worktree, branch, SHA. Never ask the user.
- Branches that existed before your task and are not yours stay untouched: never merge, rebase, delete or push them. Read them as reference only.
- Do not push to `main`. Your package ends with a pushed feature branch, green checks and (when asked) a gate verdict. The orchestrator merges and deploys.

## Code

- Rust for production code, no `.unwrap()` in production paths, no needless `unsafe`. Existing frontend toolchains (React/Vite in `bot/dashboard_v2`, `website/`) stay.
- No code comments at all.
- English for code, identifiers and commit messages. User-facing German text: natural German with real umlauts and ß. User-facing English text: natural English, no AI filler. No em dashes in user-facing text.
- Never edit applied migrations; add a new one with a fresh number after `git fetch origin`.
- Stage only your own files by literal path. Never `git add -A`. No force push, no bypassed hooks, no `--no-verify`. Never commit `.tasks/` evidence into a branch meant for main unless the briefing says so.
- Code search: run `graphify query "<question>" --graph /home/nathanael/repos/Deadlock-Twitch-Bot/graphify-out/graph.json` first (a hook blocks grep before it), then grep for targeted reading.

## Secrets and data

- Never print, echo, cat or grep secrets; never put secrets into argv, PGDATABASE or DSN strings. No `.env` files. No `/proc/*/environ`, no `systemctl show Environment`.
- Infisical only if the briefing requires it: `eval "$(/home/nathanael/Documents/Infisical/export_claude_secret.py --secret NAME)"` for the one action, then `unset NAME`.
- Production DB is locked for agents (credential rotation pending). No production DB access. Use local/test Postgres only.
- No community posts, no Discord/Twitch messages to real users, no paid provider actions, no model or cost changes. Do not touch ai-coach.

## Checks

- Few large steps: read each file once fully, batch edits per file, chain checks into one Bash call with `2>&1 | tail -40`. Never sleep-poll. Verify once.
- Dashboard: `cd bot/dashboard_v2 && npm run build` plus targeted `npx vitest run <explicit files>` and `npx eslint <changed files>`. Never run an unbounded browser test glob; browser tests only by explicit file.
- Rust: `cargo-slot` for builds/tests (follows `/home/nathanael/Documents/claude-config/machines/v50671/RUST-BUILDS.md`), `cargo fmt`, `cargo clippy` on touched crates only. No release builds.
- Inheritance rule: findings that exist unchanged on `origin/main` (lint, formatting, red pre-existing tests, missing translations elsewhere) do not block. Only your delta counts. Measure a baseline on origin/main when you claim a failure is inherited.
- Browser checks only with Moli `/home/nathanael/.local/bin/moli` (read `/home/nathanael/Documents/claude-config/wissen/agent-browser.md` once). Never Brave or any other browser. If Moli hangs twice or longer than 30 minutes, skip that artifact with a note.
- Gate, when the briefing asks for it, as one command with literal paths:
  `python3 -I /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo <ABSOLUTE_WORKTREE> --base <ORIGIN_MAIN_SHA> --head HEAD --model gpt-6.1-sol`
  Run it in the background with a long timeout. Report the verdict line verbatim.

## Final report (max 8 lines)

Branch, pushed SHA, changed paths (short), check results with numbers, gate verdict if run, open items, anything the orchestrator must decide.
