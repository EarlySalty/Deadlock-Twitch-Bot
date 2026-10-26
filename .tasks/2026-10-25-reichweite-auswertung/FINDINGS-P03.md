# P03: Timing, competition and favourable slots

Saved by the orchestrator from the P03 agent's final report (the agent's Write call was rejected). Queries: `queries-P03.sql`.

Package P03 of the Twitch reach study. SQL extraction: `queries-P03.sql`. All weighting, indexing, bootstrap and test code is Python post-processing in `/tmp/reichweite-scratch/P03/`. Times are Europe/Berlin local.

## Question

When is it favourable to stream German Deadlock content on Twitch?

1. Weekday x hour heatmap of German category demand and supply, with monthly change.
2. Within-streamer normalised viewers and first-time chatters by start hour and weekday, per size class.
3. Gap slots where small streamers face little competition.
4. Global category view, German vs all languages.
5. Calendar effects: weekends, holidays, patch days, month trends.
6. Start time vs session length.

## Data used

| Source | Filter | Units | Time range |
|---|---|---|---|
| `twitch_stats_category` (polls) | language `de%` or NULL; 2026-06-10 to 06-13 excluded (incident) | 1,063,991 poll rows, 375 days | Extracted per calendar month, 2025-10 to 2026-10 |
| `twitch_stream_sessions` | started_at from 2026-01-09; duration 600 to 86,400 s; had_deadlock_in_session; language `de%` or NULL | 11,249 sessions, 1,709 streamers | First qualifying session in February 2026; no qualifying January 2026 session |
| `twitch_session_chatters` (first-time chatters) | same session filter, June to October; rows with `messages >= 1` and `is_first_time_streamer`; logins seen in 30 or more channels excluded | 5,595 sessions, 1,060 streamers | 2026-06-01 to 2026-10-25 |
| `category_snapshots_normalized` (all languages) | snapshot_at 2026-10-09 to 2026-10-25 | 17 days | Table data begins 2026-10-09 10:40 UTC |

Qualifying sessions per month: Feb 909, Mar 2,218, Apr 1,346, May 1,181, Jun 653, Jul 734, Aug 1,091, Sep 924, Oct 2,193.

Sessions with at least one message-writing first-time chatter: Feb 275 of 909 (30%); Mar 333 of 2,218 (15%); Apr 69 of 1,346 (5%); May 62 of 1,181 (5%); Jun 71 of 653 (11%); Jul 256 of 734 (35%); Aug 537 of 1,091 (49%); Sep 504 of 924 (55%); Oct 1,252 of 2,193 (57%). March to May chat tracking is sparse, so first-timer results use June to October only.

First-time chatter definition: a `twitch_session_chatters` row with `messages >= 1` and `is_first_time_streamer`, logins seen in 30 or more channels excluded (query `chat-counts.sql`, the same definition as P02 and P04; the session base and coverage queries in `queries-P03.sql` compute these counts inline with the same rule, identical on all 11,249 analysed sessions). The session columns `first_time_chatters`, `unique_chatters` and `returning_chatters` count lurkers as well and are not used. An earlier version of this package used those columns; the results that changed are listed under caveats.

## Method

- **Poll weighting.** Each poll is weighted by the time to the next poll, capped at 330 s in Python (the SQL uses a 1,800 s cap). Poll interval was about 300 s until 2026-01-17, about 75 s until 2026-03-11, and about 15 s afterwards.
- **Rest metrics.** In each poll the three largest channels are excluded. `rest_viewers` is the viewer count of the remaining channels; `rest_vps` is `rest_viewers` divided by the number of remaining channels. Mean viewer counts are dominated by top channels (top-3 share 0.6 to 0.99 in the slot data), so rest metrics describe the small-streamer situation.
- **Heatmap (item 1).** Pooled, time-weighted ratios per weekday and Berlin hour over all days. Per-poll medians (item 3) are a different aggregation and are not comparable cell by cell (for example Mo 19: pooled rest_vps 2.32, per-poll median 2.0).
- **Within-streamer index (items 2, 3, 6).** For each group, observed sum divided by the sum of each streamer's own baseline (their mean over all their sessions in the window, times the group weight), pooled over streamers. Viewers use average viewers per session; first-timers use first-time chatters per session hour. An index of 1.00 means the streamer's own average. 95% intervals from a cluster bootstrap over streamers (200 resamples, seed 11).
- **Size classes.** By the streamer's median average viewers: 0-4, 5-19, 20-99, 100+.
- **Gap classes (item 3).** Per weekday-hour slot: A = per-poll median rest_vps at or above the 75th-percentile cut (1.556) and mean streams at most 5; B = other slots at or above that cut; C = middle tercile; D = at or below the 33rd-percentile cut (1.0).
- **Patch study (item 5).** Each patch day's daily rest_vps is compared with the median of the 14 surrounding non-patch days. One-sided sign tests. Placebo: all non-patch days from 2025-11-01.
- **Holidays (item 5).** Each holiday's daily value is compared with the median of the same weekday within 28 days (6 to 8 comparison days), excluding days within one day of another holiday.
- **Global (item 4).** sample_seconds-weighted average per Berlin hour, averaged over the days that have data in that hour. German = language exactly `de`.

## Results

### 1. Weekday x hour heatmap and monthly change

Selected slots, all time (pooled, time-weighted):

| Slot | Viewers (mean) | Streams (mean) | Rest viewers | Rest viewers per rest stream | Top-3 share |
|---|---|---|---|---|---|
| Mo 19 | 47.5 | 8.04 | 12.0 | 2.32 | 0.747 |
| Mo 21 | 53.7 | 10.94 | 19.8 | 2.46 | 0.632 |
| Di 21 | 98.2 | 9.99 | 18.2 | 2.57 | 0.815 |
| Mi 19 | 254.4 | 8.94 | 19.2 | 3.14 | 0.925 |
| Do 21 | 141.6 | 10.64 | 22.1 | 2.88 | 0.844 |
| Sa 17 | 418.2 | 6.59 | 26.5 | 6.79 | 0.937 |

Per-poll view for weekday evenings, weekday mornings and weekend afternoons (median and mean of per-poll rest_vps; median and mean streams per poll):

| Slot | Median rest_vps | Mean rest_vps | Median streams | Mean streams |
|---|---|---|---|---|
| Mo 21 | 2.2 | 2.22 | 10 | 10.94 |
| Di 21 | 2.2 | 2.24 | 9 | 9.99 |
| Mi 20 | 2.2 | 2.44 | 8 | 10.66 |
| Do 21 | 2.1 | 2.27 | 10 | 10.64 |
| Mi 19 | 2.0 | 2.52 | 7 | 8.94 |
| Mo 19 | 2.0 | 1.99 | 7 | 8.04 |
| Fr 11 | 1.0 | 2.13 | 2 | 2.42 |
| Fr 12 | 1.0 | 2.39 | 2 | 2.68 |
| Fr 13 | 1.0 | 1.83 | 3 | 3.24 |
| Fr 14 | 1.0 | 2.27 | 3 | 3.76 |
| Sa 12 | 1.33 | 1.56 | 3 | 3.15 |
| Sa 13 | 1.5 | 1.68 | 4 | 3.88 |
| Sa 14 | 1.5 | 1.61 | 4 | 4.63 |
| So 12 | 1.5 | 1.70 | 4 | 3.76 |
| So 13 | 1.67 | 1.84 | 5 | 5.02 |

Monthly change (median over days of the month; top-3 share is the median daily share; viewers is the median of daily viewers):

| Month | Days | Streams per day | Rest viewers per rest stream | Viewers | Top-3 share |
|---|---|---|---|---|---|
| 2025-10 | 22 | 2.68 | 1.19 | 12.1 | 0.96 |
| 2025-11 | 30 | 2.42 | 1.02 | 6.3 | 0.95 |
| 2025-12 | 31 | 2.33 | 1.00 | 6.6 | 0.95 |
| 2026-01 | 31 | 2.93 | 1.22 | 8.6 | 0.90 |
| 2026-02 | 26 | 7.77 | 1.95 | 251.25 | 0.92 |
| 2026-03 | 31 | 8.15 | 2.40 | 70.5 | 0.83 |
| 2026-04 | 30 | 5.53 | 2.17 | 35.3 | 0.82 |
| 2026-05 | 31 | 5.94 | 2.10 | 35.3 | 0.82 |
| 2026-06 | 26 | 3.54 | 1.59 | 19.1 | 0.89 |
| 2026-07 | 31 | 3.62 | 1.59 | 14.8 | 0.89 |
| 2026-08 | 31 | 4.51 | 1.90 | 24.1 | 0.86 |
| 2026-09 | 30 | 3.84 | 1.98 | 22.25 | 0.86 |
| 2026-10 (to 25th) | 25 | 9.60 | 2.54 | 70.0 | 0.73 |

Weekday vs weekend (all time, daily medians): Mo to Do (212 days) streams 4.02, rest_vps 1.87, viewers 21.15. Fr to So (163 days) streams 4.85, rest_vps 1.80, viewers 23.6. Within months, Fr to So has more streams than Mo to Do in 10 of 13 months (median monthly difference +0.48 streams per day).

### 2. Within-streamer start hour and weekday, per size class

Viewers index by start band (Feb 1 to Oct 25 2026, all qualifying sessions):

| Start (Berlin) | Class | Sessions | Streamers | Index | 95% CI |
|---|---|---|---|---|---|
| 00-14 | 0-4 | 2,747 | 662 | 1.027 | 1.005-1.050 |
| 15-16 | 0-4 | 1,061 | 400 | 0.979 | 0.946-1.015 |
| 17-18 | 0-4 | 1,482 | 485 | 0.995 | 0.969-1.023 |
| 19-20 | 0-4 | 2,016 | 566 | 0.997 | 0.969-1.021 |
| 21-22 | 0-4 | 1,373 | 417 | 0.996 | 0.973-1.018 |
| 23+ | 0-4 | 404 | 196 | 0.922 | 0.877-0.965 |
| 00-14 | 5-19 | 445 | 108 | 1.046 | 0.995-1.103 |
| 15-16 | 5-19 | 186 | 83 | 0.983 | 0.907-1.058 |
| 17-18 | 5-19 | 455 | 110 | 1.005 | 0.949-1.051 |
| 19-20 | 5-19 | 454 | 125 | 0.971 | 0.941-1.014 |
| 21-22 | 5-19 | 143 | 62 | 0.971 | 0.900-1.024 |
| 23+ | 5-19 | 33 | 19 | 0.946 | 0.774-1.038 |
| 17-18 | 20-99 | 34 | 15 | 0.837 | 0.716-0.986 |
| 19-20 | 20-99 | 32 | 12 | 0.881 | 0.822-1.047 |
| 00-14 | 100+ | 92 | 20 | 1.021 | 0.979-1.146 |

Only the 00-14 band has usable counts for class 100+; the other bands there have too few sessions to interpret. Class 20-99 has more than 30 sessions only in the 17-18 and 19-20 bands.

Viewers index by weekday (Feb 1 to Oct 25 2026):

| Day | 0-4 index (CI) | 0-19 index (CI) |
|---|---|---|
| Mo | 0.991 (0.961-1.017) | 1.008 (0.968-1.053) |
| Di | 1.008 (0.967-1.053) | 0.992 (0.949-1.043) |
| Mi | 0.992 (0.963-1.031) | 0.963 (0.929-0.995) |
| Do | 1.020 (0.985-1.060) | 1.017 (0.986-1.056) |
| Fr | 0.998 (0.968-1.036) | 1.006 (0.957-1.072) |
| Sa | 0.991 (0.957-1.031) | 0.982 (0.953-1.017) |
| So | 1.002 (0.962-1.038) | 1.035 (1.002-1.068) |

First-timers per session hour by start band (June 1 to Oct 25 2026, primary window; index relative to the streamer's own rate):

| Start (Berlin) | 0-4 index (CI) | 5-19 index (CI) | 0-19 index (CI), sessions, streamers |
|---|---|---|---|
| 00-05 | 0.958 (0.841-1.114) | 0.922 (0.751-1.101) | 0.951 (0.873-1.067), 406, 146 |
| 06-11 | 1.105 (1.013-1.222) | 1.090 (0.883-1.319) | 1.100 (1.024-1.206), 493, 193 |
| 12-14 | 0.890 (0.825-0.949) | 0.939 (0.723-1.268) | 0.900 (0.841-0.983), 715, 292 |
| 15-16 | 0.948 (0.847-1.042) | 1.160 (0.965-1.474) | 1.014 (0.919-1.141), 598, 285 |
| 17-18 | 1.067 (0.992-1.130) | 0.940 (0.811-1.046) | 1.021 (0.957-1.084), 986, 374 |
| 19-20 | 0.976 (0.924-1.025) | 0.997 (0.900-1.110) | 0.984 (0.933-1.032), 1,245, 415 |
| 21-22 | 1.041 (0.943-1.150) | 0.902 (0.620-1.184) | 1.011 (0.933-1.119), 711, 288 |
| 23+ | 1.059 (0.912-1.248) | 1.106 (0.543-1.868) | 1.065 (0.925-1.213), 206, 115 |

Sensitivity July to October (0-19 pooled): 06-11 1.076 (0.990-1.160), 448 sessions; 12-14 0.892 (0.831-0.970), 625 sessions; 17-18 1.023 (0.957-1.106), 868 sessions; 19-20 0.991 (0.942-1.060), 1,112 sessions; 21-22 1.016 (0.921-1.102), 598 sessions.

First-timers per session hour by weekday (June 1 to Oct 25 2026):

| Day | 0-4 index (CI) | 0-19 index (CI), sessions, streamers |
|---|---|---|
| Mo | 1.058 (0.947-1.174) | 1.106 (1.005-1.228), 685, 317 |
| Di | 1.011 (0.918-1.108) | 1.017 (0.946-1.113), 754, 344 |
| Mi | 0.991 (0.923-1.070) | 0.983 (0.921-1.050), 793, 347 |
| Do | 0.966 (0.887-1.063) | 0.971 (0.886-1.051), 753, 338 |
| Fr | 1.033 (0.953-1.119) | 1.020 (0.948-1.095), 765, 343 |
| Sa | 0.955 (0.896-1.020) | 0.911 (0.839-1.002), 853, 387 |
| So | 0.998 (0.923-1.070) | 1.013 (0.935-1.084), 757, 344 |

### 3. Gap slots for small streamers

Candidate gap slots (per-poll median rest_vps at least 1.5 and mean streams at most 4): Mo 03 (median 1.5, mean rest 1.71, median streams 1, mean streams 1.89), Do 14 (1.5, 1.66, 3, 3.55), Sa 13 (1.5, 1.68, 4, 3.88), So 12 (1.5, 1.70, 4, 3.76). Only four slots qualify.

Gap class first-timer index (June 1 to Oct 25 2026, 0-19 pooled): A (high rest, few streams) 0.906 (0.389-1.452), 44 sessions, 36 streamers; B (high rest) 0.990 (0.970-1.009), 3,009 sessions, 723 streamers; C (middle) 0.998 (0.949-1.046), 1,425 sessions, 488 streamers; D (low rest) 1.037 (0.961-1.124), 849 sessions, 314 streamers.

Gap class viewers index (Feb to Oct 2026, size class 0-4): A 1.013 (0.881-1.177), 77 sessions; B 0.984 (0.971-0.996), 5,158 sessions; C 1.007 (0.987-1.031), 2,399 sessions; D 1.047 (0.995-1.094), 1,401 sessions.

### 4. Global category view, German vs all languages

Per Berlin hour, 2026-10-09 to 2026-10-25 (German = language `de`):

| Hour | Days | German avg viewers | German share of viewers | German avg streams | German share of streams | German viewers per stream | All-language viewers per stream |
|---|---|---|---|---|---|---|---|
| 0 | 16 | 124.5 | 0.4% | 18.9 | 1.9% | 6.6 | 34.3 |
| 12 | 17 | 114.6 | 0.9% | 5.2 | 1.4% | 21.9 | 33.4 |
| 13 | 17 | 162.5 | 1.2% | 7.1 | 1.7% | 23.1 | 33.3 |
| 14 | 17 | 193.3 | 1.2% | 7.9 | 1.8% | 24.6 | 34.6 |
| 15 | 17 | 221.2 | 1.3% | 9.4 | 1.8% | 23.5 | 33.7 |
| 16 | 17 | 232.7 | 1.2% | 10.6 | 1.8% | 21.9 | 32.3 |
| 19 | 16 | 271.7 | 1.0% | 16.4 | 2.1% | 16.6 | 33.0 |
| 20 | 16 | 206.5 | 0.7% | 20.6 | 2.4% | 10.1 | 33.0 |
| 21 | 16 | 179.1 | 0.6% | 22.0 | 2.4% | 8.1 | 31.9 |

All-language average concurrent streams: 979.7 at 0h, 384.3 at 12h, 420.0 at 13h, 447.1 at 14h, 516.4 at 15h, 583.6 at 16h, 785.7 at 19h, 857.7 at 20h, 898.0 at 21h. All-language viewers per stream (30 to 35 across hours) are dominated by large channels and are not a like-for-like benchmark for small streamers.

### 5. Calendar effects

Patch days (44 usable dates; daily rest_vps ratio to the surrounding 14-day median):

| Day relative to patch | Median ratio | Share above 1 | Sign test |
|---|---|---|---|
| -1 | 1.090 | 58% (25 of 43) | one-sided p 0.18 |
| 0 | 1.071 | 68% (30 of 44) | one-sided p 0.011 |
| +1 | 0.963 | 39% (17 of 44) | lower-tail p 0.087 |
| +2 | 1.013 | 58% (25 of 43) | not tested |
| +3 | 1.051 | 60% | not tested |
| +7 | 0.965 | 38% (of 40) | not tested |

Placebo (non-patch days from 2025-11-01): rest_vps median 1.00, 49% above 1. Day 0 rest_viewers median 1.107, 55% above 1; streams median 0.975 on day 0. Day 0 is nominally significant, but day -1 is higher and the placebo median is 1.00, so the pattern does not single out patch days. Patch dates are spread across weekdays (Mon 7, Tue 8, Wed 7, Thu 8, Fri 8, Sat 4, Sun 2).

Holidays (ratio to the same-weekday median; streams / viewers / rest_vps; 6 to 8 comparison days each):

| Holiday | Streams | Viewers | rest_vps |
|---|---|---|---|
| New Year 2026 | 0.78 | 0.88 | 1.21 |
| Good Friday 2026 | 1.07 | 0.75 | 0.96 |
| Easter Monday 2026 | 0.94 | 1.08 | 0.90 |
| Labour Day 2026 | 1.45 | 2.09 | 1.03 |
| Ascension 2026 | 1.13 | 0.96 | 0.90 |
| Whit Monday 2026 | 1.19 | 1.37 | 1.14 |
| German Unity 2026 | 1.63 | 5.84 | 1.02 |
| Christmas Eve 2025 | 0.65 | 0.39 | 0.85 |
| Christmas 2025 | 1.11 | 0.94 | 0.97 |
| Boxing Day 2025 | 1.21 | 0.92 | 0.93 |
| New Year's Eve 2025 | 0.65 | 0.95 | 0.87 |

German Unity viewers come from one event (top-3 share 0.935; median viewers 23 against a mean of 226.7). rest_vps is above 1 on 4 holidays and below on 7. No holiday effect is established.

Weekends: see item 1 (more streams, same rest viewers per stream).

### 6. Start time vs session length

Viewers index, 0-19 pooled, start band x duration bucket (Feb 1 to Oct 25 2026; sessions per cell in brackets):

| Start (Berlin) | under 2 h | 2 to 4 h | 4 h or more |
|---|---|---|---|
| 00-14 | 0.946 (0.883-1.005) [1,222] | 0.994 (0.920-1.065) [1,074] | 1.141 (1.088-1.201) [896] |
| 15-16 | 0.866 (0.813-0.931) [449] | 0.935 (0.880-0.993) [386] | 1.081 (0.999-1.150) [412] |
| 17-18 | 0.884 (0.840-0.919) [583] | 0.992 (0.912-1.067) [626] | 1.064 (1.020-1.107) [728] |
| 19-20 | 0.883 (0.831-0.963) [757] | 0.976 (0.933-1.023) [1,026] | 1.069 (1.016-1.126) [687] |
| 21-22 | 0.860 (0.759-0.947) [568] | 0.991 (0.956-1.028) [652] | 1.166 (1.061-1.305) [296] |
| 23+ | 0.850 (0.776-0.908) [201] | 1.010 (0.898-1.133) [172] | 0.952 (0.827-1.083) [64] |

Session length is the strongest session-level correlate of average viewers. Riding into prime time: sessions starting 15-18 and still live at 19h index 1.033 (1.008 to 1.060) for size class 0-4 (1,684 sessions, 497 streamers, duration index 1.217); sessions starting 15-18 that end before 19h index 0.882 (0.842 to 0.923) (859 sessions, 383 streamers, duration index 0.508). Pooled 0-19: riding 1.022 (0.991 to 1.056), ending before 0.888 (0.853 to 0.929), 21-22 starts 0.988 (0.961 to 1.013).

First-timers, June to October, 0-19 pooled: start 15-18 riding into 19h 1.005 (0.944 to 1.063), 1,084 sessions; ending before 19h 1.094 (0.949 to 1.238), 500 sessions. Sessions of 4 h or more starting 19-20h: 0.901 (0.829 to 0.975), 347 sessions, 138 streamers; starting 21-22h: 0.847 (0.700 to 1.029), 121 sessions, 71 streamers. Sessions under 2 h starting 19-20h: 1.275 (1.142 to 1.435), 426 sessions, 251 streamers. Per hour, first-timers favour short sessions, the opposite of average viewers; P04 item 2 shows the same pattern.

## Actionable tips

| # | Tip | Evidence | Effect size | Support | Confidence |
|---|---|---|---|---|---|
| 1 | For average viewers, plan sessions of four hours or more. | Viewers index for 4 h+ sessions: 1.141 (1.088-1.201) starting up to 14h, 1.166 (1.061-1.305) starting 21-22h; under 2 h: 0.85 to 0.95. | 4 h+ sessions run 6 to 17% above the streamer's own average; under 2 h 5 to 15% below. | 896 and 296 sessions; 294 and 138 streamers | medium (association; streamers choose their length) |
| 2 | An evening start at 19-20h does not raise a small streamer's average viewers above that streamer's own average. | Index 0.997 (0.969-1.021) for class 0-4; 0.971 (0.941-1.014) for class 5-19. | Null | 2,016 and 454 sessions | medium (null result) |
| 3 | Staying live into the evening helps average viewers; ending before 19h costs them. | Class 0-4: riding into 19h 1.033 (1.008-1.060); ending before 0.882 (0.842-0.923). | About 17% difference between the two groups | 1,684 and 859 sessions | medium (duration confound: these sessions are also much longer) |
| 4 | Do not pick a start hour for first-time chatters; the evening start at 19-20h brings the streamer's usual rate. | 0-19 pooled first-timer index 19-20h 0.984 (0.933-1.032); July to October 0.991 (0.942-1.060). Morning 06-11h 1.100 (1.024-1.206) and 12-14h 0.900 (0.841-0.983) are the only bands whose intervals exclude 1; July to October 06-11h 1.076 (0.990-1.160). | Within about 10% per hour; null for the evening | 1,245 sessions, 415 streamers (19-20h) | low (eight bands tested; morning sample smaller) |
| 5 | Do not pick a weekday for first-time chatters. | First-timer index Sa 0.911 (0.839-1.002), Mo 1.106 (1.005-1.228), other days 0.97 to 1.02. Viewers index Sa 0.982 (0.953-1.017). | Within about 10% per hour; Saturday interval includes 1 | 853 sessions, 387 streamers (Saturday) | low (seven days tested; chat coverage varies by month) |
| 6 | Weekday evenings have the most viewers per other small stream; weekend afternoons have few small streams. | Per-poll median rest_vps 2.0 to 2.2 on Mo, Di, Mi, Do 19-21h with 7 to 10 streams; Fr 11-14h 1.0 with 2 to 3 streams; Sa 13h 1.5 with 4 streams. | Descriptive only | 161 slots | medium (describes supply and demand; does not estimate the effect of a new streamer) |
| 7 | Afternoons give the most German viewers per German stream; 19h shows the largest German average audience in the table. | Viewers per German stream 21.9 to 24.6 at 12-16h, 16.6 at 19h, 10.1 at 20h; German average viewers 271.7 at 19h. | Descriptive | 17 days of global data | medium (category means are dominated by large channels) |
| 8 | Weekends have more streams but not more viewers per stream, so they do not beat weekdays per stream. | Fr-So minus Mo-Do streams +0.48 per day in 10 of 13 months; rest_vps 1.80 vs 1.87; viewers index by weekday 0.98 to 1.04. | Null on rest_vps | 212 and 163 days | medium |
| 9 | Do not plan around patch days or holidays. | Day-0 patch median 1.071 (p 0.011) is matched by day -1 (1.090, p 0.18); placebo 1.00; holiday rest_vps above 1 on 4 of 11. | Null | 44 patch days; 11 holidays | low (no reliable effect) |
| 10 | Do not expect a gap slot for small streamers from this data. | Gap class A first-timer index 0.906 (0.389-1.452) on 44 sessions; only four slots qualify. | Not established | 44 sessions | low (do not act on it) |

## Caveats and dead ends

- **Withdrawn first-pass results.** The first pass used the full February to October window for first-timers, where March to May chat tracking is sparse. The 19-20h first-timer advantage (1.16 for class 5-19, CI 0.99 to 1.32) and the full-window riding and 19-20h 4 h+ results are withdrawn. The June-to-October values above replace them.
- **Withdrawn session-column results.** An earlier version counted first-timers from the session column `first_time_chatters`, which includes lurkers. Its 19-20h first-timer hint (1.072, CI 0.989-1.151), the Saturday penalty (0.836, CI 0.766-0.915), the 4 h+ 19-20h value (1.145) and the gap class A value (1.226) are withdrawn. With message-based counts the evening hint and the Saturday penalty disappear.
- **Uneven chat coverage.** Share of sessions with at least one message-writing first-time chatter: 11% in June, 35% in July, 49% in August, 55% in September, 57% in October (1,252 of 2,193 sessions at the export; a rerun of `queries-P03.sql` on 2026-10-26 includes sessions of 2026-10-25 that were still live at the export and gives 1,278 of 2,257, also 57%). The June-to-October indices may partly reflect coverage changes. The July-to-October sensitivity agrees on 19-20h (0.991) and 21-22h (1.016).
- **February 2026 level jump.** Daily viewers median 8.6 in January, 251.25 in February, 70.5 in March. Top-3 share stays at 0.82 to 0.92, so the level is driven by large channels. The cause is not verified (possibly collector coverage). The stream-count jump (909 in February, 2,218 in March) looks real.
- **Patch list is hand-compiled.** The 46 patch dates are a list typed into `patch_study.py` in scratch, not derived from a database query in this package. Treat the patch result as unverified input.
- **Gap classes use full-window slot metrics.** Slot classes come from the slot file over the full poll window, so they are not period-specific.
- **Small size classes.** Class 20-99 and 100+ have too few sessions per band for the June-to-October first-timer analysis; the viewers table above shows the only bands with usable counts.
- **Viewer curves not used.** `twitch_session_viewers` was not queried; item 6 uses session averages only.
- **Session window.** The brief says sessions exist from 2026-01-09; under this filter there are no qualifying January 2026 rows.
- **Brief discrepancy.** `category_snapshots_normalized` starts 2026-10-09 10:40:38 UTC, not 2026-09-27 as the brief states, so the global view covers 17 days.
- **Dead ends.** Median-based within-streamer first-timer differences were zero because of ties; a pooled index with cluster bootstrap replaced them. Mean slot values were dominated by rare large rest streams (top-3 share up to 0.99), so per-poll medians and rest-only metrics are used. The first-pass holiday baseline filter was a no-op and was corrected before the reported holiday numbers. A scratch module named `calendar.py` shadowed the standard library and was renamed to `cal_daily.py`; this did not change results.
- **Out of scope for this package.** Lurker baselines (logins seen in 30+ channels) and late-sampling share belong to P05 and are not used here.
