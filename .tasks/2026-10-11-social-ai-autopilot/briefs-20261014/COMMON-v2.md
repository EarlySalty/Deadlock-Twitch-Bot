# Finish wave v2, common rules (2026-10-14, orchestrator session 8d18a769)

Repo: Deadlock-Twitch-Bot (`~/repos/Deadlock-Twitch-Bot`, never edit that checkout). Live release 3250999ca. Spec: `../BRIEF.md` (acceptance = section 32). Global rules: `/home/nathanael/.claude/CLAUDE.md`, `/home/nathanael/Documents/claude-config/orchestrierung/ABLAUF.md`.

Your package ends at gate ALLOW plus push to main. You do NOT build a release and do NOT deploy; the orchestrator runs one integrator release for everything merged.

Work:
- Own worktree: `git -C /home/nathanael/repos/Deadlock-Twitch-Bot worktree add -b <branch> /home/nathanael/.worktrees/<name> origin/main` (unless your brief names an existing one). Touch only your write paths.
- Code search: graphify query/explain first (skill code-suche), then targeted grep.
- No code comments. Rust: no unwrap in production paths, small functions. UI text in natural German with real umlauts and ß; reuse existing components and styles.
- Builds only via `cargo-slot <cargo args> --jobs 3`; it waits for a slot by itself. Frontend: npm build, typecheck, eslint in the affected app.
- NO paid AI/STT calls, NO publishing, NO real streamer-setting changes, NO autopilot enablement, NO Brain activation. Never print `bot.toml` or any secret.

Step economy (each model call resends your whole context):
- Read each relevant file once, plan all edits per file, apply them together. Send independent tool calls in one turn.
- One chained check command per round, output trimmed (`2>&1 | tail -40`). No full logs or diffs in context.
- Never sleep-poll. Long jobs with `run_in_background`, keep working until the notification.
- Each check once; repeat only after a change that affects it.
- Merge origin/main into your branch only right before the gate, and again only if the gate or a conflict requires it.
- Foreign tool (Moli, Docker, test container) hangs twice: skip that artifact, note it, continue. Do not debug it.

Finish:
- Commit (stage only your files, no `git add -A`), then `python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo <abs worktree> --base origin/main --head HEAD --chain`. On BLOCK fix the cause and rerun, max 5 rounds, then report the concrete blocker. Inherited findings that also exist on origin/main do not block.
- After ALLOW: `git -C <abs worktree> push origin HEAD:main` as a single command (no chains, no pipes). If rejected because main moved: fetch, merge origin/main, rerun only affected checks, push again.
- Then remove your worktree and local branch (check `git merge-base --is-ancestor` exit code first).
- Product, scope or design question not settled by your brief: stop and report `FRAGE AN ORCHESTRATOR:` with options and findings. Do not decide it yourself.
- Final report at most 8 lines: SHA on main, gate verdict, checks run, what is open.
