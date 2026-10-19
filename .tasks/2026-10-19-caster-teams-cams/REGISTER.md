# Register: caster teams and cameras

Intent thread: `808e8c98-61e2-4605-b7d0-d4a690450c3b` (Grok 4.6 orchestrator after Opus limit)
Worktree: `/home/nathanael/.worktrees/tb-caster-teams-cams`
Branch: `feat/caster-teams-cams`

| Paket | Session | Modell | Status | HEAD | Note |
|---|---|---|---|---|---|
| P1 Discord casters + cam | `...a3656fa9ef08bcce3` | opus 5.5 | built, not merged; do not resume | `395cc7cb0` | Moli: `.tasks/2026-10-19-caster-teams-cams/screens/p1-*.png` |
| P2 team library + coach | (wip on branch) | grok-4.6 successor | in progress | uncommitted then wip | UI/overlays exist; API not wired |
| P3 1440p60 camera | pending | grok-4.6 successor | not started | | after P2 merge |

Integrator: this intent thread. Worker gates and merges each package to main. No release-build/deploy by the worker.
