# P08: Follows and subscriptions

Saved by the orchestrator from the P08 agent's final report (the agent's Write call was rejected). Queries: `queries-P08.sql`.

## Question

1. Where in live sessions do follows happen (stream minute, viewer count, raids, peak)?
2. How do follows relate to chat: before, after, or never; minutes from first chat to follow; does a broadcaster reply within 3 minutes matter?
3. What is the follow conversion per 100 viewer-hours, and which within-streamer session features go with more follower gains?
4. What do subscription events look like (type, gift vs paid, tier, timing), what happens to chat within 10 minutes, and what do gifted recipients do later?
5. Do viewers have community points watch time before they follow or before their first chat?
6. Orchestrator addition: lurker analysis. Silent sightings before follow, and whether followers chatted before following (messages >= 1 counts as chatter). Baselines exclude 77 bot-like logins seen in 30+ channels.

## Data used

| Source | Scope | n |
|---|---|---|
| twitch_follow_events | follows 2026-01-31 to 2026-10-25 17:28 UTC (analysis set) | 1,492 events, 63 streamers; 1,278 inside a session's time span (mapped set), of which 1,252 inside a sampled exposure interval (aligned set); 214 outside any session |
| twitch_stream_sessions / session viewer samples | tracked sessions since 2026-02-19 (lurker set) | 3,009 sessions, 2,855 sampled; presence rates use a re-export of 2026-10-26 (3,016 sessions) merged into 2,007 sampled broadcasts (overlapping sessions of one streamer count as one broadcast, checked against every session since 2026-01-25 from statement 20) from 58 streamers whose broadcast plus 60 minutes ends by the cutoff and does not touch the follow gap |
| twitch_session_chatters | lurker and chatter rows | 53,996 presence rows (33,226 silent and 10,312 chatter rows in sampled sessions) |
| twitch_chat_messages | chat since 2026-01-31 | monthly volume Feb 30,865; Mar 32,497; Apr 6,229 (27 streamers); May 6,333 (25 streamers); Jun 16,198; Jul 50,906; Aug 77,331; Sep 94,805; Oct 151,545 |
| session features (follower_delta etc.) | sessions from 2026-03-01 | 13,539 sessions, 1,734 streamers; 9,488 sessions from 1,207 streamers with follower_delta |
| twitch_subscription_events | session-joined events, Feb to Oct 2026 | 1,407 events, 71 streamers |
| twitch_community_points_viewer_daily | 49 channels, 2026-08-13 to 2026-10-25 | 14,087 viewer-days, 6,449 viewers |

Bot-like logins: 77 logins seen in 30+ distinct channels in twitch_session_chatters (excluded from lurker baselines). They account for 120 of 1,482 follow events since 2026-02-19.

## Method

- Follow to session mapping: session with started_at <= followed_at <= coalesce(ended_at, started_at + duration); ties broken by latest start, then shortest duration (29 follows overlapped two sessions).
- Exposure (q2f in `queries-P08.sql`): each viewer sample is an interval [a, b) with a = max(sample time, t0) and b = the earliest of the next sample, the session end, 300 s later and the cutoff 2026-10-25 17:28 UTC. Samples inside the follow collection gap (2026-03-15 18:00 to 2026-06-16 16:45 UTC) are dropped. Overlapping sessions of one streamer (reconnects) are made disjoint before aggregation: per streamer the intervals are ordered by (a, session start, session id, sample time) and each a is raised to the largest b of all earlier intervals, so every second of a streamer's stream counts at most once (computed from two days before each export window; June is exported from 2026-06-16 16:45 UTC, in 2-day pieces from 06-23 because the half-month query hit the 560 s timeout). Each interval is split at every 5-minute boundary of the session clock (from started_at), at every 5-minute boundary of the offset to the session peak, and at the start and end of each 10-minute raid window. Each piece takes its stream minute bin, peak bin and raid flag at its own midpoint and the viewer band of its sample. A follow takes the attributes of the piece it falls into, so follows and exposure share the same bins.
- Every rate takes numerator and denominator from the same intervals: a follow counts when it falls inside an interval [a, b) and takes that interval's attributes (aligned set, 1,252 follows). Because the intervals are disjoint, every follow falls into exactly one interval (1,252 follow rows, 1,252 distinct follows). An earlier version kept overlapping exposure twice and counted 3 follows in overlapping sessions once, for the later-starting session. Conversion = follows / viewer-hours.
- Tracked exposure starts at each streamer's first follow event (t0), so only follow-tracked streamers count. Every session that ends at or after t0 is included, so the session holding the first follow counts from t0 on. Peak and raid exposure use the same rule.
- Within-streamer comparisons: for each streamer, feature sessions vs other sessions. Reported: median of per-streamer differences, share of streamers positive, number of streamers and sessions. Size classes 0-4, 5-19, 20-99, 100+ average viewers.
- Outcome for session comparisons: gain share (follower_delta >= 1), mean of follower_delta clipped to [-5, 50], and per hour. follower_delta is discrete (median 0, p95 2), so raw median differences are 0.
- Follow outcome windows: a first message (Part 2) counts only if the 24 hours after it end by the cutoff and do not touch the follow collection gap; a broadcast presence counts only if the broadcast plus 60 minutes does the same. Units whose window falls into the gap would count as non-follows without any follow being recorded.
- Lurker: a silent sighting is a chatters-API row with messages = 0; a chatter is messages >= 1. A follower counts as having a silent sighting before following if a sampled session started before the follow and the last silent sighting was before the follow.
- Subscription chat windows: messages (excluding the streamer) in the 10 minutes before and after each event, plus a per-session 10-minute baseline (session chat_msgs / duration).
- Times: Berlin hours via a manual DST rule.

## Results

### Part 1: follows and live sessions

Aligned set: 1,252 follows, 44,546 tracked viewer-hours. Exposure and follows by stream minute (session clock, 5-minute bins on both sides; n = follows):

| Stream minute | n | Viewer-hours | per 100 vh |
|---|---|---|---|
| 0-10 | 60 | 476 | 12.6 |
| 10-20 | 63 | 862 | 7.3 |
| 20-30 | 57 | 1,022 | 5.6 |
| 30-45 | 88 | 1,782 | 4.9 |
| 45-60 | 82 | 1,946 | 4.2 |
| 60-90 | 121 | 4,133 | 2.9 |
| 90-120 | 110 | 4,052 | 2.7 |
| 120-180 | 182 | 7,468 | 2.4 |
| 180-240 | 129 | 6,055 | 2.1 |
| 240-360 | 125 | 7,909 | 1.6 |
| 360+ | 235 | 8,842 | 2.7 |

Viewer band of the interval holding the follow:

| Viewers | n | Viewer-hours | per 100 vh |
|---|---|---|---|
| 0-4 | 419 | 11,148 | 3.8 |
| 5-9 | 371 | 13,193 | 2.8 |
| 10-19 | 295 | 9,176 | 3.2 |
| 20-49 | 149 | 9,212 | 1.6 |
| 50-99 | 17 | 424 | 4.0 |
| 100-299 | 1 | 672 | 0.15 |
| 300+ | 0 | 721 | 0.0 |

- Follow timing (aligned set): exact median 130.8 minutes into the stream; 7.3% in the first 15 minutes; 72.0% after minute 60. In the minute bins above, 902 of 1,252 (72.0%) fall from minute 60 on and 671 (53.6%) from minute 120 on.
- Per viewer-hour the first minutes convert best: 12.6 follows per 100 vh in minutes 0 to 10 and 7.3 in minutes 10 to 20, against 2.1 to 2.9 from minute 60 on. Few viewers are present early (476 vh in the first 10 minutes), so most follows still come later. The early rate rests on 60 follows and only on sessions sampled from the start (late sampling, see P11).
- 419 of 1,252 aligned follows (33.5%) fall in the 0-4 viewer band. Mapped set (1,278, nearest sample): median 7.0 viewers at follow.
- Follows with an incoming raid earlier in the same session (mapped set): 264 of 1,278 (20.7%).
- Raid window (10 minutes after an incoming raid, aligned set): 63 follows (5.0%), 802 viewer-hours, 7.9 per 100 vh, against 2.7 per 100 vh outside (1,189 follows, 43,743 vh).
- Peak timing (ex-post sample maximum, offset to the peak sample at the piece midpoint, aligned set): bin [-5, 0) (0 to 5 minutes before the peak): 108 follows, 937 vh, 11.5 per 100 vh; bin [0, 5): 69 follows, 1,408 vh, 4.9 per 100 vh; bin [-10, -5): 42 follows, 816 vh, 5.1 per 100 vh; bin [5, 10): 28 follows, 1,103 vh, 2.5 per 100 vh. 177 of 1,252 (14.1%) fall in the two bins around the peak. Mapped set: median follow 3.6 minutes before the session peak.
- Offline follows (no session covers the time): 214 (14.3% of 1,492). In a session but outside any sampled interval: 26. Both are excluded from all rates.

### Part 2: follow and chat relation

Exclusive categories (1,492 follows):

| Category | n | share |
|---|---|---|
| Chatted only before follow | 109 | 7.3% |
| Chatted before and after | 395 | 26.5% |
| Chatted only after follow | 458 | 30.7% |
| Never chatted (in chat table) | 530 | 35.5% |

- Chatted before follow (504 total, 33.8%): minutes from first chat to follow median 5.0 (p25 0.8, p75 40.9); 302 (59.9%) within 10 minutes; 394 (78.2%) within 60 minutes.
- Broadcaster reply within 3 minutes of the first message: 68 of 504 chat-before followers (13.5%). Across the 10,752 first-chat events whose 24-hour window is clear of the gap and the cutoff (58 streamers; one event per viewer and broadcast, overlapping reconnect sessions merged, which drops 22 second events), 675 (6.3%) got such a reply.
- Follow within 60 minutes of first chat: 9.3% with a reply (n 675) vs 3.4% without (n 10,077), ratio 2.8. Within 24 hours: 9.6% vs 3.8%, ratio 2.6. Across all these first-chat events the 60-minute rate is 3.8%. An earlier version also counted the 1,343 first messages whose window touches the gap or the cutoff as non-follows (9.3% vs 3.0%).

### Part 3: conversion and within-streamer session features

Conversion (aligned follows / tracked viewer-hours, same intervals):
- 1,252 follows / 44,546 viewer-hours: **2.81 per 100 vh**. Exposure in the follow collection gap (2026-03-15 18:00 to 2026-06-16 16:45 UTC) is excluded, and the window ends at 2026-10-25 17:28 UTC.
- twitch_follow_events has no rows between 2026-03-15 17:56:29 and 2026-06-16 16:45:01 UTC, while chat is still recorded in that span. Treated as a collection gap (see Caveats).
- Monthly:

| Month | Follows | Viewer-hours | per 100 vh | Note |
|---|---|---|---|---|
| Feb | 10 | 190 | 5.3 | tracking starts |
| Mar | 94 | 1,911 | 4.9 | through 03-15 18:00 |
| Jun | 38 | 1,249 | 3.0 | from 06-16 16:45 |
| Jul | 212 | 6,484 | 3.3 | |
| Aug | 224 | 10,029 | 2.2 | |
| Sep | 225 | 12,275 | 1.8 | |
| Oct | 449 | 12,408 | 3.6 | through 10-25 17:28 |

Within-streamer comparisons (follower_delta from March 2026):
- 22.0% of sessions gained at least one follower; mean clipped delta 0.39; 5.2% of sessions had a negative delta.

| Comparison | Streamers | Sessions | Median diff in gain share | Share of streamers positive |
|---|---|---|---|---|
| Incoming raid vs none | 83 | 4,896 | +0.143 | 67% |
| Duration >= 4 h vs < 4 h | 178 | 6,525 | +0.140 | 67% |
| High vs low chat activation | 156 | 3,645 | +0.132 | 59% |
| Deadlock flag yes vs no | 44 | 3,609 | +0.038 | 54% |
| Start 18-22 h vs other | 210 | 6,972 | +0.000 | 32% |
| Start 12-17 h vs other | 211 | 6,772 | +0.000 | 36% |
| Start 23-05 h vs other | 111 | 4,592 | +0.000 | 33% |
| Shared chat active | 0 | 0 | no support |

Size class split (median diff in gain share; streamers, sessions, share positive):

| Comparison | 0-4 viewers | 5-19 viewers | 20-99 viewers |
|---|---|---|---|
| Raid vs none | +0.115 (75, 3,702, 70%) | +0.250 (21, 698, 76%) | +0.409 (2, 56, 100%, too few) |
| Duration >= 4 h | +0.125 (139, 4,933, 67%) | +0.159 (47, 1,166, 70%) | +0.041 (8, 151, 50%) |
| High chat activation | +0.068 (116, 2,712, 54%) | +0.250 (35, 674, 74%) | +0.070 (6, 78, 50%) |

Per hour (clipped delta per hour, within streamer): duration >= 4 h +0.014 (178 streamers, 55% positive); raid +0.030 (83, 57%); chat activation +0.020 (156, 56%). Small effects per hour.

Title tokens: 584 tokens tested; 181 had at least 5 streamers in both states. The largest shifts rest on 5 to 8 streamers and no token holds up across the tested set. No title finding.

### Part 4: subscriptions and chat

Session-joined events (1,407 total):

| Event | n | Median minute into stream | Chat msgs in 10 min before (median) | After (median) | Session baseline per 10 min (median) | After / baseline (median ratio, n) |
|---|---|---|---|---|---|---|
| Paid subscribe | 181 | 111.3 | 4 | 9 | 5.6 | 1.76 (159) |
| Gifted subscribe (recipient) | 181 | 169.9 | 4 | 12 | 15.4 | 1.79 (126) |
| Resub | 352 | 86.7 | 2 | 7 | 7.8 | 1.48 (276) |
| Gift event (incl. 40 bomb summary rows) | 631 | 124.3 | 7 | 16 | 8.5 | 1.61 (555) |

- Tiers: subscribe events all tier 1000; resub tiers 1000 (346), 2000 (2), 3000 (4). Gift-bomb summary rows are gift rows without a user login and with total_gifted >= 2.
- Resub streaks (124 events with a streak value): streak 1: 27; streak 2: 22; streak 3 or more: 75 (60.5%). Median cumulative months 11.

Recipients (Mar to Sep 25 window; chat within 30 days; later resub anywhere):

| Group | n | Streamers | Chatted within 30 days | Resub later |
|---|---|---|---|---|
| Gifted recipients | 166 | 8 | 24 (14.5%) | 0 (0.0%) |
| New paid subscribers | 132 | 32 | 100 (75.8%) | 49 (37.1%) |

Caveat for the 0: gifted subscriptions do not auto-renew on Twitch (expected behaviour, not verified in these tables). 57 resub events have no session id and cannot be joined, so later resubs are undercounted.

The 10-minute chat windows have no matched control. The after/baseline ratio is descriptive: subscriptions happen during active moments.

### Part 5: community points before follow or first chat

Covered follows (follow day >= 2026-08-20, so 7 full days of lookback exist; 893 follows, 49 channels):

| Measure | n | Share |
|---|---|---|
| Watch time > 0 in the 7 days before follow | 60 | 6.7% |
| Chat > 0 in the 7 days before follow | 19 | 2.1% |
| Watch time > 0 on the follow day | 503 | 56.3% |
| Watch time > 0 in the 7 days after follow | 614 | 68.8% |

First chat (covered window):
- Chatters before follow: 307. Watch > 0 in the 7 days before first chat: 10 (3.3%). Watch > 0 in the 7 days before follow: 32 (10.4%).
- Non-chatters (586): watch > 0 in the 7 days before follow: 28 (4.8%).

Community points watch is rare before follow. The follow-day watch figure is contemporaneous, not predictive.

### Lurkers (bot-excluded)

- Follows since 2026-02-19 (bots excluded): 1,362. With at least one sampled session before follow: 1,181 (median 24 sampled sessions before).
- At least one silent sighting before follow: 153 of 1,181 (13.0%). Before the bot exclusion the figure was 15.7%.
- Silent sessions before follow: 0 for 1,028 (87.0%); 1 for 88 (7.5%); 2-4 for 50 (4.2%); 5 or more for 15 (1.3%).
- Among the 153 with a silent sighting: chatted before follow in the chat table 67 (43.8%); chatter row before follow 81 (52.9%); silent only (no chat before follow) 86 (56.2%).
- Among the 1,028 without a silent sighting: chatted before follow 349 (33.9%).
- Presence level, sampled broadcasts whose span plus 60 minutes ends by the cutoff and does not touch the follow gap (2,007 broadcasts, 58 streamers; overlapping sessions of one streamer are merged into one broadcast over every session since 2026-01-25 (statement 20), a viewer counts once per broadcast and as a chatter if any of its rows has messages >= 1): silent presences 24,364, followed within the broadcast plus 60 minutes: 248 (10.2 per 1,000). Chatter presences 8,079: 659 (81.6 per 1,000). Chatters convert about 8 times as often per presence. An earlier version kept the sessions in the gap span in the denominators (7.95 vs 73.8 per 1,000), and the version before the broadcast merge counted a viewer once per overlapping session (2,078 sessions, 10.1 vs 81.2 per 1,000); a merge over the lurker set alone missed overlaps with 44 sessions outside it (2,008 broadcasts, 10.1 vs 81.3).

Late-sampling check (orchestrator expected about 25% after minute 15): not reproduced. Measured: silent last_seen minute 94.7% after minute 15; first silent sighting 51.8%; repeated-timestamp first poll 94.6%; chatter first messages 73.5%. last_seen_at is the final sighting, not a poll log, so first-sighting estimates vary.

### Actionable tips

1. (Medium confidence) Greet and reply to first-time chatters within minutes. Follow within 60 minutes: 9.3% after a broadcaster reply within 3 minutes vs 3.4% without (n 675 vs 10,077). Association only; active streams get more replies.
   Orchestrator note: downgraded to an association. V7 in `VERIFY-ORCHESTRATOR.md` respects the order of message, reply and follow: most quick follows in the reply arm come before the broadcaster message (38 of 52), so the raw gap is largely the alert thank-you. Measured from the reply on, a smaller lift remains: +2.3 pp within 60 minutes for a first message in a broadcast (CI -0.3 to +4.6) and +12.1 pp for a first message ever in the channel (CI +4.7 to +17.3, 78 replies); the placebo is null. Answering a first line soon is a likely-helpful habit, not a measured lever.
2. (Medium confidence) Use incoming raids as a conversion moment. Follows in the 10 minutes after a raid: 7.9 per 100 vh vs 2.7 outside (63 follows, 802 vh). Raids overlap with peaks, so this is not isolated.
3. (Medium confidence) Chat activity and raids go with more follower gains in 5 to 19 viewer streams (raid +0.25, 21 streamers, 76% positive; high chat activation +0.25, 35 streamers, 74% positive). Weak in 0 to 4 viewer streams (chat +0.07, 54% positive).
4. (Medium confidence) Silent viewers rarely convert on their own: 10.2 follows per 1,000 silent presences vs 81.6 for chatters. Bring viewers into chat with direct questions; chat is the step that precedes follows.
5. (Low confidence) Gifted recipients rarely chat (14.5% within 30 days, n 166 from 8 streamers) vs new paid subscribers (75.8%, n 132 from 32 streamers). A personal welcome in chat may help; small cohort.
6. (Low confidence) Stream length adds followers in absolute terms (gain share +0.14 for 4 h or more, 178 streamers), but per hour the effect is near zero (+0.014, 55% positive).

Not actionable (null or unsupported): start time bands (median diff 0, 32 to 36% positive), Deadlock flag (+0.04, 54% positive), title tokens, shared chat (no support).

## Caveats and dead ends

- Session clock: the stored `twitch_session_viewers.minutes_from_start` is stuck at 0 for 314 sessions from 2026-06-22 to 2026-07-03, a collector fault. All stream minutes here come from `ts_utc - started_at`. An earlier version binned by the stored field at the sample start and showed 1,864 vh and 5.1 per 100 vh in minutes 0 to 10 (now 476 vh and 12.6).
- Follow collection gap: no follow events between 2026-03-15 17:56:29 and 2026-06-16 16:45:01 UTC, while chat is still recorded (April 6,229 and May 6,333 messages from 27 and 25 streamers, against 32,497 in March). Treated as a gap, not zero conversion. Exposure from 2026-03-15 18:00 to 2026-06-16 16:45 UTC is dropped from every rate, and so is every first message or session presence whose follow window touches that span, so March and June are partial months. A viewer who followed inside the gap has no follow row and counts as not yet following afterwards.
- twitch_follow_events.id is not unique (1,476 distinct ids of 1,492 rows). All joins keyed on (streamer, follower, followed_at) with row_number() keys.
- Raw follow rows since 2026-01-01: 1,494. The analysis set is 1,492; the 2-row difference was not investigated.
- Rate consistency: the minute, band, raid, peak and monthly tables come from one set of exposure intervals and sum to the same 1,252 follows and 44,546 viewer-hours (up to rounding). Earlier versions of this file combined the 1,278 mapped follows with exposure that included April, May and the partial months around the gap; the figures here replace them. The version before the disjoint clip counted reconnect overlap twice (44,633 viewer-hours, 4.7 per 100 vh in March, 12.4 in minutes 0 to 10).
- Tracked exposure starts at each streamer's first follow event (t0), so that first follow is counted in the numerator with exposure starting at it. Excluding the first follow per streamer: 2.71 instead of 2.81 per 100 vh (45 of 63 first follows fall inside an exposure interval).
- Peak timing is an ex-post sample maximum, so follows near the peak are partly selection. Raids co-occur with peaks; the raid effect is not separated from the peak.
- Per-band rates above 50 viewers rest on 17 follows (50-99) and 1 follow (100-299). Poisson relative SE is about 24% at 17 follows and 100% at 1 follow.
- Top 5 streamers hold 48.6% of follows.
- Session chat fields in twitch_stream_sessions are zero from March to May. Per-session chat baselines from session features are unreliable: only 40.5% of sessions of 10 minutes or more have chat > 0. Activation uses the chat table instead.
- Within-streamer gain comparisons are associations; good days and raid-worthy streams may cause both.
- Shared chat: the partner_effort_shared_chat join returned 0 matches; no sessions with support.
- Title tokens: no finding. 584 tested, 181 with 5 or more streamers in both states, no correction for multiple tests.
- Subscription 10-minute chat: no matched control; after/baseline ratios of 1.5 to 1.8 are descriptive only.
- Subscription event type 'end' (62 session-joined rows): meaning not verified, excluded from analysis.
- 57 resub events without session id cannot be joined; later resub rates are undercounted.
- Community points: coverage starts 2026-08-13 in 49 channels; no matched baseline computed; watch before follow is 6.7%, which may reflect coverage rather than behaviour.
- Late-sampling claim (about 25% after minute 15) not reproduced; measured values listed above. Orchestrator note: these values measure when individual logins are sighted (last_seen, first sighting), not when the session's sampling starts. The session-level check (first sample after minute 15 in 24 to 25 % of sessions, VERIFY-ORCHESTRATOR.md V1) stands.
- Operational finding (no values): /tmp/tq.sh passed the DSN on the psql command line while this package ran. Orchestrator note: fixed on 2026-10-25; the wrapper now passes credentials through PG* environment variables only.

## Reproduction

- SQL: queries-P08.sql in this folder (17 statements, then the follow gap query, q2f and the session spans for the broadcast merge as statement 20). q2f, the aligned exposure query, is the source of every rate in Part 1 and Part 3 and supersedes the two earlier exposure statements. Run it once per month via `/tmp/tq560.sh --csv -v m1=<month start> -v m2=<next month start> -v cut='2026-10-25 17:28:00+00' -v g1='2026-03-15 18:00:00+00' -v g2='2026-06-16 16:45:00+00'`; the rest via /tmp/tq.sh, read-only, 180 s timeout.
- Post-processing: python3 -I stdlib scripts in /tmp/reichweite-scratch/P08/ (a1c_timing_tracked.py, a2b_peak_raid.py, a3b_lurker_nobots.py, a4_conversion.py, a5_conversion2.py, a6_titles_perhour.py, f1_numbers.py, f2b_numbers.py, f3_numbers.py, f4_cp_covered.py, f5_sub_numbers.py, f6_checks.py, ) on the CSV extracts in the same folder. The aligned rates come from the committed `p08-aligned.py`: `python3 -I p08-aligned.py <export dir> aligned6_` reads the q2f exports `aligned6_<YYYY-MM-DD>.csv` (one per month, June in five pieces; the month comes from the file name) and `follows_mapped.csv` (statement 1). Part 2 first-chat rates and the lurker presence rates come from the committed `p08-chat-follow-rates.py`: `python3 -I p08-chat-follow-rates.py <statement 10> <statement 20> <statement 13> <statement 14> <statement 12> <statement 6>` (CSV exports of those statements; statement 10 applies the gap rule in SQL, the script applies it to the presence windows).
- Scratch CSVs are not committed (they hold chatter and streamer identifiers).
