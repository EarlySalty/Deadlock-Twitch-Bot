# P11: Stream start, live notification, game switching, stream end and next start

Saved by the orchestrator from the P11 agent's final report (the agent's Write call was rejected). Queries: `queries-P11.sql`.

Scope: sessions started 2026-01-09 to 2026-10-25. Queries: `queries-P11.sql` in this folder. Raw extracts stay in `/tmp/reichweite-scratch/P11/`.

## Question

1. How do the start ramp and start features (live notification, title, start hour, weekend, gap since the previous stream) relate to viewers in the first 30 minutes?
2. How do title, duration, start hour, game and chat activation relate to retention_5m, retention_10m, retention_20m and dropoff?
3. How do game switches relate to viewers: Just Chatting to Deadlock, Deadlock to other, pure versus mixed sessions, and starting in Just Chatting?
4. How do the last 30 minutes, an outgoing raid at the end and the end profile relate to the next start?
5. Does a strong session (high normalized viewers or many first-time chatters) lift the next session's start?

## Data used

| Source | Coverage used | Size |
|---|---|---|
| `twitch_stream_sessions` | 2026-01-09 to 2026-10-25 | 14,979 sessions; 14,524 with a viewer curve; 14,839 with a game name (1,867 streamers) |
| `twitch_session_viewers` | 15-second samples, indexed by minutes from start | about 7.5 million rows |
| Population ("pop") | median average viewers of at least 5 | 2,821 sessions, 377 streamers |
| Early pop | pop with first curve sample at or before minute 5 | 863 sessions, 102 streamers |
| `twitch_chat_messages` | from 2026-01-31; chat analyses from 2026-08-01 with chat above zero | 3,735 sessions, 760 streamers |
| `exp_game_transitions` with `exp_sessions` | from 2026-03-01 | 4,620 transitions; 3,259 with at least 8 samples each side; 667 streamers |
| Notification text | 2026-01 to 2026-07 only | 1,198 notified sessions, 47 streamers |
| `twitch_raid_history` | outgoing raid flag from 15 minutes before to 5 minutes after the end | per session |

First-sample coverage (sessions with a curve, all months):

| First sample | Sessions | Share |
|---|---|---|
| At or before minute 5 | 6,172 | 42.5% |
| After minute 5, up to minute 15 | 4,755 | 32.7% |
| After minute 15 | 3,597 | 24.8% |

From 2026-08-01 (5,680 sessions): 44.5% at or before minute 5, 55.5% after minute 5, 24.0% after minute 15.

Chat coverage (share of sessions with zero chat messages):

| Month | Sessions | Zero-chat, all | Pop sessions | Zero-chat, pop |
|---|---|---|---|---|
| 2026-02 | 1,214 | 67.7% | 264 | 62.1% |
| 2026-03 | 2,599 | 81.6% | 545 | 76.0% |
| 2026-04 | 1,609 | 89.1% | 311 | 94.9% |
| 2026-05 | 1,455 | 90.4% | 281 | 95.0% |
| 2026-06 | 929 | 77.3% | 196 | 93.4% |
| 2026-07 | 1,197 | 51.1% | 188 | 49.5% |
| 2026-08 | 1,625 | 39.2% | 257 | 22.6% |
| 2026-09 | 1,428 | 35.7% | 235 | 17.4% |
| 2026-10 | 2,695 | 32.1% | 515 | 12.8% |

Chat results therefore use 2026-08 onward only.

## Method

- Normalization: each viewer value divided by the streamer's median average viewers over all sessions (suffix `_n`). Size classes by that median: 0 to 4, 5 to 19, 20 to 99, 100 and up. Pop = 5 and up.
- Within-streamer comparison: for each streamer with at least 3 sessions in each group, the group difference (mean for retention and dropoff). Reported: median across streamers, shares of streamers with positive and negative differences, two-sided exact sign test on non-zero streamers, bootstrap 95% CI of the median (2,000 resamples, seed 7).
- Early sampling: first curve sample at or before minute 5. Used wherever a row says "early".
- Retention and dropoff as in the bot's metrics (`tb-monitoring/src/sessions/metrics.rs`): retention at a minute is the count at that minute over the larger of the prior peak and that count, clamped 0 to 1. Dropoff is the largest relative drop between consecutive samples.
- Chat activation: at least one chat message in the session, and non-command chatters in the first 15 minutes above the streamer's own median.
- Switch metric: mean viewers in the 10 minutes after a transition minus the 10 minutes before, divided by the streamer median. Placebo: the same difference at pseudo-switches 30 minutes before and 30 minutes after, averaged. Per streamer: real minus placebo; across streamers: median, sign test, bootstrap CI. Transitions need at least 8 samples on each side.
- Pure versus mixed: Deadlock-start sessions with no switch (pure) versus at least one switch (mixed).
- Next start: next session's start viewers divided by the streamer median. Pairs with a gap of at most 48 hours and a next session first sampled at or before minute 5.
- Confidence: high = many streamers, p below 0.01, CI excludes 0, same in early and period-split checks. Medium = p below 0.05 with at least 20 streamers, or stable across periods. Low = fewer streamers, p above 0.05, or one subset only.

## Results

### 1. Start ramp (early pop, median normalized viewers)

| Minute | Median | Sessions |
|---|---|---|
| 0 (start) | 0.185 | 863 |
| 5 | 0.279 | 747 |
| 15 | 0.652 | 717 |
| 30 | 0.826 | 691 |

102 streamers. Confidence: high for the shape.

### 2. Start features (within streamer, outcome = start viewers unless stated)

| Feature | Scope | Streamers | Sessions (feature/base) | Mean diff | Median diff | Sign p |
|---|---|---|---|---|---|---|
| Notification present | all sizes, Jan to Jul | 37 | 1,146/1,868 | +0.007 | 0.000 | 0.42 |
| Notification present, minute 5 | all sizes, Jan to Jul | 29 | 881/1,003 | -0.045 | 0.000 | 0.77 |
| Notification present, minute 15 | all sizes, Jan to Jul | 30 | 870/1,113 | -0.140 | 0.000 | 0.049 |
| Notification present, minute 30 | all sizes, Jan to Jul | 31 | 863/1,120 | -0.180 | -0.093 | 0.027 |
| Notification, same streamer and month, start | all sizes | 96 | | +0.025 | 0.000 | 0.88 |
| Notification, same streamer and month, minute 30 | all sizes | 80 | | -0.107 | 0.000 | 0.03 |
| Notification, early subset, minute 15 | early, all sizes | 5 | 68/243 | -0.343 | -0.279 | 0.63 |
| Notification, early subset, minute 30 | early, all sizes | 5 | 68/232 | -0.621 | -0.286 | 0.38 |
| Title has "?" | pop | 17 | 155/929 | +0.048 | 0.000 | 0.34 |
| Title has emoji | pop | 13 | 386/421 | -0.073 | -0.019 | 0.34 |
| Title has "\|" separator | pop | 14 | 431/334 | -0.069 | -0.120 | 0.39 |
| Title 60 or more characters | pop | 43 | 762/954 | -0.007 | 0.000 | 1.00 |
| Title under 25 characters | pop | 20 | 328/699 | -0.054 | 0.000 | 0.80 |
| Title contains "deadlock" | pop | 37 | 568/848 | +0.001 | 0.000 | 0.71 |
| Start 18:00 to 22:59 Berlin | pop | 41 | 741/902 | +0.072 | 0.000 | 0.50 |
| Weekend start | pop | 52 | 609/1,335 | +0.026 | 0.000 | 0.88 |
| Gap under 3 hours since previous stream | pop | 39 | 350/1,366 | +0.048 | +0.108 | 0.24 |
| Gap 48 hours or more since previous stream, minute 15 | pop | 21 | 224/696 | -0.108 | -0.093 | 0.02 |

Other checks:
- Title word scan: 22 tokens tested out of 245 candidates (at least 5 streamers). Best: "discord", start median +0.143, mean +0.234, 10 streamers, p 0.008, Benjamini-Hochberg q 0.17. Not robust.
- Weekend, early subset, minute 5: +0.099 mean (10 streamers, p 0.07).
- Gap 48 hours or more, early subset, minute 5: -0.098 mean (11 streamers, p 0.07).

Confidence: no start lift from notification, title, hour, weekend or gap (medium). The mid-stream notification dip and the 48-hour-gap dip are low confidence and not confirmed in the early subset.

### 3. Retention, dropoff, title, duration, start hour and game (within streamer)

| Feature | Outcome | Scope | Streamers | Sessions (feature/base) | Mean diff | Sign p | Note |
|---|---|---|---|---|---|---|---|
| Title has "?" | retention_10m | pop | 16 | 131/747 | +0.024 | 0.013 | early: 7 streamers, +0.048, p 0.016; early dropoff +0.042 (worse), p 0.016; directions conflict |
| Duration above own median | retention_20m | pop | 49 | 853/692 | +0.020 | 0.006 | early: 13 streamers, +0.049, p 0.09; not used |
| Start 18:00 to 22:59 Berlin | retention_5m | pop | 29 | 552/663 | -0.012 | 0.12 | dropoff +0.034, p 0.052; early: 8 streamers, +0.005, p 0.73 |
| Deadlock vs other game | retention_5m | all sizes | 42 | 2,343/1,488 | +0.049 | 0.003 | early: 38 streamers, +0.028, p 0.12; from Aug: 20 streamers, -0.017, p 1.0 |
| Deadlock vs other game | dropoff | all sizes | 42 | 2,268/1,482 | -0.101 | 0.004 | early: 0.000 (ties); from Aug: 20 streamers, -0.061, p 0.5 |

Chat activation (from 2026-08, chat above zero; 0 to 4 = small streams):

| Group | Streamers | Sessions (feature/base) | Outcome | Mean diff | CI95 | Sign p |
|---|---|---|---|---|---|---|
| All sizes | 59 | 604/995 | retention_5m | +0.053 | 0.000 to 0.067 | 0.0004 |
| All sizes | 59 | 604/995 | retention_10m | +0.063 | 0.000 to 0.068 | 0.011 |
| All sizes | 59 | 604/995 | retention_20m | +0.029 | -0.029 to 0.046 | 0.79 |
| All sizes | 56 | 590/955 | dropoff | +0.005 | 0.000 to 0.000 | 1.00 |
| Size class 0 to 4 | 41 | 366/926 | retention_5m | +0.063 | 0.004 to 0.086 | 0.0001 |
| Size class 5 to 19 | 11 | 93/122 | retention_5m | +0.005 | 0.000 to 0.006 | 1.00 |
| Pop (5 and up) | 12 | 124/135 | retention_5m | +0.004 | 0.000 to 0.003 | 1.00 |
| Pop, early | 7 | 101/85 | retention_5m | +0.009 | 0.000 to 0.007 | 0.63 |

Pooled over all chat-tracked sessions (not restricted to August onward, coverage-affected, not used): chatters above median, retention_5m +0.049 (77 streamers, p 0.0002); zero chat versus any chat, retention_5m -0.044 (74 streamers, p 0.025).

Confidence: chat activation for small streams low to medium; title, duration and start hour low (no robust effect); game choice not robust.

### 4. Game switching (switch effect minus placebo, normalized)

| Switch | Streamers | Transitions | Median | Mean | CI95 | Positive/negative streamers | Sign p |
|---|---|---|---|---|---|---|---|
| Just Chatting to Deadlock | 25 | 52 | +0.107 | +0.109 | 0.003 to 0.318 | 18/7 | 0.043 |
| Deadlock to Just Chatting | 64 | 127 | +0.040 | +0.051 | 0.012 to 0.093 | 41/22 | 0.023 |
| Deadlock to other game | 397 | 1,152 | 0.000 | +0.031 | 0.000 to 0.014 | 188/162 | 0.18 |
| Other game to Deadlock | 186 | 665 | 0.000 | +0.016 | -0.019 to 0.026 | 89/86 | 0.88 |
| Just Chatting to other game | 17 | 106 | +0.001 | +0.133 | -0.051 to 0.161 | 9/7 | 0.80 |
| Other game to Just Chatting | 16 | 108 | 0.000 | +0.121 | -0.058 to 0.042 | 7/8 | 1.00 |
| Other game to other game | 32 | 791 | +0.011 | -0.004 | -0.019 to 0.054 | 18/13 | 0.47 |

Real-window medians before placebo subtraction: Just Chatting to Deadlock +0.129, Deadlock to Just Chatting +0.043.

Pure versus mixed, and start in Just Chatting:

| Comparison | Streamers | Sessions | Median | Mean | CI95 | Sign p |
|---|---|---|---|---|---|---|
| Mixed minus pure (Deadlock starts), average viewers | 141 | 1,283 mixed / 3,826 pure | +0.044 | +0.119 | -0.001 to 0.107 | 0.108 |
| Mixed minus pure, peak viewers | 141 | 1,283 / 3,826 | +0.071 | +0.145 | -0.007 to 0.194 | 0.148 |
| Any switch vs none (all starts), average viewers | 154 | 1,945 / 4,886 | +0.018 | +0.121 | -0.015 to 0.075 | 0.26 |
| Start in Just Chatting vs pure Deadlock, average viewers, at least 1 session per group | 12 | 139 / 487 | +0.175 | +0.301 | -0.077 to 0.650 | 0.146 |
| Start in Just Chatting vs pure Deadlock, average viewers, at least 3 sessions per group | 5 | 129 / 250 | +0.536 | +0.538 | 0.020 to 1.027 | 0.063 |

Descriptive only: 160 Just Chatting starts have a raw median of 8.0 average viewers against 2.1 for pure Deadlock starts. This reflects which streamers start in Just Chatting, not a within-streamer effect.

Confidence: Just Chatting to Deadlock and Deadlock to Just Chatting low to medium. Deadlock to other and other to Deadlock: no effect. Mixed versus pure: null (low). Start in Just Chatting: inconclusive (low; the 5-streamer version is positive in all five but too small to act on).

### 5. Stream end and next start

End profile (early pop, 666 sessions, 77 streamers; median viewers as share of the curve maximum):

| Minutes before end | Share of curve maximum |
|---|---|
| 0 to 4 | 0.664 |
| 10 to 14 | 0.678 |
| 30 to 34 | 0.683 |
| 55 to 59 | 0.696 |

The curve declines gently into the end with no cliff; the lowest share is in the final 5 minutes.

Other end measures:
- Median end viewers divided by peak: 0.684 in early pop (850 sessions). Across all 14,345 sessions with end and peak values: 0.600. The early subset is not representative for this measure.
- Median minutes from the last sample above half of peak to the end: 0.0 (early pop, 863 sessions; all 14,526 sessions with a curve give 0.0 too).
- Median minutes since the last non-command chat message before the end: 1.21 (early pop, 374 sessions); 6.28 (from 2026-08, 3,703 sessions); 10.05 (all months from February, 5,679 sessions). The value depends on subset and chat coverage. Not used as a measure.

End features and next start (within streamer; outcome = next session's start viewers, normalized; next session first sampled at or before minute 5, gap at most 48 hours):

| Current-session feature | Scope | Streamers | Sessions | Mean diff | Median diff | CI95 | Positive/negative streamers | Sign p |
|---|---|---|---|---|---|---|---|---|
| End/peak above own median, current early-sampled | pop | 12 | 242/269 | +0.172 | +0.113 | 0.067 to 0.217 | 12/0 | 0.0005 |
| End/peak above own median, current early-sampled | all sizes | 56 | 1,591/1,877 | +0.120 | +0.064 | 0.036 to 0.114 | 40/13 | 0.0003 |
| End/peak above own median, unrestricted current | pop | 12 | 265/284 | +0.164 | +0.121 | 0.045 to 0.235 | 11/1 | 0.006 |
| End/peak above own median, Feb to Jul | all sizes | 41 | 949/1,145 | +0.066 | +0.048 | 0.004 to 0.136 | 27/13 | 0.039 |
| End/peak above own median, from Aug | all sizes | 34 | 612/712 | +0.139 | +0.099 | 0.013 to 0.141 | 25/8 | 0.005 |
| Chat silent more than 10 min before end, current early-sampled | all sizes | 34 | 1,006/718 | -0.149 | -0.071 | -0.142 to -0.041 | 8/26 | 0.003 |
| Chat silent more than 10 min before end, unrestricted | all sizes | 39 | 1,054/748 | -0.121 | -0.076 | -0.127 to -0.016 | 11/28 | 0.010 |
| Chat silent more than 10 min before end, Feb to Jul | all sizes | 21 | 468/271 | -0.124 | -0.033 | -0.158 to 0.000 | 5/14 | 0.064 |
| Chat silent more than 10 min before end, from Aug | all sizes | 23 | 490/400 | -0.171 | -0.079 | -0.134 to 0.027 | 7/16 | 0.093 |
| Outgoing raid at end, current early-sampled | all sizes | 38 | 1,305/1,838 | -0.067 | -0.023 | -0.087 to 0.033 | 16/21 | 0.51 |
| Outgoing raid at end, unrestricted | all sizes | 42 | 1,374/1,980 | -0.078 | -0.036 | -0.116 to 0.000 | 14/27 | 0.060 |
| Long tail (curve below 50% of peak), above own median | pop | 8 | 127/339 | -0.044 | -0.058 | -0.090 to 0.018 | 2/6 | 0.29 |

Confidence: end near peak followed by a stronger next start, medium (positive in both periods and under early sampling, but peak spikes can drive the ratio). Chat silent before end, low (significant in the restricted subset only). Outgoing raid at end, no measurable effect (low confidence in the null). Long tail, null.

### 6. Strong previous session and next start

| Previous-session feature | Scope | Streamers | Sessions | Mean diff | Median diff | CI95 | Sign p | Note |
|---|---|---|---|---|---|---|---|---|
| Average viewers above streamer median, both early-sampled | pop | 11 | 249/243 | +0.115 | +0.107 | 0.006 to 0.183 | 0.065 | main restricted result |
| Average viewers above streamer median, both early-sampled | all sizes | 53 | 1,820/1,571 | +0.041 | +0.037 | -0.002 to 0.070 | 0.25 | |
| Average viewers above streamer median, unrestricted | pop | 13 | 278/286 | +0.123 | +0.094 | 0.050 to 0.183 | 0.023 | not used: late samples in the previous session inflate it |
| Average viewers above streamer median, unrestricted | all sizes | 80 | 2,148/2,008 | +0.034 | +0.031 | 0.000 to 0.078 | 0.064 | |
| Average viewers above streamer median, streamers with median 0 to 4 | all sizes | 67 | 1,870/1,722 | +0.016 | +0.005 | -0.008 to 0.058 | 0.37 | |
| First-time chatters above streamer median, from June | pop | 10 | 118/181 | +0.004 | -0.033 | -0.110 to 0.086 | 0.34 | |
| Previous session had outgoing raid | pop | 6 | 52/393 | -0.157 | -0.059 | -0.529 to 0.118 | 0.69 | |

Confidence: low. The pop effect shrinks from p 0.023 (unrestricted) to p 0.065 (both early-sampled). The all-size effect is not significant. Not established.

## Actionable tips

| # | Tip | Effect (within streamer) | Support | Confidence |
|---|---|---|---|---|
| 1 | Judge a stream from about minute 15. Viewers climb from 0.19 of the median at start to 0.65 at minute 15 and 0.83 at minute 30. | ramp, not a lever | 717 sessions at minute 15, 102 streamers | high (shape) |
| 2 | Do not expect the live notification to lift starting viewers. | start +0.007 | 37 streamers, p 0.42 | medium (null) |
| 3 | Notified sessions had lower viewers at minutes 15 and 30. Do not act on this yet. | -0.140 and -0.180 | 30 and 31 streamers, p 0.049 and 0.027; early subset 5 streamers, p 0.63 and 0.38 | low, unresolved |
| 4 | In streams with a median of 0 to 4, more non-command chatters in the first 15 minutes go with higher 5-minute retention. Not seen at median 5 and up. | retention_5m +0.063 (class 0 to 4) | 41 streamers, p 0.0001; pop +0.004 (12 streamers) | low to medium |
| 5 | Switching Just Chatting to Deadlock gave the largest positive effect against placebo; Deadlock to Just Chatting also rose. No switch measurably lowered viewers. | +0.107; +0.040; 0.000 | 25 streamers (52 transitions), p 0.04; 64 streamers (127), p 0.02; 397 streamers, p 0.18 | low to medium (reverse causality possible) |
| 6 | Mixed sessions (Deadlock plus a switch) show no penalty against pure sessions. | +0.044 | 141 streamers, p 0.11 | low (null) |
| 7 | Starting in Just Chatting shows no clear gain or loss. | +0.175 (12 streamers); +0.536 (5 streamers) | p 0.15 and p 0.06 | low (inconclusive) |
| 8 | Keep the end near the peak. A strong finish goes with a stronger next start. | +0.120 mean | 56 streamers, 1,591 pairs, p 0.0003; pop +0.172, 12 of 12 positive | medium |

Orchestrator note on tip 8: withdrawn. V8 in `VERIFY-ORCHESTRATOR.md` merges reconnects into one broadcast; the effect on the next start is then +0.007 (CI -0.015 to +0.025, 74 streamers, with level, duration and peak controls; placebo against the previous start -0.006). Session pairs that were reconnects of one broadcast produced the raw figure.

| 9 | Chat silent for more than 10 minutes before the end goes with a weaker next start. | -0.149 mean | 34 streamers, p 0.003; not significant within either period alone | low |
| 10 | An outgoing raid at the end shows no measurable effect on the next start. | -0.067 | 38 streamers, p 0.51 | low (null) |
| 11 | Do not plan around a strong previous session. | pop +0.115; all sizes +0.041 | 11 streamers, p 0.065; 53 streamers, p 0.25 | low; not established |
| 12 | Title, start hour, weekend and gap are not robust levers. A "?" title showed a retention_10m lift that does not hold under early sampling for dropoff. | +0.024 | 16 streamers, p 0.013; early 7 streamers, p 0.016 | low; do not act |
| 13 | Game choice is not a robust retention lever. | +0.049 at all sampling, +0.028 early, -0.017 from Aug | 42 streamers, p 0.003; early 38, p 0.12 | low; do not act |

## Caveats and dead ends

- All effects are observational and within streamer. Retention uses the larger of prior peak and current count as denominator, so it can look stable after a drop.
- Dropoff depends on the sample grid and has many ties at zero. The Deadlock-versus-other dropoff gap disappears under early sampling. A second script version gave -0.128 (median -0.092) for the same 42 streamers; the reported -0.101 comes from the query version that also gives the early and August figures. The discrepancy is not resolved, and dropoff results are not used for tips.
- Late sampling: 24.8% of sessions with a curve have their first sample after minute 15 (all months), 24.0% from 2026-08. The brief's figure of about 60% was not supported.
- Pooled chat results across all chat-tracked months are coverage artifacts (zero-chat share in pop sessions reached 93 to 95% in April to June). Not used.
- Previous-session results: unrestricted values are inflated by late samples in the previous session. Restricted values are reported.
- End-to-next-start: the end-to-peak ratio is confounded by peak spikes; the analysis cannot separate sustained audience from spikes.
- End figures: the earlier 0.688 end/peak value did not reproduce; the current value is 0.684 (early pop, n 850), and 0.600 across all sessions. The earlier 10.05-minute chat gap is the all-months median (n 5,679), and the early-pop median is 1.21 (n 374). Both are coverage-limited.
- Switches: placebo windows remove only part of the time trend; viewers may follow the switch itself. Just Chatting groups are small (52 and 127 transitions). Transitions exist only from 2026-03-01.
- Notification: text is available only for 2026-01 to 2026-07. The early subset has 5 streamers, so the mid-stream dip is not confirmed.
- Title scan: 22 tokens tested out of 245 candidates; the best hit has q 0.17.
- Small streams: normalization by a median below 5 is noisy. Pop restriction is used where it matters.
- Database: the all-period first-sample query crashed the shared Postgres server during the run (recovery mode; OOM kill at 20:00 per the coordinator). It was not re-run. The 2026-08 figures come from a query that completed before the crash. The all-period shares come from the saved per-session extract.
- Not used: `twitch_stats_category` (Deadlock-only snapshots), `follower_delta` (not needed for these questions), lurker baselines (not applicable to P11).
- Several sub-subsets have low power (for example the 5-streamer early notification subset and the 5-streamer JC-start subset).
