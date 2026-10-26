# P04: Stream length, frequency, consistency and long-term growth

Saved by the orchestrator from the P04 agent's final report (the agent's Write call was rejected).
Queries: `queries-P04.sql`. Curve summaries: `p04-curves.py` on the Q1a export.

## Question

Which streaming habits go along with channels actually growing over months?

## Data used

| Source | Filter | Size | Time range |
|---|---|---|---|
| `twitch_session_viewers` | Deadlock sessions of at least 60 min with a curve | 7,534,673 rows; 9,830 curve sessions, 1,574 streamers | 2026-01-09 to 2026-10-25 17:28 UTC |
| `twitch_stream_sessions` | Deadlock sessions | 14,993 sessions (all games), 12,957 Deadlock, 1,870 logins | 2026-01-09 to 2026-10-25 17:28 UTC |
| `twitch_stats_category` | German filter, June 2026 incident rule applied | 4,989,250 rows, 1,380 user ids; 10,680 derived streams of at least 20 min | since 2025-10-01 |
| `twitch_session_chatters` | sessions | 164,861 rows (130,269 lurker rows, 34,592 chatter rows) | since 2026-01-31 |

Size classes (curve analyses): median Deadlock session `avg_viewers` per streamer, sessions of at least 60 min: 0-4 (1,234 streamers), 5-19 (281), 20-99 (61), 100+ (29).

## Method

- Curves: per-session minute averages on the session clock, `floor((ts_utc - started_at) / 60)`, normalized by the session's own peak; only sessions whose first sample is at or before minute 15 and peak at least 3. About 25 % of sessions start sampling after minute 15 and about 58 % after minute 5 (14,525 sessions with a curve, P11 rerun). Snapshot curves from `twitch_stats_category` as second source.
- Duration and habit effects: ratio of sums per hour; within-streamer differences, median across streamers.
- Cohort: active week = at least 2 h live. Baseline = median weekly mean over the first 4 active weeks; outcome = weeks 8 to 12 later. Grower = at least 1.5x and +2 viewers; decliner = at most 0.67x and -2 viewers.
- Breaks: median of next 4 streams divided by previous 4.
- Consistency: regular week = at least 75 % of sessions start within plus or minus 1 h of the modal start hour.

## Results

### 1. Build-up within a stream

| Class | Sessions | Median time to 95 % of peak (min) | IQR | Median argmax (min) |
|---|---|---|---|---|
| 0-4 | 4,136 | 84 | 43 to 144 | quantized |
| 5-19 | 832 | 98 | 57 to 168 | 104 |
| 20-99 | 61 | 102 | | |
| 100+ | 18 | 121 | | |

Sessions of at least 5 h, class 5-19 (n=196): time to 95 % of peak median 195 min, argmax median 206 min, 60 % of peaks at 180 min or later.

Normalized level, class 5-19 (session basis / snapshot basis): minute 30 0.523 / 0.615, minute 60 0.630 / 0.667, minute 120 0.692 / 0.704, minute 180 0.714 / 0.684, minute 240 0.700 / 0.692, minute 300 0.667 / 0.647.

Hour k relative to hour 2 (same stream; session basis = median per-stream ratio from `p04-curves.py`):

| Class | Basis | Hour 3 | Hour 4 | Hour 5 | Hour 6 | Hour 8 |
|---|---|---|---|---|---|---|
| 5-19 | session (n=515 / 322 / 182 / 105 / 37) | 1.074 | 1.156 | 1.246 | 1.248 | 1.199 |
| 5-19 | snapshot, streams of 5 h or more (n=147) | 1.091 | 1.126 | 1.170 | 1.169 | 1.267 |
| 0-4 | snapshot, 5 h or more (n=265) | 0.997 | 0.980 | 0.952 | | |
| 20-99 | snapshot, 5 h or more (n=32) | 1.042 | 1.027 | 1.025 | | |

### 2. Duration vs chatters and followers (within streamer)

First-time chatters per hour (June to October 2026, sessions with at least one chatter): under 2 h 1.23 (767 sessions), 2 to 4 h 0.71 (1,351), 4 h or more 0.81 (1,290). Within streamer (153 streamers): 4 h or more minus under 2 h = -0.38 per hour, 25 % positive. A first-time chatter is a `twitch_session_chatters` row with `messages >= 1` and `is_first_time_streamer`, logins seen in 30 or more channels excluded, the same definition as P02, computed by `chat-counts.sql`; the session columns `first_time_chatters` and `unique_chatters` include lurkers and are not used. An earlier version used those columns (0.93, 0.87, 1.08; -0.13 on 117 streamers).

Follower delta per hour (March to October 2026): under 2 h 0.065, 2 to 4 h 0.084, 4 h or more 0.038. Within streamer (209): median 0.00.

### 3. Growth cohorts

| Baseline class | n | Growers | Flat | Decliners | Median outcome / baseline |
|---|---|---|---|---|---|
| 0-4 | 79 | 5 | 74 | 0 | 1.04 |
| 5-19 | 21 | 0 | 18 | 3 | 0.95 |
| 20-99 | 8 | 0 | 7 | 1 | 0.89 |
| 100+ | 2 | 0 | 2 | 0 | 0.97 |
| All | 110 | 5 (4.5 %) | 101 (91.8 %) | 4 (3.6 %) | |

Habits (medians, growers n=5 / flat n=101 / decliners n=4): streams per week 3.38 / 1.50 / 1.94; live hours per week 14.0 / 3.82 / 4.26; stream length 3.18 / 2.45 / 2.25 h; start-hour consistency 0.43 / 0.50 / 0.47.

Spearman rho with log2(outcome/baseline), n=110 (strict n=75): streams per week 0.16 (0.01), live hours per week 0.21 (0.06), stream length 0.12 (-0.01), start-hour consistency -0.09 (-0.10), weekday consistency -0.18 (-0.08).

Loyalty: median per-streamer returning share of present viewers (lurkers included) 0.10 (778 streamers); chatters only 0.08 (554).

### 4. Breaks

| Pause | Events | Median after/before | Share up |
|---|---|---|---|
| 7 days or more | 657 | 1.015 | 52 % |
| 14 days or more | 352 | 1.041 | 55 % |
| 30 days or more | 183 | 1.054 | 57 % |

Class 5-19 after 14 days: 56 events, 1.13; after 30 days: 38 events, 1.105.

### 5. Regular vs irregular weeks

147 streamers, median within-streamer difference +0.011 normalized, 52 % positive. Null.

## Actionable tips

1. Small channels (5-19 viewers) gain from long streams: hour 5 is 1.17x to 1.25x hour 2, peak typically after minute 180 in 5 h streams. Confidence medium (self-selection).
2. Longer streams do not bring more first-time chatters per hour within a streamer (-0.38, n=153). Confidence medium.
3. Pauses of 7 to 30 days cost nothing measurable (1.02 to 1.05). Confidence medium.
4. Schedule regularity shows no measurable within-streamer effect (+0.01, n=147). Confidence low.
5. Growth over 8 to 12 weeks is rare (4.5 % of 110). Growers streamed about 14 h per week vs 3.8 h, but the correlation vanishes on the strict subset. Confidence low for habit claims.

## Caveats

- Chat fields in `twitch_stream_sessions` are zero from March to May 2026; `follower_delta` implausible in January and February 2026.
- About 25 % of sessions start sampling after minute 15 (58 % after minute 5).
- Session clock: the stored `minutes_from_start` is stuck at 0 for 314 sessions from 2026-06-22 to 2026-07-03 (collector fault). The curves above use the session clock; the earlier figures on the stored field differed by at most 0.01 in level and 4 minutes in timing.
- The hour-k session row reproduces from `p04-curves.py` on the exported curve sessions; the earlier row (1.066 / 1.152 / 1.237 / 1.248 / 1.193) came from the agent's own run and did not reproduce. The earlier note of 60 % after minute 15 was wrong; P04's curve filter is unaffected.
- Class 0-4 curves are quantized.
- Hour-k values exist only for streams that lasted hour k (survivorship).
- Cohort is small: 110 streamers, 5 growers.
