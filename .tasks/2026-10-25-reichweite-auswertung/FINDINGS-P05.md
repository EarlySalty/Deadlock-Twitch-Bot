# P05: Lurkers vs chatters

Saved by the orchestrator from the P05 agent's final report (the agent's Write call was rejected). Queries: `queries-P05.sql`.

## Bottom line

- Most viewers in every size class are lurkers. Bot-excluded lurker share is 0.70 (0-4 average viewers), 0.78 (5-19), 0.86 (20-99) and 0.87 (100+) at session level.
- Chatters per average viewer fall with size: pooled 0.83, 0.70, 0.36 and 0.13. Viewer messages per viewer-hour fall from 3.54 to 0.08 across the same classes.
- Bots inflate small-stream activity most. Excluding them lowers pooled activation for 0-4 average viewers from 0.961 to 0.828 (about 14%). Bots also flip the median ordering of the 0-4 and 5-19 classes.
- The strongest within-streamer lever is the broadcaster writing in their own chat: median +0.418 chatters per average viewer (110 streamers, 84% positive). The dose-response is also positive (rho +0.290, 139 streamers).
- Lurkers who start writing do so in livelier rooms. Their first-write session has +0.200 viewer messages per minute compared with their earlier sessions (control +0.009, n = 2,151 converting viewers).
- New chatters arrive after active stretches. The chance of a new chatter in a 5-minute slot is 0.042 after a silent slot and 0.152 after an active one.
- There is no chat-velocity threshold. Once viewer messages arrive, the chance of at least one new chatter in a 5-minute slot stays between 0.30 and 0.40 up to 6.4 messages per minute. New chatters per message fall from 0.34 to 0.04 as speed rises.
- Category-wide, rooms of 0-19 average viewers show about 0.48 to 0.50 distinct chatters per average viewer per hour. This drops to 0.29 at 20-99 and 0.13 at 100+.
- Flag for the coordinator: last-seen timing gives 92.8% of non-bot lurker rows last seen after minute 15, not the roughly 25% in the brief. Details under Caveats.

## Question

Why do some streams have mostly lurkers while others have an active chat, and what raises the share of viewers who chat?

## Data used

Window: 2026-07-01 to 2026-10-25 for all session-level analyses. Lurker rows exist from 2026-02-19, but the brief addendum states chat fields are zero for March to May 2026, so the Jul-Oct window is used throughout. Category analysis: 2026-10-09 to 2026-10-25 (category snapshots start 2026-10-09).

| Source | Scope | Rows or sessions | Streamers or channels |
|---|---|---|---|
| Stream sessions (all, Jul 1 to Oct 25) | Base table | 6,945 sessions | not counted |
| Sessions with avg > 0 and duration >= 600 s | Activation base | 6,014 sessions | 1,008 streamers |
| Sessions with at least one chatter-API row (bots excluded) | Lurker share | 5,550 sessions | not counted |
| Sessions with chat data present (has_sc flag) | Within-streamer base | 5,929 sessions | 1,005 streamers |
| Community-points viewer-days (bots excluded) | Channel-day lurker share | 12,306 viewer-days (1,786 bot viewer-days removed of 14,092) | 49 channels |
| Converting viewers (bot-excluded) | Conversion | 2,151 viewers | not counted |
| Non-converting control viewers | Conversion control | 8,968 viewers with at least 2 sightings | not counted |
| 5-minute slots (velocity) | Velocity | 240,254 slots in 6,033 sessions; 21,842 new-chatter starts; 302,678 viewer messages | not counted |
| First-write events with session grid | Empty-room base | 2,116 events; 9,812 sessions with grid | not counted |
| Category chat, raw messages, hour level | Category check | 17 days; 287,339 channel-hours (avg > 0, at least 30 live minutes); 32,099 channels; 190,173 channel-hours with any chat | 32,099 channels |

Bot list: 58 logins (57 user IDs) seen in 30 or more channels since 2026-07-01. Identifiers are kept only in scratch. Bots account for 57,800 of 366,805 viewer messages (15.8%) and 13.6% of question-labelled messages in the first count.

Monthly lurker coverage (raw, not bot-adjusted, broadcaster excluded):

| Month | Sessions | Sessions with chatter rows | Lurker rows | Chatter rows | Lurker share of rows |
|---|---|---|---|---|---|
| 2026-01 | 228 | 7 | 0 | 19 | 0.000 |
| 2026-02 | 1,214 | 413 | 196 | 3,073 | 0.060 |
| 2026-03 | 2,599 | 721 | 3,803 | 2,819 | 0.574 |
| 2026-04 | 1,609 | 392 | 3,687 | 555 | 0.869 |
| 2026-05 | 1,455 | 393 | 3,781 | 427 | 0.899 |
| 2026-06 | 929 | 469 | 5,238 | 921 | 0.850 |
| 2026-07 | 1,197 | 1,168 | 14,777 | 2,893 | 0.836 |
| 2026-08 | 1,625 | 1,592 | 29,090 | 5,503 | 0.841 |
| 2026-09 | 1,428 | 1,384 | 20,375 | 5,232 | 0.796 |
| 2026-10 | 2,697 | 2,615 | 42,655 | 11,572 | 0.787 |

Only July to October is used for findings. March to June shares are not comparable (see Caveats).

## Method

- Lurker: chatter-API row with messages = 0. Chatter: row with messages >= 1. Broadcaster excluded from both.
- Bot exclusion: logins seen in 30 or more channels are removed from chatters, messages and lurker rows. Sessions whose rows all belong to bots drop out of the lurker-share base (268 in total, 266 in the 0-4 class).
- Activation: unique chatters (broadcaster and bots excluded) per average viewer. Pooled values are sums of chatters over sums of average viewers. Size classes use average viewers: 0-4, 5-19, 20-99, 100+.
- Viewer messages per viewer-hour: (messages minus broadcaster messages) divided by (average viewers times hours).
- Conversion: discrete-time hazard per observed lurk day. Paired comparison for each converting viewer: first-write session against the same viewer's earlier lurker sessions. Control: non-converting viewers with at least 2 sightings, last sighting against earlier sightings.
- Within-streamer: for each streamer with at least 2 sessions on each side, the median difference in chatters per average viewer between sessions with and without the feature. Continuous check: Spearman per streamer with at least 6 sessions.
- Velocity: 5-minute slots; viewer messages per minute in the slot; probability of at least one new chatter (a viewer's first message in the session) in the slot; lead-lag to the next slot within the same session.
- Empty-room base: share of empty 10-minute windows (time-weighted) before a first write, compared with the same sessions' time-weighted base.
- Category: exact distinct chatters per hour from raw category chat messages, shared-chat copies excluded. Average viewers from snapshots weighted by sample seconds.

Queries are in queries-P05.sql in the task folder. Python post-filters and bot-adjusted exports are in /tmp/reichweite-scratch/P05/. The activation table comes from an12_activation.py; the lurker-size table from an11_lurker_size.py; within-streamer from an2_nobot.py and an4_nobot.py; conversion from an6_nobot.py (sections A and B only); velocity from an8_nobot.py; empty-room base from base_empty_nobot.py; category from cat_exact_nobot.py.

## Results

### 1. Chat activation

Session level, Jul to Oct 2026, 6,014 sessions with avg > 0 and duration >= 600 s:

| Size class (avg viewers) | Sessions | Streamers | Chatters per avg viewer, pooled, bots excluded | Same, with bots | Median session value, bots excluded | Viewer msgs per viewer-hour, pooled, bots excluded | Same, with bots |
|---|---|---|---|---|---|---|---|
| 0-4 | 4,523 | 855 | 0.828 | 0.961 | 0.513 | 3.54 | 4.60 |
| 5-19 | 1,225 | 241 | 0.700 | 0.775 | 0.545 | 2.49 | 2.89 |
| 20-99 | 213 | 55 | 0.360 | 0.382 | 0.381 | 1.38 | 1.49 |
| 100+ | 53 | 22 | 0.127 | 0.128 | 0.077 | 0.08 | 0.08 |

- Bot effect is largest in small rooms on pooled activation: -14% (0-4), -10% (5-19), -6% (20-99), -1% (100+).
- Median session activation is about 0.5 at 0 to 19 average viewers, then 0.38 at 20-99 and 0.08 at 100+. The pooled decline is steeper than the median decline, so part of it is a mix effect of large sessions.
- Implied seen-to-average ratio: the chatter-API seen count is roughly 2.7 times average viewers in the 0-4 class (derived from pooled lurker share and activation). Activation is a per-session accumulated count, not a concurrent-viewer share.

### 2. Lurker share and conversion

Session level, bots excluded, sessions with at least one chatter row (5,550 in total):

| Size class | Sessions | Pooled lurker share | Median session lurker share | With bots, pooled (sessions) |
|---|---|---|---|---|
| 0-4 | 4,068 | 0.697 | 0.733 | 0.738 (4,334) |
| 5-19 | 1,219 | 0.778 | 0.778 | 0.780 (1,220) |
| 20-99 | 211 | 0.861 | 0.850 | 0.859 (212) |
| 100+ | 52 | 0.867 | 0.949 | 0.867 (52) |

Finer average-viewer bins (bots excluded): [0,1) 0.671 (327 sessions); [1,2) 0.678 (1,645); [2,5) 0.705 (2,096); [5,10) 0.762 (899); [10,20) 0.801 (320); [20,50) 0.816 (148); [50,100) 0.934 (63); [100,inf) 0.867 (52).

Community-points channel-day level, bots excluded: 49 channels, 12,306 viewer-days, 7,522 lurker-days (61.1%). 0-4 class 0.581 (39 channels, 5,471 viewer-days); 5-19 class 0.671 (7 channels, 4,235); 20-99 class 0.578 (2 channels, 2,600). Support for size comparisons at channel level is too small.

Conversion (bot-excluded):

| Measure | Converting viewers | Value | Control |
|---|---|---|---|
| Paired change in viewer msgs per minute (first-write session minus own earlier lurker sessions, median) | 2,151 | +0.200 (76% positive) | +0.009 (52% positive; 8,968 viewers) |
| Same, with the viewer's own first message removed | 2,151 | +0.152 (70.5% positive) | not applicable |
| Paired change in log average viewers | 2,149 | +0.032 (53% positive) | +0.016 |
| Paired change in question share | 1,663 | +0.007 (55% positive) | +0.000 |
| Paired change in command share | 1,663 | +0.000 (39% positive) | +0.000 |

- Share of converting viewers who write within one observed lurk day: 0.734 (first run with bots: 0.702).
- Discrete-time hazard after one observed lurk day: 0.125 (102 of 815 at risk; first run: 0.121).
- Median cumulative watch time before the first message: 14 minutes (first run: 18 minutes).

Empty-room base (time-weighted):

- Empty share in the 10 minutes before a first write: 0.328 (2,116 events).
- Base for the same sessions (1,099 event sessions, time-weighted): 0.468. Unweighted session mean for the same sessions: 0.555.
- Base over all 9,812 sessions with a grid: 0.838. Event sessions are busier than typical sessions, so the event-session base is the relevant comparison.
- First writes come into less empty rooms than the base. The earlier narrative that silence deters first writes is reversed by this measure.

### 3. Within-streamer drivers

Median per-streamer difference in chatters per avg viewer (base 5,929 sessions, 1,005 streamers; bots excluded):

| Feature | Streamers | Sessions on / off | Median difference | Share of streamers positive |
|---|---|---|---|---|
| Broadcaster chatted in own chat | 110 | 988 / 1,988 | +0.418 | 0.84 |
| Any command message | 86 | 791 / 1,296 | +0.331 | 0.81 |
| Viewer question share >= 0.05 | 147 | 1,744 / 958 | +0.268 | 0.80 |
| Viewer question share >= 0.10 | 151 | 1,193 / 1,579 | +0.216 | 0.71 |
| Command share >= 0.05 | 42 | 230 / 1,149 | -0.028 | 0.45 |
| Title contains "?" | 54 | 271 / 1,511 | +0.000 | 0.43 |
| Deadlock game | 30 | 946 / 553 | -0.035 | 0.43 |
| Evening 18-24 (local, UTC+2 assumed) | 178 | 1,796 / 1,932 | +0.000 | 0.43 |
| Weekend | 192 | 1,324 / 2,773 | +0.000 | 0.42 |

Continuous checks (Spearman per streamer, at least 6 sessions):

| Predictor | Streamers | Median rho (activation) | Share positive | Median rho (viewer msgs per viewer-hour) |
|---|---|---|---|---|
| Broadcaster messages per hour | 139 | +0.290 | 0.84 | +0.301 |
| Viewer question share (sessions with >= 20 messages) | 76 | +0.039 | 0.53 | -0.053 |
| Command share (sessions with >= 20 messages) | 67 | +0.105 | 0.58 | -0.067 |
| Duration (hours) | 212 | +0.396 | 0.85 | +0.160 |
| Local start hour | 211 | -0.038 | 0.45 | +0.020 |
| Viewer chat speed (msgs per minute) | 212 | +0.800 | 0.97 | +0.980 |

Viewer messages per viewer-hour, threshold splits: broadcaster chatted +1.647 (110 streamers, 87% positive); question share >= 0.05 +2.136 (147 streamers, 88% positive, but the continuous check is null); command share >= 0.05 -0.340 (42 streamers, 31% positive).

### 4. Category-wide check

Raw category chat, exact distinct chatters per hour, bots excluded, 17 days (2026-10-09 to 2026-10-25). First (unadjusted) run in brackets.

| Size class (channel median avg viewers) | Channels | Channel-hours | Chatters per avg viewer | Viewer msgs per viewer-hour |
|---|---|---|---|---|
| 0-4 | 24,601 | 194,153 | 0.483 [0.488] | 4.71 |
| 5-19 | 5,593 | 60,810 | 0.496 [0.502] | 4.74 |
| 20-99 | 1,429 | 21,631 | 0.291 [0.292] | 2.43 |
| 100+ | 476 | 10,745 | 0.133 [0.133] | 0.71 |

- German vs other, all sizes: 0.184 (487 channels, 3,953 channel-hours) vs 0.191 (31,622 channels). No difference.
- German channels by size: 0-4 0.425 (372 channels); 5-19 0.407 (89); 20-99 0.186 (19); 100+ 0.047 (7). Support for the German 20-99 and 100+ values is very small.
- Within-channel broadcaster chat (per-channel median difference in chatters per avg viewer, hours with broadcaster messages vs hours without): all hours +0.080 (70% of 1,147 channels positive); hours with any chat +0.036 (60% of 1,083); hours with viewer chatter +0.045 (64% of 1,047). By size: 0-4 +0.241 (76% of 372 channels), 5-19 +0.089 (73% of 499), 20-99 +0.017 (58% of 194), 100+ +0.001 (51% of 82). Channel-days: +0.023 (60% of 254 channels).

Category and session figures are not directly comparable. Session activation accumulates distinct chatters over the whole session (0-4: 0.828); the category figure counts distinct chatters per hour (0-4: 0.483).

### 5. Chat velocity threshold

Probability of at least one new chatter in a 5-minute slot, by viewer messages per minute (all sessions, 240,254 slots):

| Viewer msgs per minute | Slots | P(at least one new chatter) | New chatters per message |
|---|---|---|---|
| 0 (silent) | 193,311 | 0.000 | not applicable |
| (0.2, 0.4] | 8,872 | 0.338 | 0.338 |
| (0.4, 0.8] | 12,126 | 0.300 | 0.140 |
| (0.8, 1.6] | 12,818 | 0.309 | 0.074 |
| (1.6, 3.2] | 9,075 | 0.348 | 0.048 |
| (3.2, 6.4] | 3,432 | 0.398 | 0.038 |
| > 6.4 | 620 | 0.553 | 0.082 |

- Silent slots cannot produce a new chatter by definition, so the 0 row is a construction, not a finding.
- No sharp threshold: all non-zero bins up to 6.4 per minute sit between 0.30 and 0.40. The top bin (> 6.4) is small (620 slots).
- Per-message yield falls as speed rises. Sparse rooms convert each message into a new chatter more often.
- Lead-lag, next slot (within session): broadcaster message in slot t gives P(new in t+1) 0.158 (3,074 slots) vs 0.063 otherwise. Viewer question in slot t: 0.185 (17,003) vs 0.055. Viewer messages >= 3 in slot t: 0.172 (29,620) vs 0.049. No viewer messages in slot t: 0.043 (188,606) vs 0.153 (45,615).
- Arrival pattern: 80.2% of slots follow a silent slot; 46.0% of new starts follow one. P(new start | silent previous slot) 0.042 (7,967 of 187,773) vs 0.152 after an active previous slot (7,066 of 46,448).

## Actionable tips

Orchestrator note: V10 in `VERIFY-ORCHESTRATOR.md` tests tips 1 to 3 within the same session on 5-minute slots, after dropping the first 10 minutes of tracked chat (tracked chat often starts late, P07, so untracked slots look silent and everyone present counts as a new chatter once tracking starts). A broadcaster chat message does not raise the chance of a new chatter in the next slot (+1.4 pp, CI -2.3 to +5.1; in silent slots -0.4 pp), and neither does an active chat (+0.0 pp, CI -0.7 to +0.6). The 4.2 % vs 15.2 % gap in tip 3 is mostly this artifact plus room level. Tips 1 and 3 do not hold as levers; tip 5 (no speed target) stands.

1. Chat in your own channel. Evidence: within-streamer median +0.418 chatters per avg viewer when the broadcaster chatted (110 streamers, 988 vs 1,988 sessions, 84% positive). Dose-response rho +0.290 (139 streamers). Category: +0.080 per channel (1,147 channels, 70% positive), strongest in 0-4 rooms (+0.241) and close to zero at 100+. Confidence: medium. Association only; streamers who chat are also more interactive in general.
2. In small rooms, speak to individual viewers directly. Evidence: a viewer question in a 5-minute slot is followed by a new chatter in the next slot 18.5% of the time vs 5.5% otherwise (17,003 slots). Confidence: medium for the lead-lag. Low for question share as a session-level lever: the threshold split is +0.268 (147 streamers), but the continuous check is null (rho +0.039, 53% positive).
3. Fill silent stretches. Evidence: a new chatter appears in a slot 4.2% of the time after a silent slot vs 15.2% after an active one, and 80% of slots follow a silent slot. Confidence: medium (association, not an experiment).
4. Expect lurkers to start writing when the room is already moving. Evidence: converting viewers' first-write sessions are +0.200 viewer messages per minute above their earlier sessions (control +0.009; 76% positive; n = 2,151). The effect remains +0.152 without the viewer's own message. Confidence: medium.
5. Do not chase a chat-speed number. Evidence: P(new chatter) is flat at 0.30 to 0.40 across all non-zero speed bins up to 6.4 per minute (240,254 slots). Confidence: high for the flatness. The practical lever is steady message flow, not a target rate.
6. Do not benchmark large streams against small ones. Evidence: 0.133 chatters per avg viewer at 100+ in the category check (476 channels) and 0.127 at session level (53 sessions). Confidence: high (category), medium (session).

No tip is supported for: game category (Deadlock vs other, -0.035, 30 streamers), title question mark (+0.000, 54 streamers), time of day (null), weekend (null), command share above 0.05 (-0.028, 42 streamers), or start hour (rho -0.038).

## Caveats and dead ends

- Bots: excluding them changes small-room numbers materially (0-4 activation 0.961 to 0.828). Bot messages are 15.8% of viewer messages. Bot-only sessions drop out of the lurker base (268 in total).
- Lurker share for March to June is not usable. The brief addendum states chat fields are zero for March to May 2026. June looks like a transition month. Findings use July to October only.
- Late sampling: measured, not the brief's figure. 92.8% of 90,364 non-bot lurker rows have last-seen times after minute 15 (mean last-seen fraction 0.675, median 0.762). The coordinator's figure of about 25% is not reproduced. last_seen_at is a last sighting, so lurkers who stay until the end naturally produce late values; this is not a sampling-time measure. Flagged for the coordinator.
- "Unique chatters" in the brief is a seen-viewers measure in the chatter API, not a concurrent-viewer count. Activation values depend on that definition (see Results 1).
- Chat speed as a driver is tautological. Viewer messages per minute shares its numerator with the viewer-hour outcome (rho +0.980), and activation rises with chat volume (rho +0.800). Reported as an identity-level correlation, not a driver.
- Duration as a driver is partly mechanical: distinct chatters accumulate over longer sessions (rho +0.396). Reported with this caveat only.
- Command effect is mechanical at presence level (any command message +0.331), but the share threshold is null (-0.028). Dead end as a lever.
- Question-share threshold is inconsistent between the split (+0.268) and the continuous check (rho +0.039). Not established.
- Category: the exact export is live and bot-excluded (254,860 hour rows in the export vs 253,877 in the first run). 153 hours' distinct viewers and 325 hours' messages changed from late-arriving data. Effect on size-class values is at most 0.01 (for example 0-4: 0.483 vs 0.488).
- Category rollup is not usable for distinct chatters: rollup distinct sum 2,278,717 vs exact 1,756,584 (bot-excluded), while rollup message totals agree (11,525,478 vs 11,501,570). Use raw category_chat_messages only.
- Velocity artifact: an earlier first-message anchor (first_message_at) created false windows. Fixed by anchoring on min(message_ts).
- Conversion window artifact: earlier windows were anchored to first_message_at and skipped empty windows, which produced "0 empty windows" and "median chatters in window 0" as artifacts. Those sections (an6 C and D) are superseded and not used. Empty-room values come from the corrected base only.
- Conversion self-message check: the viewer's own first message is removed before comparison. The +0.152 figure is the value after removal.
- Channel-level community-points sizes: 39, 7 and 2 channels in the three classes. No size inference.
- An item recorded earlier as a "4332 artifact" (empty first timestamp) could not be re-verified in this pass and is not used in any number above.
- Resource note: a foreign process (1.28 GB, cwd /tmp/reichweite-scratch/P06, another package) was seen running at 20:02. It is not this package's process and was not touched. Mentioned for the coordinator.

## Reproduction

- Queries in queries-P05.sql, run read-only through /tmp/tq.sh. Bot IDs are computed in SQL and never written to the task folder.
- Post-filters: Python 3 stdlib only, run with -I; scripts and exports in /tmp/reichweite-scratch/P05/.
- Row counts and window dates above come from the executed scripts listed under Method.
