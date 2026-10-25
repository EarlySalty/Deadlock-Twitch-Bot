# TODO: Twitch reach analysis (state 2026-10-25 20:15 CET)

User goal: deep analysis of all Twitch data on how German Deadlock streams win new viewers, turn
lurkers into chatters and newcomers into followers, subs and regulars; also when Twitch shows a
channel more often. The user chose Haiku for the data analysis. The orchestrator (main session)
verifies and writes the synthesis.

Branch `analysis/reichweite-20261025`, worktree `/home/nathanael/.worktrees/twitch-reichweite`
(repo Deadlock-Twitch-Bot, PUBLIC: aggregates only, no logins, IDs or chat quotes).

## Package state

| Package | Topic | State |
|---|---|---|
| P01 | titles | resumed 19:57, running |
| P02 | tags | done, FINDINGS-P02.md |
| P03 | timing, competition | done, FINDINGS-P03.md |
| P04 | length, frequency, growth | done, FINDINGS-P04.md |
| P05 | lurkers vs chatters | resumed 19:57, running |
| P06 | first-time chatter loyalty | resumed 19:57, running |
| P07 | chat content | resumed 19:57, running |
| P08 | follows and subs | done, FINDINGS-P08.md (tip 1 withdrawn by V7) |
| P09 | raids, shared chat | done, FINDINGS-P09.md |
| P10 | global category benchmark | done, FINDINGS-P10.md |
| P11 | stream opening, games | resumed 19:57, running |
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
- Broadcaster chat replies show no within-channel effect on return (V2) or on follows (V7: the
  follow alert prompts the thank-you); tips must not claim either.
- More German streams live costs no new viewers within streamer (V5). Early-stream jumps are
  returning regulars, not discovery (V6).

## Next steps

1. After the Haiku limit reset (22:10 CET): resume the SAME stopped agents with SendMessage (user
   decision, no fresh spawns). Agent ids: P01 a1879883538bcfd58, P02 a438a5af86250a3df,
   P03 a35fa98244ee4bf79, P05 ac1cbfb7c1e2cce2d, P06 a8ac14dfe0e975bc3, P07 ad74e288c3ab2f477,
   P08 a81f3b96dc981e787, P10 a51316e1f22f287e2, P11 a6292928aab92488a. Each returns the COMPLETE
   findings text; the orchestrator saves it as FINDINGS-<PKG>.md. Wakeup `reichweite` at 22:15.
2. Orchestrator verifies the strongest claims with own queries, then writes `SYNTHESE.md`
   (prioritized tips with evidence, null results, caveats) and optionally an HTML visual.
3. Commit only this folder, run `gate_hook.py --review` (skill rolle-merge-schleuse), merge to
   main on ALLOW, clean up branch and worktree. Report to the user in English.
