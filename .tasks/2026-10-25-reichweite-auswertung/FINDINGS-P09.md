# P09: Raids, shared chat and network entry points

Saved by the orchestrator from the P09 agent's final report (the agent's Write call was rejected).
Queries: `queries-P09.sql`.

Status: analysis complete on the data exported 2026-10-25 between 18:35 and 18:41 UTC (read-only, production DB). Scratch CSVs and scripts are in `/tmp/reichweite-scratch/P09/`, not committed, except the follow post-processing in `p09-raid-follows.py`. All queries are in `queries-P09.sql` in this folder. No channel logins, IDs, chatter names or chat quotes appear here.

## Strongest findings

- Incoming raids give a small, measurable, short-lived viewer gain. Net of same-channel controls, the channel holds 0.114 extra viewers per viewer sent at +60 min (CI 0.044 to 0.205, n=346 raids). For targets with 1-4 viewers at raid time the figure is 0.185 (CI 0.107 to 0.298, n=257).
- Raids bring new chatters within the hour: +0.115 per raid within 60 min (paired, CI 0.035 to 0.192, n=425).
- Raids bring silent new viewers: +1.02 per raid within 60 min (CI 0.74 to 1.31, n=391). They return later at the organic rate (22.2 vs 20.9 percent in controls), so no extra loyalty.
- Raids bring no measurable follow gain: +0.037 follows per raid within 60 min against matched controls (CI -0.018 to +0.091; 0.107 vs 0.070; 22 vs 27 follows; 205 raids with 356 controls in 19 follow-tracked channels, follow gap excluded).
- A raided channel does not keep a higher level in later streams (null: median per-streamer -0.047, n=116).
- Shared chat shows no viewer effect (null). Its chatter effect is weak and not robust.
- The earlier "0.45 percent of weekly viewer minutes" does not reproduce as one number. Estimates range from 0.01 to 0.97 percent depending on the denominator.

## Question

Where do new viewers and chatters come from, and how much do incoming raids, outgoing raids, shared chat and the network bring? Specifically: (1) incoming raid retention, new chatters and follows against a same-channel control, raid size vs target size, raid timing and later levels; (2) share of first-time chatters and follows within 30 min after a raid; (3) reciprocity and own growth of outgoing raids; (4) shared chat sessions with vs without; (5) network members vs others (chatter overlap and new chatters known from other network channels). Also re-check the earlier 0.45 percent finding.

## Data used

| Source | Scope | Rows / counts used |
|---|---|---|
| `twitch_raid_history` | raids since 2026-02-01, before 2026-10-25 17:00 | 2,223 rows, 2,044 successful, 2,090 distinct ids; 117 successful ids reused at different times, so the key is (id, executed_at) |
| `twitch_raid_retention` | joined to successful raids only | 2,010 rows, 151 on unsuccessful raids (excluded) |
| `twitch_stream_sessions` | sessions since 2026-01-09 | 14,980 sessions in export (14,981 live), 1,869 channels; 1,303 of 14,981 have NULL user ID and are matched by login |
| `twitch_session_viewers` | viewer curve per session, time-filtered | samples about every 16 s; used at raid time, +15, +30, +60 min and as 60-min average |
| `twitch_session_chatters` | sessions since 2026-01-09 | 164,821 rows, 9,153 sessions; since 2026-02-01: 34,572 chatter rows (messages >= 1) and 130,232 lurker rows (messages = 0, 79.0 percent) |
| `twitch_follow_events` | 2026-02-15 to 2026-10-25 | 1,492 events, 63 channels |
| `category_chat_messages` | shared-chat proxy since 2026-09-27 | 2,914 sessions with user ID, 108 with shared-chat copies, 70 channels with copies, 51 with both session types |
| `partner_effort_shared_chat_observations` | intended shared-chat source | 0 rows (empty, not usable) |
| `twitch_streamers`, `twitch_partners`, `twitch_raid_auth` | network membership | 99 unique logins (261 source rows); raid auth 80 rows, 68 enabled |

Time windows: raids since 2026-02-01; lurker rows since 2026-02-19 (silent-viewer windows since 2026-02-20); shared chat since 2026-09-27. Diagnostics run after the export show the live tables have grown slightly (for example 164,856 session-chatter rows since 2026-02-01 vs 164,804 at export). All findings use the export snapshot.

## Method

**Raid sample and controls.** Successful raids into tracked channels since 2026-02-01. Each raid is compared with the same channel at the same clock time 7 and 14 days before and after. A control is valid only if the channel was live for 60 min at control time with fresh viewer samples and no other successful raid into that channel within ±60 min. Raid and control rows are keyed by (raid id, executed_at), and controls are mapped back to their source raid.

**Viewer net retention.** For each raid, net_X = (viewers at +X min minus viewers at raid time) minus the mean of the same quantity over the valid controls. Retention is pooled: sum of net_X divided by sum of viewers sent. Bootstrap CIs use 2,000 resamples of raids (1,000 for subgroups). Targets with 0 viewers at raid time are excluded from the ratio. Viewer-minute uplift = (60-min average viewers minus control average) × 60.

**Chatters.** A chatter is a row with messages >= 1. New chatter = first tracked message in the channel, counted within the window. Silent new viewer = lurker row (messages = 0) first seen in the 60-min window with no message in that window (since 2026-02-20). Silent return = lurker seen again in a later session of the same channel. Outcomes are paired per raid against the valid controls.

**Follows.** Follow events within 30 and 60 min after the raid vs the same windows in the valid controls. Only raids and controls since 2026-02-15 in channels with follow tracking count, and only from the channel's first recorded follow on; a raid in a channel without tracking is not a zero. Follow outcome windows follow one rule: a unit with window [a, b) is kept only if b <= 2026-10-25 17:28 UTC and the window does not overlap the follow gap from 2026-03-15 18:00 to 2026-06-16 16:45 UTC. The window share uses tracked live time since 2026-02-15 with the gap removed; sessions without `ended_at` end at start plus duration. The control comparison is matched: each control row is mapped to its source raid by (raid id, control time minus 7 × k days), both arms pass the same unit filter (60 min covered, no other raid within ±60 min, follow-tracked channel, gap rule), and a raid counts only with at least one valid control. Per raid the outcome minus the mean of its controls; reported is the mean of these differences with a bootstrap 95% CI over raids (2,000 resamples, seed 7).

**Shared chat.** Session-level comparison of channels with and without shared-chat copies, as per-streamer medians of differences. Chatter counts are recomputed from raw chat (distinct non-shared chatters with messages), not from the session chat fields (see caveats).

**Network and reciprocity.** Network = the 99 member logins. Chatter overlap per channel (channels with >= 5 chatters). Outgoing and incoming raids counted for 97 network channels. Growth = log ratio of median session average viewers in the first 28 days since 2026-02-01 vs the last 28 days, for channels with >= 3 sessions in both.

**Next-session level.** Average viewers of the next session (within 7 days after a session), divided by the streamer's median of the 28 days before. Compared after raided vs non-raided sessions per streamer.

## Results

### 1. Incoming raids

Funnel:

| Step | Raids |
|---|---|
| successful raids since 2026-02-01 | 2,044 |
| with a target session | 1,876 |
| session covers 60 min after raid | 1,389 |
| fresh viewer samples | 1,335 |
| no other successful raid within ±60 min | 870 |
| target viewers > 0 at raid time | 835 |
| with valid same-channel controls | 346 |

Net viewer retention per viewer sent (pooled; CI from bootstrap):

| Group | n raids | sent per raid | +15 min | +30 min | +60 min |
|---|---|---|---|---|---|
| All | 346 | 4.31 | 0.180 [0.109; 0.265] | 0.134 [0.065; 0.216] | 0.114 [0.044; 0.205] |
| Target 1-4 viewers | 257 | 4.32 | 0.193 [0.113; 0.292] | 0.191 [0.115; 0.297] | 0.185 [0.107; 0.298] |
| Target 5-19 viewers | 78 | 4.27 | 0.113 [-0.027; 0.268] | -0.077 [-0.280; 0.116] | -0.065 [-0.284; 0.165] |
| Target 20+ viewers | 11 | 4.36 | 0.347 [-0.583; 1.294] | 0.288 [-0.507; 1.329] | -0.295 [-1.291; 0.480] |

The 20+ class is too small to interpret. The clean set has no targets with 100+ viewers.

Raid size relative to target viewers (sent / target viewers at raid time):

| Ratio | n raids | sent per raid | +15 min | +30 min | +60 min |
|---|---|---|---|---|---|
| < 0.5 | 86 | 1.59 | -0.073 [-0.458; 0.353] | -0.213 [-0.693; 0.272] | -0.231 [-0.830; 0.328] |
| 0.5-1 | 131 | 2.15 | 0.206 [0.084; 0.336] | 0.080 [-0.074; 0.226] | 0.150 [0.019; 0.301] |
| 1-2 | 28 | 4.61 | 0.273 [0.075; 0.482] | 0.209 [0.011; 0.417] | 0.192 [0.010; 0.414] |
| >= 2 | 101 | 9.35 | 0.197 [0.109; 0.315] | 0.191 [0.105; 0.301] | 0.143 [0.074; 0.249] |

Raid timing (minutes into the target's stream at raid time):

| Minutes into stream | n raids | +15 min | +60 min |
|---|---|---|---|
| 0-10 | 19 | 0.269 [-0.027; 0.508] | 0.086 [-0.348; 0.383] |
| 10-30 | 46 | 0.200 [-0.026; 0.395] | 0.271 [0.058; 0.503] |
| 30-60 | 58 | 0.035 [-0.108; 0.171] | 0.153 [-0.032; 0.365] |
| 60+ | 223 | 0.195 [0.108; 0.312] | 0.090 [0.004; 0.184] |

Per target channel (46 channels): median per-streamer net +15-min retention +0.208, share of channels positive 0.54; 20 channels with >= 5 raids. Per viewer sent the figure is +0.063.

Viewer-minute uplift in the first 60 min: 14,930 across the 346 raids (3,931 for targets with 1-4 viewers).

Chatters and silent viewers (paired per raid against valid controls):

| Outcome | Window | n raids | Difference per raid | CI |
|---|---|---|---|---|
| New chatters | 30 min | 425 | +0.090 | [+0.030; +0.153] |
| New chatters | 60 min | 425 | +0.115 | [+0.035; +0.192] |
| Silent new viewers | 60 min (since 2026-02-20) | 391 | +1.019 | [+0.743; +1.305] |
| Silent viewers back in later sessions | later sessions (since 2026-02-20) | 391 | +0.334 | [+0.232; +0.447] |

Return share of silent viewers: 400 of 1,805 under raids (22.2 percent) vs 271 of 1,295 in controls (20.9 percent).

### 2. Share of first-time chatters and follows within 30 min after a raid

These raw shares are not control-adjusted and include time-of-stream effects.

| Population | N | Within 30 min after raid | Share of live minutes in raid windows | Rate in window vs organic (per 1,000 min) | Ratio |
|---|---|---|---|---|---|
| First-time chatters since 2026-02-01 | 18,389 | 549 (2.99 percent) | 1.36 percent | 10.44 vs 4.66 | 2.24 |
| Follows (tracked live time since 2026-02-15, gap excluded, 59 channels) | 1,278 | 102 (7.98 percent) | 4.95 percent | 3.90 vs 2.35 | 1.66 |

Control-adjusted follows (raids vs valid controls):

| Window | Raid per raid (events) | Control per raid (events) | Difference | n raids / n control events |
|---|---|---|---|---|
| 30 min | 0.063 (13) | 0.047 (17) | +0.016 (CI -0.026 to +0.059) | 205 / 356 |
| 60 min | 0.107 (22) | 0.070 (27) | +0.037 (CI -0.018 to +0.091) | 205 / 356 |

An earlier version reported +0.046 at 60 min on 883 raids vs 1,183 controls. It counted raids in channels without follow tracking and windows inside the follow gap as zero follows. A second version reported +0.087 at 60 min on 454 raids vs 697 controls. It compared all raids with all controls, so raids without any valid control and controls of raids that failed the unit filter entered one arm only.

### 3. Outgoing raids, reciprocity and growth

- 97 network channels: 69 send raids, 73 receive raids.
- Median incoming raids: 9 for channels that send raids, 0 for channels that do not.
- Spearman correlation outgoing vs incoming raids: 0.894. This is structural: auto-raid picks live partners, so it does not show that raiding causes being raided.
- Growth (18 channels with >= 3 sessions in both windows): channels with 6+ outgoing raids (n=16) have median log-growth +0.304 (ratio 1.36). Channels with 1-5 or 0 outgoing raids have n=1 each, too few to compare. In the stratum with median first-window viewers below 5 (n=17), the Spearman correlation between outgoing raids and growth is -0.002.

No evidence that outgoing raids drive own growth. Support is insufficient.

### 4. Shared chat (since 2026-09-27)

The shared-chat table is empty. The proxy is `shared_chat_copy` in `category_chat_messages`.

- 108 of 2,913 sessions had shared-chat copies (70 channels; 51 channels had both session types in the first pass).
- Viewers, first pass (all sessions): median per-streamer ratio shared / non-shared 1.008, share > 1 = 0.53, n=49.
- Viewers, recheck (sessions with messages > 0): median per-streamer ratio 0.947, share > 1 = 0.42, n=48.
- Chatters, recheck from raw chat (distinct non-shared chatters per session): median per-streamer difference +0.75, share positive 0.56, n=48. Mean of per-streamer differences +1.82, bootstrap CI [+0.19; +3.54].

The session-field version from the first pass (first-time chatters +1.17, unique chatters +2.50, share positive 0.63, n=51) is not used. Those fields are zero for many September and October sessions (see caveats). Copies may also bring the other channel's chatters into the room, so chatter counts can rise mechanically.

### 5. Network members vs others

Channels with >= 5 chatters:

| Group | n channels | Median share of chatters also chatting in another network channel | Pooled share |
|---|---|---|---|
| Network members | 80 | 0.249 | 0.297 |
| Others | 462 | 0.095 | 0.103 |

First-time chatters already known from another network channel: 1,104 of 5,017 (22.0 percent) in network channels vs 915 of 13,382 (6.8 percent) in others. This is descriptive: network membership and channel size are confounded.

### 6. Next-session level after raids

116 channels have both raided and non-raided sessions. Median per-streamer difference in next-session level (normalized to the streamer's own median): -0.047, share positive 0.40. Pooled: 1.229 (n=1,029 sessions after a raid) vs 1.234 (n=7,006 after non-raided sessions). No higher level after being raided.

### 7. Re-check of the earlier "0.45 percent of weekly viewer minutes"

| Measure | Value | n |
|---|---|---|
| Pooled control-adjusted 60-min uplift / weekly viewer minutes of raided streamer-weeks | 0.966 percent | 214 streamer-weeks, 367 raids |
| Median per streamer-week | 0.307 percent | 214 |
| Gross upper bound (viewers sent × 60 min) | 6.05 percent | same sample |
| Control-adjusted uplift / all tracked viewer-minutes since 2026-02-01 (114,070,889) | 0.013 percent | all tracked channels |

The earlier 0.45 percent lies inside the range of the raided-streamer-week measures (median 0.31, pooled 0.97). It depends on the denominator and on pooled vs median aggregation. The original method was not available to verify. Recommended wording: raids add a control-adjusted uplift of well under 1 percent of a raided channel's weekly viewer minutes (median 0.3 percent). Do not quote 0.45 percent as a single figure.

## Actionable tips

| # | Tip | Evidence | Effect | Support n | Confidence |
|---|---|---|---|---|---|
| 1 | For small targets (1-4 viewers), an incoming raid adds a net gain that holds for about an hour. | Net retention +60 min for 1-4 viewer targets | 0.185 per viewer sent (CI 0.107 to 0.298) | 257 raids | medium |
| 2 | Raids that are at least twice the target's viewer count keep the most viewers. | Ratio >= 2, +60 min | 0.143 (CI 0.074 to 0.249) | 101 raids | medium |
| 3 | Raids much smaller than the target showed no detectable gain. | Ratio < 0.5, +60 min | -0.231 (CI -0.830 to 0.328) | 86 raids | low |
| 4 | Raids bring a few new chatters within the hour. | Paired new chatters, 60 min | +0.115 per raid (CI 0.035 to 0.192) | 425 raids | medium |
| 5 | Raids bring silent viewers within the hour, but they return at the organic rate. | Silent new viewers, 60 min; return share | +1.02 per raid (CI 0.74 to 1.31); return 22.2 vs 20.9 percent | 391 raids | medium |
| 6 | Do not plan on follows from raids; no follow gain is measurable within the hour. | Follows vs matched control, 60 min | +0.037 per raid (CI -0.018 to +0.091; 22 vs 27 events) | 205 raids vs 356 controls, 19 channels | low |
| 7 | Do not expect a lasting level increase after being raided. | Next-session level | -0.047 median per streamer; share positive 0.40 | 116 channels | medium |
| 8 | Do not use shared chat as a viewer tool on this evidence. | Viewer ratio, shared vs non-shared (recheck) | 0.947 (share > 1 = 0.42) | 48 channels | low |
| 9 | Raid timing within the stream showed no consistent effect; do not optimise for it. | Retention by minute of stream | Only 10-30 min (+60 min) and 60+ (+15 and +60 min) have CIs above zero; no consistent pattern | 19 to 223 raids per class | low |
| 10 | Network members share a pool of already-known chatters. Treat as context, not a cause. | Chatters already known from another network channel | 22.0 vs 6.8 percent | 5,017 vs 13,382 first-time chatters | low |

Outgoing raids showed no evidence of own growth, and reciprocity is structural. No tip is given for that dimension.

## Caveats and dead ends

- The shared-chat table `partner_effort_shared_chat_observations` is empty (0 rows). The proxy covers only sessions since 2026-09-27.
- Follow coverage is 63 channels since 2026-02-15 (1,492 events). After the gap rule and the tracked-channel rule, 1,278 follows in 59 channels fall in tracked live time and the matched control comparison covers 19 channels (205 raids with at least one valid control). Follow results rest on small counts. Follows during the gap from 2026-03-15 to 2026-06-16 were not recorded, so that span is excluded.
- Lurker data starts 2026-02-19. Silent-viewer windows start 2026-02-20. Lurker rows make up 79.0 percent of session-chatter rows since 2026-02-01.
- `twitch_chatter_rollup`: 18,023 of 36,435 rows (49.5 percent) have zero messages, and `first_seen_at` counts lurkers. Not used for chatter definitions.
- Session chat fields are unreliable. `unique_chatters` is zero for every session in March (2,599 of 2,599), April (1,609 of 1,609) and May (1,455 of 1,455), and still zero for 400 of 1,428 sessions in September and 1,049 of 2,697 in October. Not used. The shared-chat chatter comparison was recomputed from raw chat.
- `follower_delta` is implausible in January and February (values from -8,149 to 6 and -8,139 to 48, with many NULLs). Not used. Follows come from `twitch_follow_events`.
- `confirmed_first_ever` is always false and is not used.
- In `twitch_raid_retention`, `new_chatters` and `new_to_target` disagree on about half the rows. Not used as a source.
- `raid_id` is not unique: 117 successful ids are reused at different times. Earlier scripts that keyed on id alone gave wrong control sets. All results use (id, executed_at).
- 1,303 of 14,981 sessions since 2026-01-09 have NULL user ID and were matched by login.
- Small numbers: median target viewers at raid time is 2. The 20+ class has 11 raids and is not interpretable.
- Selection: raiders choose targets, and auto-raid picks live partners, so raided and control channels differ in who they are. Controls match time, not target choice. Large streamers' habits and patch days are unmeasured confounders.
- Many subgroup cuts were tested. CIs are not adjusted for multiple comparisons. Subgroup results are exploratory.
- Raw population ratios (2.24 and 1.66) include time-of-stream effects. The control-adjusted increments are the better estimates.
- Null results: shared-chat viewer effect, next-session level, outgoing-raid growth, most timing effects, and reciprocity (structural only).
- The 0.45 percent figure could not be reproduced as one number (see section 7).

## Reproduction

- SQL: `queries-P09.sql` in this folder, read-only via /tmp/tq.sh. Statement 1 exports raids and controls, statement 2 the sessions, statement 4 the follow events.
- Follow results (section 2 follow row, control-adjusted follows, tip 6): `python3 -I p09-raid-follows.py <statement 1 csv> <statement 2 csv> <statement 4 csv>`. The script applies the follow gap rule and the tracked-channel rule.
