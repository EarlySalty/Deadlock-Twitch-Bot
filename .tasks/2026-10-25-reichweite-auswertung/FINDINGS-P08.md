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
| twitch_follow_events | follows since 2026-01-31 (analysis set) | 1,492 events, 63 streamers; 1,278 mapped to a tracked session, 214 outside any session |
| twitch_stream_sessions / session viewer samples | tracked sessions since 2026-02-19 (lurker set) | 3,009 sessions, 2,855 sampled |
| twitch_session_chatters | lurker and chatter rows | 53,996 presence rows (33,226 silent and 10,312 chatter rows in sampled sessions) |
| twitch_chat_messages | chat since 2026-01-31 | monthly volume Feb 30,865; Mar 32,497; Apr 6,229 (27 streamers); May 6,333 (25 streamers); Jun 16,198; Jul 50,906; Aug 77,331; Sep 94,805; Oct 151,545 |
| session features (follower_delta etc.) | sessions from 2026-03-01 | 13,539 sessions, 1,734 streamers; 9,488 sessions from 1,207 streamers with follower_delta |
| twitch_subscription_events | session-joined events, Feb to Oct 2026 | 1,407 events, 71 streamers |
| twitch_community_points_viewer_daily | 49 channels, 2026-08-13 to 2026-10-25 | 14,087 viewer-days, 6,449 viewers |

Bot-like logins: 77 logins seen in 30+ distinct channels in twitch_session_chatters (excluded from lurker baselines). They account for 120 of 1,482 follow events since 2026-02-19.

## Method

- Follow to session mapping: session with started_at <= followed_at <= coalesce(ended_at, started_at + duration); ties broken by latest start, then shortest duration (29 follows overlapped two sessions).
- Exposure: viewer-seconds from sample gaps capped at 300 s, per stream minute and per viewer band. Conversion = follows / viewer-hours.
- Tracked exposure starts at each streamer's first follow event (t0), so only follow-tracked streamers count.
- Within-streamer comparisons: for each streamer, feature sessions vs other sessions. Reported: median of per-streamer differences, share of streamers positive, number of streamers and sessions. Size classes 0-4, 5-19, 20-99, 100+ average viewers.
- Outcome for session comparisons: gain share (follower_delta >= 1), mean of follower_delta clipped to [-5, 50], and per hour. follower_delta is discrete (median 0, p95 2), so raw median differences are 0.
- Lurker: a silent sighting is a chatters-API row with messages = 0; a chatter is messages >= 1. A follower counts as having a silent sighting before following if a sampled session started before the follow and the last silent sighting was before the follow.
- Subscription chat windows: messages (excluding the streamer) in the 10 minutes before and after each event, plus a per-session 10-minute baseline (session chat_msgs / duration).
- Times: Berlin hours via a manual DST rule.

## Results

### Part 1: follows and live sessions

Tracked exposure and follows by stream minute (per 100 viewer-hours; n = follows):

| Stream minute | n | per 100 vh |
|---|---|---|
| 0-10 | 62 | 3.0 |
| 10-20 | 64 | 5.7 |
| 20-30 | 58 | 4.5 |
| 30-45 | 95 | 3.2 |
| 45-60 | 83 | 5.2 |
| 60-90 | 123 | 2.4 |
| 90-120 | 112 | 2.3 |
| 120-180 | 186 | 2.0 |
| 180-240 | 132 | 1.7 |
| 240-360 | 126 | 1.3 |
| 360+ | 237 | 2.2 |

Viewer band at follow (tracked exposure):

| Viewers | n | per 100 vh |
|---|---|---|
| 0-4 | 417 | 2.7 |
| 5-9 | 374 | 2.25 |
| 10-19 | 292 | 2.3 |
| 20-49 | 151 | 1.6 |
| 50-99 | 18 | 3.9 |
| 100-299 | 1 | 0.1 |
| 300+ | 0 (757 vh) | 0.0 |

- Follows mapped to sessions: median 129.7 minutes into the stream; 7.5% in the first 15 minutes; 71.7% after minute 60.
- Viewers at follow: median 7.0; 33.1% at 4 or fewer.
- Follows with an incoming raid earlier in the same session: 264 of 1,278 (20.7%); with a raid in the last 10 minutes: 63 (4.9%).
- Raid window (10 minutes after an incoming raid): 63 follows, 939 viewer-hours, 6.7 per 100 vh, against 2.2 per 100 vh outside (1,215 follows, 55,059 vh).
- Peak timing (ex-post sample maximum): median follow 3.6 minutes before the session peak; 14.1% within +-5 minutes. Bin [-5, 0) relative to peak (0 to 5 minutes before): 109 follows, 9.5 per 100 vh; bin [0, 5) (0 to 5 minutes after): 69 follows, 3.6 per 100 vh; bin [5, 10): 27 follows, 1.9 per 100 vh.
- Offline follows (no session covers the time): 214 (14.3%). Excluded from rates.

### Part 2: follow and chat relation

Exclusive categories (1,492 follows):

| Category | n | share |
|---|---|---|
| Chatted only before follow | 109 | 7.3% |
| Chatted before and after | 395 | 26.5% |
| Chatted only after follow | 458 | 30.7% |
| Never chatted (in chat table) | 530 | 35.5% |

- Chatted before follow (504 total, 33.8%): minutes from first chat to follow median 5.0 (p25 0.8, p75 40.9); 302 (59.9%) within 10 minutes; 394 (78.2%) within 60 minutes.
- Broadcaster reply within 3 minutes of the first message: 68 of 504 chat-before followers (13.5%). Across all 12,117 first-chat events, 719 (5.9%) got such a reply.
- Follow within 60 minutes of first chat: 9.3% with a reply (n 719) vs 3.0% without (n 11,398), ratio 3.1. Within 24 hours: 9.6% vs 3.4%, ratio 2.8. Across all first-chat events the 60-minute rate is 3.4%.

### Part 3: conversion and within-streamer session features

Conversion (follows / tracked viewer-hours):
- Online follows (1,278) / online tracked viewer-hours without April and May (55,983 - 7,422 = 48,561): **2.63 per 100 vh**. Including April and May: 2.28 per 100 vh.
- April and May have no follow rows at all, while chat is still recorded for them. Treated as a collection gap (see Caveats).
- Monthly (all follows incl. offline / tracked vh): Feb 16.6 (157 vh, tracking start); Mar 3.0; Jun 2.0; Jul 3.7; Aug 2.6; Sep 2.2; Oct 4.0 (partial month, through 2026-10-25).

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
- Presence level, sampled sessions: silent presences 31,336, followed within session plus 60 minutes: 249 (7.95 per 1,000). Chatter presences 8,953: 661 (73.8 per 1,000). Restricted to sessions with at least one silent row: chatters 75.0 per 1,000 (659 of 8,784). Chatters convert about 9 times as often per presence.

Late-sampling check (orchestrator expected about 25% after minute 15): not reproduced. Measured: silent last_seen minute 94.7% after minute 15; first silent sighting 51.8%; repeated-timestamp first poll 94.6%; chatter first messages 73.5%. last_seen_at is the final sighting, not a poll log, so first-sighting estimates vary.

### Actionable tips

1. (Medium confidence) Greet and reply to first-time chatters within minutes. Follow within 60 minutes: 9.3% after a broadcaster reply within 3 minutes vs 3.0% without (n 719 vs 11,398). Association only; active streams get more replies.
   Orchestrator note: withdrawn as a follow lever. V7 in `VERIFY-ORCHESTRATOR.md` shows 52 of 57 quick follows in the reply arm fall inside the same 3 minutes (the follow alert prompts the thank-you). Without them, within channel: -0.9 pp (-1.6 to -0.2).
2. (Medium confidence) Use incoming raids as a conversion moment. Follows in the 10 minutes after a raid: 6.7 per 100 vh vs 2.2 outside (63 follows, 939 vh). Raids overlap with peaks, so this is not isolated.
3. (Medium confidence) Chat activity and raids go with more follower gains in 5 to 19 viewer streams (raid +0.25, 21 streamers, 76% positive; high chat activation +0.25, 35 streamers, 74% positive). Weak in 0 to 4 viewer streams (chat +0.07, 54% positive).
4. (Medium confidence) Silent viewers rarely convert on their own: 7.95 follows per 1,000 silent presences vs 73.8 for chatters. Bring viewers into chat with direct questions; chat is the step that precedes follows.
5. (Low confidence) Gifted recipients rarely chat (14.5% within 30 days, n 166 from 8 streamers) vs new paid subscribers (75.8%, n 132 from 32 streamers). A personal welcome in chat may help; small cohort.
6. (Low confidence) Stream length adds followers in absolute terms (gain share +0.14 for 4 h or more, 178 streamers), but per hour the effect is near zero (+0.014, 55% positive).

Not actionable (null or unsupported): start time bands (median diff 0, 32 to 36% positive), Deadlock flag (+0.04, 54% positive), title tokens, shared chat (no support).

## Caveats and dead ends

- April and May 2026: no follow events at all, while chat is still recorded (6,229 and 6,333 messages from 27 and 25 streamers, against 32,497 in March). Treated as a follow collection gap, not zero conversion. Excluded from the conversion denominator (7,422 viewer-hours).
- twitch_follow_events.id is not unique (1,476 distinct ids of 1,492 rows). All joins keyed on (streamer, follower, followed_at) with row_number() keys.
- Raw follow rows since 2026-01-01: 1,494. The analysis set is 1,492; the 2-row difference was not investigated.
- Exposure: the monthly files sum to 55,993 viewer-hours vs 55,983 in the mapped total (10 vh difference, not investigated).
- Tracked exposure starts at each streamer's first follow event (t0), so that first follow is counted in the numerator with exposure starting at it. Excluding the first follow per streamer: 2.53 instead of 2.63 per 100 vh (51 of 63 first follows are online).
- Peak timing is an ex-post sample maximum, so follows near the peak are partly selection. Raids co-occur with peaks; the raid effect is not separated from the peak.
- Per-band rates above 50 viewers rest on 18 follows (50-99) and one streamer (100+). Poisson relative SE is about 13% at 62 follows per bucket and larger for smaller buckets.
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

- SQL: queries-P08.sql in this folder (16 statements plus the bot-login query). Run via /tmp/tq.sh, read-only, 180 s timeout.
- Post-processing: python3 -I stdlib scripts in /tmp/reichweite-scratch/P08/ (a1b_timing_tracked.py, a2_peak_raid.py, a3b_lurker_nobots.py, a4_conversion.py, a5_conversion2.py, a6_titles_perhour.py, f1_numbers.py, f2_numbers.py, f3_numbers.py, f4_cp_covered.py, f5_sub_numbers.py, f6_checks.py) on the CSV extracts in the same folder.
- Scratch CSVs are not committed (they hold chatter and streamer identifiers).
