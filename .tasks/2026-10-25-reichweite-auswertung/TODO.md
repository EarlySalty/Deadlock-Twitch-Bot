# TODO: Twitch reach analysis (state 2026-10-25 19:50 CET)

User goal: deep analysis of all Twitch data on how German Deadlock streams win new viewers, turn
lurkers into chatters and newcomers into followers, subs and regulars; also when Twitch shows a
channel more often. The user chose Haiku for the data analysis. The orchestrator (main session)
verifies and writes the synthesis.

Branch `analysis/reichweite-20261025`, worktree `/home/nathanael/.worktrees/twitch-reichweite`
(repo Deadlock-Twitch-Bot, PUBLIC: aggregates only, no logins, IDs or chat quotes).

## Package state

| Package | Topic | State |
|---|---|---|
| P01 | titles | stopped by Haiku limit, early stage |
| P02 | tags | stopped right before writing findings |
| P03 | timing, competition | stopped right before writing findings |
| P04 | length, frequency, growth | done, FINDINGS-P04.md |
| P05 | lurkers vs chatters | stopped mid-analysis (conversion window) |
| P06 | first-time chatter loyalty | stopped right before writing findings |
| P07 | chat content | stopped right before writing findings |
| P08 | follows and subs | stopped mid-analysis |
| P09 | raids, shared chat | done, FINDINGS-P09.md |
| P10 | global category benchmark | stopped right before writing findings |
| P11 | stream opening, games | stopped right before writing findings |
| P12 | exposure triggers | done, FINDINGS-P12.md |

## Resources for a fresh agent per open package

- Common rules: `BRIEF-COMMON.md` in this folder.
- Original package brief: `/tmp/reichweite-scratch/VERIFY/partial/<PKG>-brief.md`.
- What the stopped agent found so far (narration only): `/tmp/reichweite-scratch/VERIFY/partial/<PKG>-texts.md`.
- Its scratch CSVs and scripts: `/tmp/reichweite-scratch/<PKG>/`; its SQL if written: `queries-<PKG>.sql`.
- DB: `/tmp/tq.sh` (read-only, credentials via env, not argv).

## Corrections the orchestrator must apply

- The "about 60 % of sessions start sampling after minute 15" note was wrong. P11 measured
  (since August): 55.5 % after minute 5, 24 % after minute 15. Fix the caveat in FINDINGS-P04.md
  after verifying.
- `first_message_at` in `twitch_session_chatters` differs from the real first message in
  `twitch_chat_messages` for about 29 % of chatters (P05). Lurker `first_message_at` is the first
  poll time, not arrival (P12).
- P06 measured broadcaster replies within 3 min at 974 of 18,421 newcomers (5.3 %), not the
  orchestrator's 602; reconcile definitions.
- Lurker baselines must exclude logins seen in 30+ channels (bots, scene surfers): they return
  at 52 %; ordinary lurkers at 15.5 % (VERIFY-ORCHESTRATOR.md V4). About 30 % of one-message
  newcomers are commands, spam or moderated (V3).
- Broadcaster chat replies show no within-channel effect on return (V2); tips must not claim it.

## Next steps

1. After the Haiku limit reset (22:10 CET): spawn one fresh Haiku agent per open package with the
   resources above. Each finishes its package from the scratch state, reruns only what it needs,
   and returns the COMPLETE findings text in its final reply. The orchestrator saves it as
   FINDINGS-<PKG>.md.
2. Orchestrator verifies the strongest claims with own queries, then writes `SYNTHESE.md`
   (prioritized tips with evidence, null results, caveats) and optionally an HTML visual.
3. Commit only this folder, run `gate_hook.py --review` (skill rolle-merge-schleuse), merge to
   main on ALLOW, clean up branch and worktree. Report to the user in English.
