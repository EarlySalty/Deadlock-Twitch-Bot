# Orchestrator verification checks

Own queries against the production DB (read-only), run 2026-10-25. Queries: `queries-VERIFY.sql`.

## V1. Late sampling in `twitch_session_viewers`

| Sessions started | Sessions | First sample after min 5 | After min 15 | After min 30 |
|---|---|---|---|---|
| 2026-01 to 2026-07 | 8,847 | 60.1 % | 25.7 % | 18.8 % |
| 2026-08 to 2026-10-25 17:28 UTC | 5,678 | 55.5 % | 24.0 % | 22.1 % |

The first sample minute is `floor((first sample - started_at) / 60)`. The earlier brief note "about 60 % after minute 15" was wrong; the correct figure is about 25 %. Confirms P11 (14,525 sessions in both).

Collector fault found during this check: the stored `twitch_session_viewers.minutes_from_start` is 0 for every sample of 314 sessions from 2026-06-22 to 2026-07-03 and equals the session clock everywhere else. All minute measures in P04, P08, P11 and here now use the session clock.

## V2. Does a broadcaster chat reply bring newcomers back?

First-time chatters (`messages >= 1`) from 2026-02-20 to 2026-09-24 (30-day follow-up), real first message time from `twitch_chat_messages`: 8,477 newcomers, 583 channels. "Back" = present again (chat or lurk) in another session of the same channel within 30 days.

| Newcomer messages in first session | Broadcaster wrote within 3 min: n / back | Did not: n / back |
|---|---|---|
| 1 | 145 / 18.6 % | 2,907 / 10.3 % |
| 2-4 | 124 / 27.4 % | 2,250 / 18.6 % |
| 5+ | 158 / 36.7 % | 2,893 / 33.9 % |
| all | 427 / 27.9 % | 8,050 / 21.1 % |

Within channel and message bucket (Mantel-Haenszel, bootstrap over channels): back -0.7 pp (CI -6.1 to +5.3), chats again -2.1 pp (CI -7.3 to +3.8), 98 channels. A broadcaster chat message naming the newcomer within 10 min (134 newcomers, 25 channels): back -2.3 pp (CI -10.9 to +7.1).

Reading: the raw advantage is a channel effect (channels where the streamer chats actively also have more returning viewers). Within the same channel a chat reply shows no measurable effect. Spoken greetings are invisible in this data, so this does not test greeting by voice.

## V3. One-message newcomers: spam and commands

3,052 newcomers with exactly one message (same window). First message type and 30-day return:

| Type | n | Back within 30 days |
|---|---|---|
| ordinary message | 2,124 | 14.1 % |
| command (`!...`) | 392 | 4.8 % |
| link or promo pattern | 390 | 1.8 % |
| moderated | 146 | 0.0 % |

About 30 % of one-message newcomers are commands, spam or moderated. Ordinary one-message newcomers return at 14 %.

## V4. Lurker baseline is inflated by logins seen in many channels

First-time rows by group and by how many channels the same login appeared in since 2026-02-20:

| Group | Login in fewer than 10 channels: n / back | 10-29 channels | 30+ channels |
|---|---|---|---|
| lurker | 9,272 / 15.5 % | 1,343 / 32.8 % | 1,044 / 52.1 % |
| 1 message | 2,868 / 9.1 % | 85 / 36.5 % | 100 / 33.0 % |
| 2-4 messages | 2,140 / 17.4 % | 113 / 39.8 % | 121 / 29.8 % |
| 5+ messages | 2,574 / 31.3 % | 245 / 46.5 % | 233 / 51.1 % |

For ordinary viewers (fewer than 10 channels): a silent newcomer returns at 15.5 %, one who writes 5+ messages at 31.3 %. A single genuine message is not better than silence; a real conversation is. Logins seen in 30+ channels include bots and scene-wide surfers and should be excluded from lurker baselines.

## V5. Competition effect from P12 does not survive a within-streamer check

P12 reported 0.80x new presences per viewer when 6-10 German Deadlock streams are live instead of 1-5 (pooled ratio, month x hour strata). Pooled per-viewer rates fall steeply with channel size, so the ratio mixes composition with competition. Recomputed on the same 16 s stream-minute panel (`verify-v5-competition.py`): new presences over the next 15 minutes per stream-minute, compared inside cells of streamer x viewer bin x hour band x weekend, at least 30 minutes on each side, weighted per streamer, bootstrap over streamers. Viewer bins are contiguous on the averaged viewer value (under 3, 3 to under 5, 5 to under 10, 10 to under 20, 20 to under 50, 50 and more); an earlier version used integer ranges and dropped fractional values between them.

| Contrast | Streamers | Mean difference | 95 % CI | Median | Share negative |
|---|---|---|---|---|---|
| 6-10 live minus 1-5 live | 337 | +0.027 | -0.001 to +0.064 | 0.0 | 26.4 % |
| 11+ live minus 1-5 live | 199 | +0.004 | -0.045 to +0.066 | 0.0 | 43.2 % |

Baseline at 1-5 live: 0.11 new presences per 15 min under 3 viewers, 0.21 for 5 to under 10, 0.46 for 20 to under 50. No measurable cost of more German streams being live. The synthesis must not claim that avoiding busy slots wins viewers.

## V6. Early-stream jumps are returning viewers, not new discovery

P12 found that unexplained viewer jumps cluster in the first 20 to 100 minutes of a stream. Check on the 30-second chatter-list poll (`twitch_viewer_presence_ticks`, data from 2026-04-04; `queries-VERIFY.sql` V6, run per month): Deadlock sessions of at least 2 h from 2026-05-04, in channels with at least 30 days of earlier chatter data, whose first poll is within 5 minutes of the start and which have polls in at least 20 of the 24 five-minute blocks of the first 2 hours. 1,123 sessions, 42 channels, first poll on average 47 s after the start. An arrival is the first poll that lists a login (chatters and lurkers, logins seen in fewer than 10 channels, broadcaster excluded). It is returning if the login appeared in an earlier session of the same channel.

| Minutes since start | New to channel, per 100 session-minutes | Returning, per 100 session-minutes | Returning share of arrivals |
|---|---|---|---|
| 0-10 | 1.3 | 17.2 | 92.9 % |
| 10-30 | 1.8 | 3.8 | 68.1 % |
| 30-60 | 2.2 | 2.5 | 53.3 % |
| 60-120 | 1.8 | 1.8 | 50.1 % |

The opening burst is almost entirely regulars arriving after the go-live notification (93 % of first-10-minute arrivals are returning). New people arrive at a flat 1.3 to 2.2 per 100 session-minutes over the first two hours, with no early exposure peak; in 5 of the 6 months the first 10 minutes bring the fewest new arrivals (May, 120 sessions, is the exception). The 0-10 bucket includes the 308 logins already listed at the first poll (30 new, 278 returning, about 0.3 per session); without them it reads 1.05 new and 14.7 returning (93.3 % returning). Arrival times are poll times of the chatter list, not exact join times. An earlier version of this check used `first_message_at` in `twitch_session_chatters` as the arrival time (1,777 sessions since 2026-03-11) and gave the same picture (90 % returning, new 1.5 to 2.3). Tips must not claim that Twitch pushes new streams to new viewers in the first minutes; the start matters for regulars, and new viewers accrue with time live.

## V7. The reply-to-follow gap from P08 is mostly reverse causation; a smaller association remains

P08 reported a follow within 60 minutes for 9.3 % of first messages with a broadcaster message within 3 minutes, against 3.4 % without. Recheck with the order of message, reply and follow respected (`queries-VERIFY.sql` V7, `verify-v7-reply-follow.py`). Logins seen in 10+ channels are excluded, follow-tracked channels only, first messages dropped when the 24-hour follow window from any landmark (reply or synthetic landmark, at most 3 minutes after the first message, so first message + 24 h 3 min) touches the follow collection gap (no follow rows from 2026-03-15 18:00 to 2026-06-16 16:45 UTC) or ends after the cutoff (2026-10-25 17:28 UTC); an earlier bound of first message + 24 h gave a byte-identical export (6,120 events), so no event sat in the 3-minute margin, strata are channel x chat activity in the 10 minutes before the message (0-4, 5-19, 20+ messages). In the reply arm the clock starts at the broadcaster message, and events whose follow came at or before it are dropped. Messages without a reply get the same landmark: each is evaluated at the nine deciles of the reply delays in its activity stratum, counting only at landmarks it reaches without having followed (fractional weights). Mantel-Haenszel, 1,000 bootstrap draws over channels. Placebo: among messages without a reply, those where the broadcaster had written in the 3 minutes before, against those where not.

| Check | Events (broadcaster message within 3 min) | Quick follows in the reply arm: before / after the broadcaster message | Follow within 60 min from the landmark, reply vs none | MH 60 min (95 % CI) | MH 24 h (95 % CI) | Placebo MH 60 min (95 % CI) |
|---|---|---|---|---|---|---|
| V7b: first message in a broadcast, 57 channels | 6,115 (389) | 38 / 14 of 52 | 5.41 % (351) vs 3.06 % (5,654) | +2.29 pp (-0.26 to +4.64) | +2.42 pp (-0.19 to +4.94) | +0.06 pp (-2.78 to +3.13) |
| V7a: first message ever in the channel, 44 channels | 1,999 (110) | 32 / 14 of 46 | 23.08 % (78) vs 8.47 % (1,835) | +12.07 pp (+4.70 to +17.31) | +11.61 pp (+2.34 to +17.37) | -0.79 pp (-7.84 to +14.24) |

Quick follows are follows within 3 minutes of the first message. Placebo rates: V7b 3.43 % (weighted n 253) vs 3.04 % (5,402); V7a 10.84 % (72) vs 8.37 % (1,763). A broadcast is a run of sessions of one streamer whose time spans overlap (reconnects); a viewer's first message counts once per broadcast, which drops 5 second events against the per-session version (6,120 events, MH 60 min +2.28 pp, CI -0.32 to +4.76; V7a CIs +3.14 to +18.05 and +1.05 to +17.57 there, same point estimates, the bootstrap draws follow the row order). A chatter who followed inside the collection gap shows no follow row and counts as not yet following afterwards; this affects both arms alike.

Most quick follows in the reply arm come before the broadcaster message: the follow alert prompts the thank-you, so the raw gap is largely reverse causation. Measured from the reply on, a smaller lift remains: +2.3 pp for any first message in a broadcast (the interval touches zero) and +12 pp for a first message ever in the channel (78 replies). The placebo is null, so the lift is not just the broadcaster reading chat at that moment. It is still an association from a small arm: streamers answer some newcomers and not others for reasons the data does not see (a line that invites a reply, a quiet moment). The synthesis may call answering a newcomer's first line soon a likely-helpful habit, not a measured lever. An earlier version of this check dropped every follow inside the 3-minute window regardless of order and found no lift; that design also removed follows that came after the reply and is replaced.

## V8. Ending near the peak does not lift the next start

P11 reported +0.125 (in units of the streamer's median viewers) on the next start when a session ends above its own median end-to-peak ratio. Recheck (`queries-VERIFY.sql` V8, `verify-v8-end-next.py`): rows of the same stream_id are deduplicated, and sessions of one streamer that start within 10 minutes of the previous end (or overlap it) are merged into one broadcast, so a reconnect is not counted as the next stream. Broadcasts of at least 1 h since 2026-01-09, next broadcast within 48 h and first sampled by minute 5, three earlier broadcasts known, streamer fixed effects, values capped at 5x the median, bootstrap over streamers (300 resamples).

| Model | Pairs | Streamers | Effect on next start (95 % CI) |
|---|---|---|---|
| Raw | 3,045 | 73 | +0.011 (-0.011 to +0.030) |
| + current average, current start, mean of last 3 broadcasts | 3,045 | 73 | +0.008 (-0.014 to +0.025) |
| + duration and current peak | 3,045 | 73 | +0.008 (-0.015 to +0.025) |
| Placebo: same flag vs the PREVIOUS broadcast's start | 2,993 | 71 | -0.007 (-0.027 to +0.015) |
| Channels with median 5+ viewers, full model | 418 | 13 | +0.042 (-0.003 to +0.097) |
| Strong broadcast (average above own median), raw | 3,045 | 73 | +0.030 (+0.003 to +0.055) |
| Strong broadcast, + current start, mean of last 3 broadcasts | 3,045 | 73 | +0.020 (-0.002 to +0.044) |
| Placebo: strong broadcast vs the PREVIOUS broadcast's start | 2,993 | 71 | +0.013 (-0.018 to +0.044) |

Null result. Without the merge, the same check finds a lift: V8 with the broadcast counter in `queries-VERIFY.sql` replaced by `sum(1)`, so every session is its own broadcast, run through `verify-v8-end-next.py` unchanged, gives +0.079 raw (CI +0.044 to +0.117) and +0.074 to +0.084 with the controls (CI +0.041 to +0.121), 3,371 pairs from 87 streamers; the placebo against the previous start gives +0.009 to +0.016. Of the 6,152 pairs it exports, 608 have the next start within 10 minutes of the end and 288 overlap the current session: those are reconnects of one broadcast, where the "next start" is the audience that was still there. With reconnects merged, the effect is +0.008 to +0.011 with intervals that include zero. Ending while the room is still full is not a measured lever for the next stream.

The same merge tests P11's "strong previous session" result (section 6, +0.065 on session pairs). A broadcast with average viewers above the streamer's median goes with +0.030 on the next start raw, +0.020 with start and level controls (interval includes zero), and +0.013 against the previous start (placebo). Most of the raw lift is the streamer's current level, so a strong broadcast is not an established lever either.

## V9. Title effects from P01 shrink to near zero against nearby sessions

P01 compared all of a streamer's sessions with and without a title feature. A streamer who adds "!discord" or brackets during a growth phase gets credited with that growth. Recheck (`queries-VERIFY.sql` V9, `verify-v9-titles.py`): German Deadlock sessions of at least 30 min since 2026-01-09 (10,917 sessions). Each session with the feature is compared with the same streamer's sessions without it inside plus or minus 14 days (at least 2 such sessions), log2(average viewers + 1), median across streamers, bootstrap CI and sign test. Percentages are ratios of (average viewers + 1), which slightly understates relative changes for very small channels.

| Feature | All sessions (reproduces P01) | Matched within 14 days |
|---|---|---|
| Bang command (`!discord`, `!socials` and so on) | +12.8 % (48 streamers, p 0.013) | +3.1 % (27, CI -2.7 to +9.4, p 0.44) |
| Word "discord" | +22.5 % (22, p 0.052) | +5.7 % (14, CI +3.1 to +24.7, p 0.057) |
| Bracket tag | +9.2 % (37, p 0.32) | +0.5 % (29, p 0.71) |
| Title under 30 characters | -3.3 % (183, p 0.018) | -1.7 % (137, CI -4.4 to -0.1, p 0.026) |
| Word "chill" | -13.0 % (35, p 0.090) | -0.8 % (29, p 0.46) |

The large title effects are mostly time trends inside the channel. What stays is small: very short titles cost about 2 %, and "discord" may help a little (14 streamers, borderline). No title word is a reach lever worth chasing.

## V10. Neither broadcaster typing nor a moving chat pulls in new chatters within a stream

P05 (and P10) found far more chatters per viewer in sessions where the broadcaster writes in chat (+0.418 per average viewer, 110 streamers). At session level this cannot separate cause from reaction: a busy chat makes the streamer type. Recheck on 5-minute slots (`queries-VERIFY.sql` V10, `verify-v10-slots.py`): sessions since 2026-07-01 with at least 5 viewer messages, logins in 30+ channels removed. Outcome: at least one new chatter (first message in the session) in the next slot. Mantel-Haenszel within session, stratified by viewer messages in the current and previous slot and by whether a new chatter already appeared; bootstrap over streamers.

P07 showed that tracked chat often starts late (first tracked message after minute 15 in 62.9 % of sessions with chat, 39.9 % even in sessions with 200+ messages). Untracked slots look silent, and at the start of tracking everyone already in the room counts as a new chatter. The first run kept those slots and gave D +1.7 pp (CI +0.9 to +2.5). That was this artifact. The table below drops every slot before the first tracked slot plus 2 (10 minutes); trimming plus 6 gives the same picture (D -0.2 pp, CI -0.9 to +0.5).

| Check | Streamers | Exposed slots: next-slot new chatter | Unexposed | Within-session difference (95 % CI) |
|---|---|---|---|---|
| A: broadcaster wrote in the slot | 111 | 15.2 % (1,047) | 11.7 % (5,727) | +1.4 pp (-2.3 to +5.1) |
| B: same, only slots where viewers were silent | 51 | 8.9 % (292) | 7.7 % (3,740) | -0.4 pp (-4.8 to +5.7) |
| C: reverse, new chatter in the slot -> broadcaster writes next | 339 | 4.0 % (5,632) | 3.3 % (9,287) | +0.6 pp (-0.1 to +1.4) |
| D: viewers chatting in the slot (vs silent) | 488 | 12.6 % (27,372) | 7.5 % (55,178) | +0.0 pp (-0.7 to +0.6) |

Inside the same stream, neither typing in chat (also to break a silence) nor an already moving chat raises the chance that a new person writes in the next five minutes. The raw gaps (15.2 vs 11.7 %, 12.6 vs 7.5 %) are the busy-room level, not a pull. Spoken interaction, which is how most streamers talk to chat, is invisible here. The session-level gap in P05 and P10 is engagement style and room size, not the act of typing.

## V11. P07's "social messages draw more replies" is a busy-moment effect

P07 reported a 2-minute follow-up lift of 1.10 for Social viewer messages (60 % of streamers above 1) and 0.70 for Game-Related. Lift there is the label mean over the session mean, so a label that tends to be posted when chat is already fast gets a lift without causing anything. Recheck (`queries-VERIFY.sql` V11, `verify-v11-followup.py`): 305,446 messages from 2026-02-01 to 2026-10-04, logins in 30+ channels removed, viewer messages from 5 minutes after the first tracked message to 2 minutes before the last (265,364 messages, 518 streamers). Follow-up = messages by other chatters (broadcaster included) in the next 2 minutes. Comparison against all other labelled viewer messages in the same session and with the same number of messages by others in the 2 minutes before (buckets 0, 1-2, 3-5, 6-10, 11-20, 21+); bootstrap over streamers.

| Label | Naive lift (median of streamers, share above 1) | Same session and same prior speed: relative follow-up (95 % CI) |
|---|---|---|
| Social | 1.23 (0.71) | -0.8 % (-5.4 to +10.4) |
| Greeting | 1.15 (0.62) | +2.1 % (-10.1 to +14.3) |
| Reaction | 1.02 (0.52) | +4.7 % (+0.4 to +6.4) |
| Question | 0.97 (0.46) | -12.2 % (-18.4 to +0.3) |
| Game-Related | 0.87 (0.32) | -19.2 % (-23.6 to -4.1) |
| Statement | 0.97 (0.40) | -19.4 % (-25.7 to -3.8) |

The naive lift reproduces the direction. Holding chat speed equal, Social messages draw no more follow-up than other messages; the 1.10 is when they are posted, not what they do. Game talk and longer statements stay below other messages at equal speed, short reactions slightly above. Practical reading: chat replies follow the room's tempo. Only short reactions sit slightly above other messages at equal speed (+4.7 %, CI +0.4 to +6.4), too small to build a tip on; no tested message type is a reply lever. The confidence intervals are wide and skewed because a few large channels carry much of the weight.
