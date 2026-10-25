# P12: When Twitch shows a channel more often (exposure triggers)

Written by the orchestrator from the P12 agent's stage output (`/tmp/reichweite-scratch/P12/py/stage2.out`) and final report. The agent's Write call was rejected.
Queries: `queries-P12.sql`. Post-processing: Python stdlib on per-month CSV exports.

## Question

Under which conditions does a German Deadlock stream suddenly gain viewers without a raid, and does directory position, momentum or competition change how many new people arrive?

## Data used

| Source | Filter | Size | Time range |
|---|---|---|---|
| `twitch_stats_category` | German filter, June 2026 incident rule, averaged per streamer and minute | 1,527,524 stream-minutes (13 monthly panels) | 2025-10 to 2026-10 |
| `twitch_stream_sessions` | session start, end, start viewers | used for uptime and next-day start | since 2026-01-09 |
| `twitch_session_chatters` | first-time rows; chatters (`messages >= 1`) and presence (chatters plus lurkers seen by the chatters API) | first-time presence since 2026-03-01: 29,625 (14,584 chat only) | since 2026-01-31 |
| `twitch_raid_history`, `twitch_raid_arrival_tracking` | incoming raids, excluded around events | | full history |
| `category_snapshots_normalized` | rank of German streams among all category channels | 254 jump events with all-language rank | since 2026-10-09 |

The 16-second snapshot era (since 2026-03-11) is used for Q1 conditions and Q2 to Q5.

## Method

- **Jump event:** minute viewers at least 1.5x the value 5 minutes earlier and at least +3 viewers; first minute of a run; at least 10 minutes between events of one streamer.
- **Strict:** no recorded incoming raid from 10 minutes before to 2 minutes after. **Conservative (cons):** also no matching German drop (another German stream losing at least half of the gain in the same minutes, or going offline).
- **Baseline:** all stream-minutes of the same viewer class.
- **Rank:** position among tracked German Deadlock channels at that minute. This is not Twitch's directory order, which also depends on personalisation.
- **Outcomes over the next 15 minutes:** new presence (first-time chatters plus first-time lurkers seen) and new chatters.
- **Lurker caveat:** `first_message_at` for lurker rows is the first chatters-API poll that saw the viewer, not the true arrival time. Non-first-time lurkers have a 0-minute span (115,045 rows), first-time lurkers a median of 6.5 minutes (15,041 rows). Presence is therefore only a 15-minute window proxy. Lurker rows grow from 4,134 (March) to 44,709 (October), so pooled levels drift; Q5 is stratified by month.

## Results

### Q1. Organic jumps: how many, and under which conditions

Events per month (all / strict / cons):

| Month | All | Strict | Cons |
|---|---|---|---|
| 2025-10 | 97 | 97 | 97 |
| 2025-11 | 132 | 132 | 132 |
| 2025-12 | 100 | 100 | 100 |
| 2026-01 | 220 | 220 | 178 |
| 2026-02 | 798 | 792 | 462 |
| 2026-03 | 1,113 | 1,075 | 499 |
| 2026-04 | 676 | 650 | 367 |
| 2026-05 | 527 | 498 | 310 |
| 2026-06 | 386 | 355 | 262 |
| 2026-07 | 388 | 349 | 254 |
| 2026-08 | 676 | 604 | 408 |
| 2026-09 | 601 | 520 | 327 |
| 2026-10 | 1,148 | 1,069 | 446 |
| Total | 6,862 | 6,461 | 3,842 |

Event counts before March 2026 are lower partly because the snapshot interval was 300 s and 75 s then.

Events by viewers before the jump (all / cons): 0-2: 2,676 / 1,437; 3-4: 1,750 / 982; 5-9: 1,954 / 1,114; 10-19: 302 / 195; 20-49: 50 / 42; 50+: 41 / 35. Big organic jumps (cons, at least +10 and at least doubling): 146 events from 76 streamers.

Conditions just before a conservative organic jump vs all stream-minutes of the same viewer class (16-s era, 2,683 cons events):

| Viewers before | Events | Median uptime (min) event / base | German rank median event / base | Share in German top 3 event / base | German streams live (median) event / base | Share 18-23 h event / base | Title changed in window event / base |
|---|---|---|---|---|---|---|---|
| 0-2 | 992 | 22 / 90 | 4 / 6 | 0.458 / 0.248 | 6 / 9 | 0.403 / 0.525 | 0.016 / 0.008 |
| 3-4 | 662 | 83 / 125 | 2 / 4 | 0.713 / 0.475 | 6 / 9 | 0.385 / 0.533 | 0.009 / 0.008 |
| 5-9 | 796 | 100 / 146 | 2 / 2 | 0.857 / 0.730 | 6 / 8 | 0.416 / 0.525 | 0.008 / 0.008 |
| 10-19 | 143 | 112 / 170 | 1 / 2 | 0.958 / 0.933 | 9 / 10 | 0.531 / 0.622 | 0.021 / 0.013 |
| 20-49 | 27 | 28 / 199 | 1 / 1 | 1.000 / 0.981 | 7 / 8 | 0.333 / 0.481 | 0.037 / 0.019 |
| 50+ | 26 | 155 / 219 | 1 / 1 | 0.962 / 0.979 | 10 / 10 | 0.462 / 0.449 | 0.000 / 0.014 |

Rank among all category channels (all languages, since 2026-10-09) at 0-2 viewers: median 336 at event vs 489 base. Chat speed in the 5 minutes before was 0 in the median for both.

Weekend share was lower at events than at base in most classes (0-2: 0.29 vs 0.321).

### Q2. Directory position (German rank), next 15 minutes

All 16-s stream-minutes with outcome: 1,239,072.

Within the same viewer class, new presence per 15 minutes by German rank bucket:

| Viewers | Rank 1-3 | Rank 4-6 | Rank 7-12 | Rank 13-24 |
|---|---|---|---|---|
| 0-2 | 0.118 (139,830 min) | 0.117 (171,744) | 0.084 (200,088) | 0.097 (71,541) |
| 3-4 | 0.156 (114,997) | 0.122 (87,976) | 0.118 (43,341) | 0.159 (3,914) |
| 5-9 | 0.199 (161,285) | 0.159 (50,470) | 0.165 (10,379) | |
| 10-19 | 0.265 (83,107) | 0.188 (5,329) | | |

Boundary tests (rank k vs k+1 at the same viewer class): no step at 3|4, 6|7, 12|13 or 24|25. Differences in new presence stay within about 0.04 per 15 minutes in every class with enough minutes.

Within streamer, top 6 vs rank 7 or worse (streamers with at least 20 minutes in each): 627 streamers, median difference 0.0, share positive 0.397.

### Q3. Snowball

Organic growth over the next 5 minutes (uptime at least 10 minutes, no raid nearby): the share of minutes with positive growth stays below 0.5 at every level (1-2 viewers 0.233, 5-9 0.423, 20-29 0.490, 100+ 0.424). Median change per minute is 0 or slightly negative everywhere. There is no level above which a stream keeps growing by itself.

### Q4. Momentum

Pooled new presence per viewer is higher in rising and falling windows than in flat ones (for example 5-9 viewers: rising 0.0256, flat 0.0200, falling 0.0266). Within streamer, rising minus flat: 658 streamers, median 0.0, share positive 0.257. Not a lever.

### Q5. Competition: how many German Deadlock streams are live

| German streams live | Minutes | New presence per viewer per 15 min (pooled) | Median ratio vs 1-5 inside month x hour strata (n strata) |
|---|---|---|---|
| 1-5 | 342,526 | 0.0201 | 1.00 |
| 6-10 | 452,956 | 0.0173 | 0.802 (118) |
| 11-15 | 274,669 | 0.0126 | 0.849 (81) |
| 16-20 | 145,672 | 0.0157 | 0.803 (40) |
| 21-25 | 65,978 | 0.0178 | 0.384 (16) |
| 26+ | 51,513 | 0.0280 | 0.671 (5) |

With 6 to 20 German streams live, each viewer brings about 15 to 20 percent fewer new people per 15 minutes than with 1 to 5 live, at the same month and hour.

Orchestrator check (VERIFY-ORCHESTRATOR.md V5): the pooled ratio mixes channel size with competition. Within streamer x viewer bin x hour band x weekend, 6-10 live minus 1-5 live is +0.027 new presences per 15 min (95 % CI -0.003 to +0.062, 329 streamers); 11+ minus 1-5 is +0.003 (CI -0.046 to +0.065, 196 streamers). No measurable competition cost.

### Q6. Day after a big organic jump

4,146 streamer-days with a next-day start and a 14-day baseline; 23 big-inflow days. Next-day start vs 14-day median (mean log ratio): 0.149 after big-inflow days vs 0.027 after other days (about 13 percent higher). Within streamer: 14 streamers, median difference 0.168, share positive 0.571.

## Actionable tips

1. **The first 20 to 100 minutes of a stream are when unexplained viewer jumps happen.** Median uptime at a conservative organic jump is 22 min vs 90 min base (0-2 viewers), 83 vs 125 (3-4), 100 vs 146 (5-9); 2,683 events. Likely mechanism: go-live notifications and fresh "just went live" placement. Confidence medium.
   Orchestrator note: V6 in `VERIFY-ORCHESTRATOR.md` shows 93 % of arrivals in the first 10 minutes are returning viewers on the 30-second chatter poll; new people arrive at a flat 1.3 to 2.2 per 100 session-minutes. The early jump is regulars answering the notification, not new discovery.
2. **Jumps come at quieter times, but busy slots do not cost new viewers.** Median 6 German streams live at events vs 9 base; evening share 0.40 vs 0.53 at 0-2 viewers. Both partly reflect that jumps cluster early in a stream. Within streamer, more German streams live shows no drop in new presences (V5). Confidence low for any timing lever.
3. **Being in the German top 3 goes along with jumps, but no specific rank boundary matters.** Top-3 share 0.46 vs 0.25 at events (0-2 viewers); within-streamer top 6 vs 7+ median 0.0 (627 streamers). The raw rank gap is mostly viewer size. Confidence medium (null for boundaries).
4. **There is no snowball and no momentum effect to chase.** Growth share below 0.5 at every level; within-streamer rising vs flat median 0.0 (658 streamers). Confidence medium.
5. **A big organic day may lift the next day's start.** +0.149 vs +0.027 log ratio, but only 23 days and 14 streamers. Confidence low.

## Caveats and dead ends

- Rank is among tracked German channels, not Twitch's personalised directory order. All-language rank exists only since 2026-10-09 (254 events).
- Raids and hosts are only partly visible; the conservative filter also removes jumps that coincide with another German stream ending (viewer migration).
- Q2 to Q5 use only the 16-s era. First-time flags start 2026-01-31.
- Lurker presence is a window proxy (see Method). Chatters are always `messages >= 1`. Session chat fields and `follower_delta` were not used; Q6 uses `start_viewers` only.
- Title changes before a jump double at 0-2 viewers (0.016 vs 0.008) but the absolute share is tiny; not a finding.
- All results are correlational.
