# [Orchestrator] Multi-server and English roadmap, round 2 (Twitch bot)

You are the Opus 5.5 orchestrator (effort medium) continuing the multi-server and English roadmap. The previous Opus orchestrator (thread 44b13b55-25b3-4bfb-be78-f9bbc49626b5) stopped on a usage limit; do not resume or write into it. Its native sol-worker subagents died with it.

Main orchestrator thread (report here, max 3 lines, only merges, live proof, blockers, decisions): f52add9e-54d7-4bc7-9351-2bb88580787b

## Read first

- Original briefing, role and all binding rules: `.tasks/2026-10-14-multiserver-opus/BRIEFING.md` and `WORKER-RULES.md` on branch `origin/orchestrator/multiserver-opus-20261014`. They still apply unchanged (Opus coordinates, sol-worker subagents build, max 4 in parallel, no ultracode, gate via `gate_hook.py --review`, deploy via `/usr/local/bin/deploy-twitch-release <sha>` and `--pruefen`, German production must stay intact, Moli only, no secrets in output).
- Register: `.tasks/2026-10-14-multiserver-opus/REGISTER.md` on the same branch. Continue it there.

## State at handover (verify)

- origin/main and live: 1a46231ee (D2 to D10 live). `--pruefen` was ok.
- D11, D12, D13 (remaining 174 German strings in the English dashboard, #1060): each has one commit pushed, gate status unknown:
  - `origin/feat/opus-d11-pages-english-20261014` 995f4b061 (Verwaltung: overlay builder, private VODs, AI engagement)
  - `origin/feat/opus-d12-pages-english-20261014` a92bf7320 (Uplink, social metadata)
  - `origin/feat/opus-d13-pages-english-20261014` 820001ae3 (trial, onboarding, scopes, roadmap)
  Check each for completeness against its scope, finish missing strings, gate, merge, then one release and deploy for all three, live check DE and EN, close #1060 with SHA and URL.
- H1 (Discord hub fix) was blocked after 5 rounds (ratelimit.rs:195 stale-response misclassification). USER DECISION NOW MADE: rebuild the rate limiter with per-bucket serialization (one in-flight request per bucket) as you recommended. Worktree `ttb-opus-h1-20261014`, branch `feat/opus-h1-discord-hub-20261014`, local HEAD f97e73bfd; push it first if not on origin.
- Queue after that, in this order: 390px live overflow check, C (texts), A (admin dashboard, AI off on foreign servers), G and K only once the DB rotation is confirmed (ask the main orchestrator in one line), L (channel_profile HTTP 412) when a worker is free.

## Extra packages from finished neighbour threads (add to your queue, one sol-worker each)

1. SEC: the pre-push security check reports RUSTSEC-2026-0346 to 0349 (rustls) and cargo-audit lists 9 vulnerabilities; this blocks remote branch deletion (e.g. `feat/tiktok-dialog-presets`, `feat/social-ap-finish-20261014`) and the branch watchdog's pushes. Bump the affected crates on main, gate, merge, then delete those two already-merged remote branches. Check first whether main already contains part of the fix.
2. SOC-EN: Social Media Manager (https://deutsche-deadlock-community.de/twitch/social-media) still has untranslated strings in the English view ("Braucht dich", "Benachrichtigungen", "Zeitplan öffnen", subtitles on Heute, KPI strip, preview banner) and the Approval legend on Planen may overlap content. Coordinate with D12 to avoid overlap.
3. TEST: `tests/socialMediaContract.test.ts` "route discovery observes schedule" and one Challenges test fail on main. Fix the cause if it is a real bug, otherwise fix the test.

## Acceptance

Same as the original briefing: each package merged with gate ALLOW, deployed via wrapper, live checked on the real URL in German and English, German production unaffected; GitHub issues closed with SHA and URL. Done when all packages are live or blocked only by a named user decision. Write `TODO.md` with state and next steps if you run out of quota.
