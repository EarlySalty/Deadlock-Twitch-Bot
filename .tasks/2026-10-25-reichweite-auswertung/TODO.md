# TODO: Twitch reach analysis (state 2026-10-25 21:05 CET)

User goal: deep analysis of all Twitch data on how German Deadlock streams win new viewers, turn
lurkers into chatters and newcomers into followers, subs and regulars; also when Twitch shows a
channel more often. The user chose Haiku for the data analysis. The orchestrator (main session)
verifies and writes the synthesis.

Branch `analysis/reichweite-20261025`, worktree `/home/nathanael/.worktrees/twitch-reichweite`
(repo Deadlock-Twitch-Bot, PUBLIC: aggregates only, no logins, IDs or chat quotes).

## Package state

| Package | Topic | State |
|---|---|---|
| P01 | titles | done, FINDINGS-P01.md (tips 1, 2, 4, 5 downgraded by V9) |
| P02 | tags | done, FINDINGS-P02.md |
| P03 | timing, competition | done, FINDINGS-P03.md |
| P04 | length, frequency, growth | done, FINDINGS-P04.md |
| P05 | lurkers vs chatters | done, FINDINGS-P05.md (tips 1 and 3 withdrawn by V10) |
| P06 | first-time chatter loyalty | done, FINDINGS-P06.md |
| P07 | chat content | done, FINDINGS-P07.md (tips 1 and 2 downgraded by V11) |
| P08 | follows and subs | done, FINDINGS-P08.md (tip 1 withdrawn by V7) |
| P09 | raids, shared chat | done, FINDINGS-P09.md |
| P10 | global category benchmark | done, FINDINGS-P10.md |
| P11 | stream opening, games | done, FINDINGS-P11.md (tip 8 annotated by V8) |
| P12 | exposure triggers | done, FINDINGS-P12.md |

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
- Ending near the peak lifts the next start by about +0.07 of the streamer median, placebo null
  (V8). Title word effects mostly vanish against a +-14 day local baseline; only titles under 30
  characters keep a small cost (V9). Neither a broadcaster chat message nor an active chat raises new chatters
  in the next 5 minutes within the session (V10, after trimming the late start of tracked chat;
  the first run's +1.7 pp for active chat was that artifact). Social messages draw no extra
  follow-up at equal chat speed; game talk draws less (V11).

## Next steps

1. All packages are in and verified; `SYNTHESE.md` and `SYNTHESE.html` are final.
2. Commit only this folder, run `gate_hook.py --review` (skill rolle-merge-schleuse), merge to
   main on ALLOW, clean up branch and worktree, cancel wakeup `reichweite`. Report to the user in
   English.
