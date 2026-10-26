# TODO: Twitch reach analysis (state 2026-10-26, after gate round 6 fixes)

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
| P08 | follows and subs | done, FINDINGS-P08.md (tip 1 downgraded to an association by V7) |
| P09 | raids, shared chat | done, FINDINGS-P09.md |
| P10 | global category benchmark | done, FINDINGS-P10.md |
| P11 | stream opening, games | done, FINDINGS-P11.md (tip 8 withdrawn by V8) |
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
- Broadcaster chat replies show no within-channel effect on return (V2). For follows the raw gap is
  mostly the alert thank-you; with the order respected a smaller association remains (V7). Tips may
  call a quick reply a likely-helpful habit, not a lever.
- More German streams live costs no new viewers within streamer (V5). Early-stream jumps are
  returning regulars, not discovery (V6).
- Ending near the peak does not lift the next start once reconnects are merged into one broadcast
  (+0.008, CI -0.015 to +0.025, V8; +0.074 to +0.084 without the merge is a reconnect artifact). Title word effects mostly vanish against a +-14 day local baseline; only titles under 30
  characters keep a small cost (V9). Neither a broadcaster chat message nor an active chat raises new chatters
  in the next 5 minutes within the session (V10, after trimming the late start of tracked chat;
  the first run's +1.7 pp for active chat was that artifact). Social messages draw no extra
  follow-up at equal chat speed; game talk draws less (V11).

## Next steps

1. Gate round 1 (BLOCK, 11 findings) is fixed: P06 shared-chat copies removed and the German
   return gap rechecked against live days (`p06-live-days.py`); P08 exposure cut at t0, next
   sample and session end; V6 rerun on the 30-second presence poll; V7 rerun with the order of
   message, reply and follow respected; wording fixes in P07, P10, V9, V11 and the synthesis.
2. Gate round 3 (BLOCK, 6 findings) is fixed: P08 rates take follows and viewer-hours from the
   same sampled intervals (q2f, 2.81 per 100 vh, collection gap 2026-03-15 to 2026-06-16
   excluded); V8 merges reconnects into one broadcast (null result); P02 counts chatters from
   messages; P06 no longer drops newcomers with earlier silent presence (`prior_silent` split);
   P07 bounds each label window over every message source; V5 viewer bins are contiguous.
   The follow-gap class is fixed in every follow outcome: a unit with outcome window [a, b) counts
   only if b <= 2026-10-25 17:28 UTC and the window does not overlap 2026-03-15 18:00 to
   2026-06-16 16:45 UTC (V7 in `queries-VERIFY.sql`, P08 first chat in statement 10, P08 lurker
   presence in `p08-chat-follow-rates.py`, P09 raids in `p09-raid-follows.py`). P09 also counts
   follows only in channels with follow tracking. New numbers: V7 +12.1 pp (78 replies), lurker
   presence 10.1 vs 81.2 per 1,000, P09 +0.087 follows per raid within 60 min. The blog carries
   them since Website commit 13956fa.
3. Gate round 4 (BLOCK, 2 findings) is fixed: V8 dedups broadcasts with
   `coalesce(stream_id, id::text)` so sessions without a stream id are no longer collapsed, and
   P08 q2f splits every sample interval at raid window start and end as well as at the 5-minute
   and peak bins, taking attributes at the piece midpoint. While rerunning, a collector fault
   showed up: `twitch_session_viewers.minutes_from_start` is stuck at 0 for 314 sessions from
   2026-06-22 to 2026-07-03. Every query now uses the session clock
   `floor(extract(epoch from (ts_utc - started_at)) / 60)`, and P04, P08, P11, V1 and V8 were
   rerun on it (`p04-curves.py`, `p08-aligned.py` reproduce the tables). New numbers: raid window
   7.9 per 100 vh, 0 to 5 minutes before the peak 11.5, minutes 0 to 10 12.4 per 100 vh (483 vh),
   V8 +0.008 (73 streamers), strong previous session +0.020 with CI including 0, P11 ramp
   0.18 / 0.65 / 0.84. The blog carries them since Website commit 0c13619 (live 2026-10-26).
4. Gate round 5 (BLOCK, 6 findings) is fixed; the numbers below replace the round 2 and 3 values
   above. P08 exposure intervals are disjoint per streamer (a sample interval starts no earlier
   than the previous one ended), so every follow sits in exactly one interval: 2.81 per 100 vh on
   44,546 vh, minutes 0 to 10 12.6 per 100 vh (476 vh). Overlapping sessions of one streamer
   count as one broadcast for P08 first chat, P08 lurker presence (10.2 vs 81.6 per 1,000, 2,007
   broadcasts) and V7 (first message in a broadcast +2.3 pp, CI -0.3 to +4.6; first message ever
   +12.1 pp, CI +4.7 to +17.3); the V7 follow window is 24 h 3 min, which the cutoff and gap
   checks use. P09 compares each raid with the mean of its own controls and bootstraps over
   raids: +0.037 follows per raid within 60 min (CI -0.018 to +0.091, 205 raids), a null. P03,
   P04 and P11 count first-time chatters from messages (`chat-counts.sql`) instead of the session
   column that includes lurkers; the Saturday penalty and the 19-20h hint are withdrawn. P05
   question labels count only for sessions ending before 2026-10-05 00:00 UTC, where labels are
   complete; P06 needed no rerun (its labelled first messages end 2026-09-25).
5. Gate round 6 (BLOCK, 3 findings) is fixed in the shipped files, numbers unchanged.
   `queries-P03.sql`, `queries-P04.sql` (Q3) and `queries-P11.sql` compute the message-based
   chatter counts inline with the `chat-counts.sql` rule; `queries-P05.sql` returns NULL for every
   label count of a session ending after 2026-10-05 00:00 UTC; `p08-aligned.py` stops on a follow
   that lies in two exposure intervals instead of picking one (1,252 follows, 2.81 per 100 vh).
6. Re-run `gate_hook.py --review` (skill rolle-merge-schleuse) until ALLOW, merge to main, clean
   up branch and worktree, cancel wakeup `reichweite`. Update the blog with the round 5 numbers.
   Report to the user in English.
