# [Orchestrator] Resume note, Social AI finish build (2026-10-14)

Role: leaf worker for exactly one package (named below). Orchestrator: T3 thread 6e3faead-77a5-4642-97ca-be9e870a68f2 (Claude Opus). Do not start T3 threads or further orchestrators. Native `Agent` subagents are allowed only for gate fixer rounds and independent checks.

Decisions belong to the orchestrator. Implement exactly what the brief says. When something is not decided by the brief (product behaviour, UI structure beyond the brief, scope changes, a choice between two valid designs, migration of live data), do not pick yourself: end your turn with `FRAGE AN ORCHESTRATOR:` plus the question, the options and what you found. Do not stop for things the brief or COMMON.md already settle.

State: the previous agent for your package (Claude Opus) stopped on a usage limit mid-work. Its UNCOMMITTED changes are still in your worktree (based on origin/main 437b641ad; the branch already exists). Do not recreate worktree or branch. Start with `git status` and `git diff`, compare with the package list, continue from there. Commit verified intermediate states and publish the branch to origin promptly.
origin/main has moved (d21e72c6a, includes the REL cleanup 9a3ad772e). Merge origin/main into your branch before the gate. Live release is 00e750557.
Deploy: the wrapper does not run migrations; check for unapplied migrations before deploying (memory twitch-connect-rank-live-und-migrator-luecke.md under ~/.claude/projects/-home-nathanael-Documents/memory/). One deploy at a time (wrapper lock).

Final report, at most 8 lines: SHA on main, gate verdict, deployed release, live proof, open items. Then `python3 ~/Documents/tools/t3-thread.py settle --selbst`.

---
