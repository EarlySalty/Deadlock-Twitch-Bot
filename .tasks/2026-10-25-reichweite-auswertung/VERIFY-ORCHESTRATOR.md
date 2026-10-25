# Orchestrator verification checks

Own queries against the production DB (read-only), run 2026-10-25. Queries: `queries-VERIFY.sql`.

## V1. Late sampling in `twitch_session_viewers`

| Sessions started | Sessions | First sample after min 5 | After min 15 | After min 30 |
|---|---|---|---|---|
| 2026-01 to 2026-07 | 8,847 | 58.8 % | 25.3 % | 18.4 % |
| since 2026-08 | 5,691 | 55.5 % | 23.9 % | 22.0 % |

The earlier brief note "about 60 % after minute 15" was wrong; the correct figure is about 25 %. Confirms P11.

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

P12 reported 0.80x new presences per viewer when 6-10 German Deadlock streams are live instead of 1-5 (pooled ratio, month x hour strata). Pooled per-viewer rates fall steeply with channel size, so the ratio mixes composition with competition. Recomputed on the same 16 s stream-minute panel (`verify-v5-competition.py`): new presences over the next 15 minutes per stream-minute, compared inside cells of streamer x viewer bin x hour band x weekend, at least 30 minutes on each side, weighted per streamer, bootstrap over streamers.

| Contrast | Streamers | Mean difference | 95 % CI | Median | Share negative |
|---|---|---|---|---|---|
| 6-10 live minus 1-5 live | 329 | +0.027 | -0.003 to +0.062 | 0.0 | 27.7 % |
| 11+ live minus 1-5 live | 196 | +0.003 | -0.046 to +0.065 | 0.0 | 43.4 % |

Baseline at 1-5 live: 0.11 new presences per 15 min for 1-2 viewers, 0.21 for 5-9, 0.46 for 20-49. No measurable cost of more German streams being live. The synthesis must not claim that avoiding busy slots wins viewers.

## V6. Early-stream jumps are returning viewers, not new discovery

P12 found that unexplained viewer jumps cluster in the first 20 to 100 minutes of a stream. Check: Deadlock sessions of at least 2 h since 2026-03-11 whose first chatter-list sample is within 5 minutes of the start (1,777 sessions). Each arrival (first time a login appears in the session, chatters and lurkers, logins seen in fewer than 10 channels, broadcaster excluded) is classed as returning if the login appeared in an earlier session of the same channel.

| Minutes since start | New to channel, per 100 session-minutes | Returning, per 100 session-minutes |
|---|---|---|
| 0-10 | 1.5 | 14.0 |
| 10-30 | 2.1 | 3.3 |
| 30-60 | 2.3 | 2.1 |
| 60-120 | 1.9 | 1.6 |

The opening burst is almost entirely regulars arriving after the go-live notification (90 % of first-10-minute arrivals are returning). New people arrive at a flat 1.5 to 2.3 per 100 session-minutes over the first two hours, with no early exposure peak. The 0-10 bucket also holds viewers already present at the first sample. Tips must not claim that Twitch pushes new streams to new viewers in the first minutes; the start matters for regulars, and new viewers accrue with time live.

## V7. The reply-to-follow effect from P08 is reverse causation

P08 reported a follow within 60 minutes for 9.3 % of first messages with a broadcaster message within 3 minutes, against 3.0 % without. Recomputed in three steps (`queries-VERIFY.sql`, V7a to V7c). Logins seen in 10+ channels are excluded, follow-tracked channels only, April and May 2026 skipped (no follow rows), strata are channel x chat activity in the 10 minutes before the message (0-4, 5-19, 20+ messages), Mantel-Haenszel with bootstrap over channels.

| Check | Reply arm n | Follow 60 min, reply vs none | MH difference (95 % CI) |
|---|---|---|---|
| V7c: per-session first message, follows before the message dropped | 394 | 57 follows, 52 of them inside the 3-minute window | |
| V7b: same, follows inside the 3-minute window also dropped | 342 | 1.5 % vs 2.2 % | -0.9 pp (-1.6 to -0.2) |
| V7a: first message ever in the channel, same exclusions | 70 | 5.7 % vs 5.8 % | -2.3 pp (-4.4 to +0.2) |

52 of the 57 quick follows in the reply arm happen in the same 3 minutes as the broadcaster message: the follow alert prompts the thank-you, not the other way round. Once those are removed, a broadcaster message after a first chat line shows no follow lift inside the same channel. Greeting newcomers stays sensible for retention reasons (V2 shows conversation depth matters), but the synthesis must not promise more follows from a quick reply.

## V8. Ending near the peak and the next start: smaller, but it holds

P11 reported +0.120 (in units of the streamer's median viewers) on the next start when a session ends above its own median end-to-peak ratio. Recheck (`queries-VERIFY.sql` V8, `verify-v8-end-next.py`): sessions of at least 1 h since 2026-01-09, next session within 48 h and first sampled by minute 5, three earlier sessions known, streamer fixed effects, values capped at 5x the median, bootstrap over streamers.

| Model | Pairs | Streamers | Effect on next start (95 % CI) |
|---|---|---|---|
| Raw | 3,228 | 80 | +0.064 (+0.033 to +0.105) |
| + current average, current start, mean of last 3 sessions | 3,228 | 80 | +0.059 (+0.031 to +0.095) |
| + duration and current peak | 3,228 | 80 | +0.067 (+0.040 to +0.101) |
| Placebo: same flag vs the PREVIOUS session's start | 3,143 | 73 | +0.005 (-0.026 to +0.035) |
| Channels with median 5+ viewers, full model | 442 | 12 | +0.086 (+0.021 to +0.181) |

The effect is about half of P11's figure but survives level, duration and peak controls, and the placebo against the earlier session is null, so it is not a channel-level trend. A stream that still holds most of its peak audience at the end is followed by a start about 6 to 9 % of the usual audience higher. This supports ending while the room is still full instead of streaming into an emptying room; it does not show that cutting a stream short helps, because longer streams also collect more viewers (P03, P04).

## V9. Title effects from P01 shrink to near zero against nearby sessions

P01 compared all of a streamer's sessions with and without a title feature. A streamer who adds "!discord" or brackets during a growth phase gets credited with that growth. Recheck (`queries-VERIFY.sql` V9, `verify-v9-titles.py`): German Deadlock sessions of at least 30 min since 2026-01-09 (10,917 sessions). Each session with the feature is compared with the same streamer's sessions without it inside plus or minus 14 days (at least 2 such sessions), log2 average viewers, median across streamers, bootstrap CI and sign test.

| Feature | All sessions (reproduces P01) | Matched within 14 days |
|---|---|---|
| Bang command (`!discord`, `!socials` and so on) | +12.8 % (48 streamers, p 0.013) | +3.1 % (27, CI -2.7 to +9.4, p 0.44) |
| Word "discord" | +22.5 % (22, p 0.052) | +5.7 % (14, CI +3.1 to +24.7, p 0.057) |
| Bracket tag | +9.2 % (37, p 0.32) | +0.5 % (29, p 0.71) |
| Title under 30 characters | -3.3 % (183, p 0.018) | -1.7 % (137, CI -4.4 to -0.1, p 0.026) |
| Word "chill" | -13.0 % (35, p 0.090) | -0.8 % (29, p 0.46) |

The large title effects are mostly time trends inside the channel. What stays is small: very short titles cost about 2 %, and "discord" may help a little (14 streamers, borderline). No title word is a reach lever worth chasing.

## V10. Broadcaster chat messages do not pull in new chatters within a stream

P05 (and P10) found far more chatters per viewer in sessions where the broadcaster writes in chat (+0.418 per average viewer, 110 streamers). At session level this cannot separate cause from reaction: a busy chat makes the streamer type. Recheck on 5-minute slots (`queries-VERIFY.sql` V10, `verify-v10-slots.py`): sessions since 2026-07-01 with at least 5 viewer messages, logins in 30+ channels removed (144,967 slots). Outcome: at least one new chatter (first message in the session) in the next slot. Mantel-Haenszel within session, stratified by viewer messages in the current and previous slot and by whether a new chatter already appeared; bootstrap over streamers.

| Check | Streamers | Exposed slots: next-slot new chatter | Unexposed | Within-session difference (95 % CI) |
|---|---|---|---|---|
| A: broadcaster wrote in the slot | 128 | 16.1 % (1,241) | 11.5 % (7,199) | +1.8 pp (-2.1 to +5.0) |
| B: same, only slots where viewers were silent | 54 | 9.9 % (364) | 7.9 % (4,959) | -0.1 pp (-4.6 to +4.3) |
| C: reverse, new chatter in the slot -> broadcaster writes next | 426 | 3.6 % (7,132) | 3.1 % (11,053) | +0.5 pp (-0.2 to +1.3) |
| D: viewers chatting in the slot (vs silent) | 594 | 13.9 % (32,366) | 7.2 % (89,616) | +1.7 pp (+0.9 to +2.5) |

Typing in chat, also to break a silence, shows no measurable pull on new chatters inside the same stream. A chat that is already moving does carry a small lift (D, about +1.7 pp per slot), consistent with P05's finding that lurkers start writing in livelier rooms. Spoken interaction, which is how most streamers talk to chat, is invisible here, so the honest reading is: the session-level gap is engagement style, not the act of typing.
