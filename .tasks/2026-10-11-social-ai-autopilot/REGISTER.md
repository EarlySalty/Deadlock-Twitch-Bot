# Register: Social AI Autopilot

Intent thread (main orchestrator): `eddfce29-0b2a-4f47-931f-8b5387672f94`, Claude Opus 5.5 session `cc9b61b9-7d63-4977-9486-b11a84bdb047`.
Stage: riesig. Base origin/main `3862b764a`. User allowed GPT workers on 2026-10-11; stale sol/astra quota locks released.

## Session register

| Pkg | Thread ID | Creator | Start proof | Harness | Model | Status | Worktree | Branch | HEAD | Last report |
|---|---|---|---|---|---|---|---|---|---|---|
| P1 | 81c8c48a-fde2-4def-b87a-0e1c2eaa34b9 | cc9b61b9 | t3-harness new 04:31:40Z, running | claudeAgent | gpt-6.1-sol ultracode | running | /home/nathanael/.worktrees/tb-social-autopilot | feat/social-autopilot-20261011 | 0085b717f | M0 LIVE 33656f2c; M1a LIVE d5c30b730 (import 1, 178 eligible, 143 pool rows, no upload effects); M1b import UI on main 0085b717f (gate ALLOW, 58/0/0, not yet mounted; P3 told to adopt); B UI fixed 590739f24, runner waits on P2 lease; C early PublishReviewPanel contract 7878c5431 (forwarded to P3) |
| P2 | 92e11d80-06bd-4765-aa75-41d5b8fe1757 | cc9b61b9 | t3-harness new 04:31:40Z, running | claudeAgent | gpt-6.1-sol ultracode | running | /home/nathanael/.worktrees/tb-social-ai (+ tb-social-ai-*) | sub-branches social-ai-core 6b5c62fd7, jobs b1c39b55f, provider 8a962fe0e, review-ui 35847ff6c, stt-deploy 1056488f6 | 536b53c05 | M1 ON MAIN 536b53c05 (gate ALLOW after 2 BLOCK rounds); deploying alone, paid test held for user approval; MediaRuntime follow-up pending |
| P3 | 10eb1247-91e9-4677-b3b3-4bcdba076044 | cc9b61b9 | t3-harness new 04:31:40Z, running | claudeAgent | gpt-6.1-sol ultracode | running | /home/nathanael/.worktrees/tb-social-dash | feat/social-dashboard-redesign-20261011 (fix/p3-social-responsive fafd98e75) | ec81c8b48 | M1 LIVE 33656f2c; M2 on feat/p3-social-m2-layout-20261011: layout 6fb82a1bc, schedule fixture 45c2695ad, import fixture ec81c8b48 (accepted, preview checked); 590739f2 schedule proof running; rebase on fresh main before gate |

Decisions sent: Anthropic connector scoped to social editor (Opus 5.5 via YAML, $2 hard test cap), per-user consent default off, transient STT spawn, global advisory production lease, AiEditorPanel contract, pid/comm-only process checks. Open with user: DB credential rotation after log redaction; final assisted logged-in UI check.

## 2026-10-11 ~21:10Z: parallel tracks (user: "build the stuff parallel")
- P1: A = M1a gate/deploy then M1b UI; B = M2 schedule + runner; C = M3 modes/approval/retries + M4 retention/notifications.
- P2: A = M1 gate/deploy/live run; B = M2 revisions/produce/multi-output; C = M3 learning; D = M4 admin pages.
- P3: M2 started now against fixtures and published contracts; integrates real components as they land.
- Guardrail: memory about 6 GB free, cargo -j2 via cargo-slot, never two release builds at once.

## 2026-10-12 ~05:40Z decisions
- DB role deadlock password rotated 04:10Z (user approved), all 9 consumers restarted and verified.
- Speed directive sent: critical path P2 M1 -> P1 B -> P3 M2, no re-verification ceremony, one batched deploy, 15-min watch.
- P1 B composes on P2 candidate 741ba64a0 (WIP adf4a6206), rebases on P2 main SHA.
- MediaRuntime owned by P2 (media-only loader, EditorRuntime builds on it); B uses editor=None when AI config fails.

## 2026-10-12 ~20:55Z: takeover by native Opus subagents
- P1/P2/P3 Sol threads stopped: weekly limit, resets 2026-10-18 06:21 Europe/Berlin. Do not resume them before that; work moved to native subagents of session d7e0c71c.
- A critical path: TOML fix fc77864af gate/merge/deploy, then P1 Track B 7d70fa8a8 gate/merge/deploy.
- P2 M2 revisions/versions/produce/transfer (from f3cd92197 / b6e8a57e6), migrations 20261011203xxx.
- P2 M3+M4 learning, skill versioning, admin usage/budgets (from b8ed37b5a, d56efa51b), migrations 20261011204xxx.
- P3 M2 dashboard integration (from 8ae9a1031).
- P1 Track C modes/approval/retries/retention/notifications (contract 7878c5431), migrations 20261011105xxx.
- All: no paid AI calls until user approves the paid live test; review mode only, no real publishing.
