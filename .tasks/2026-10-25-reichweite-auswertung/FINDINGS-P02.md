# FINDINGS P02: Stream tags

Saved by the orchestrator from the P02 agent's final report (the agent's Write call was rejected). Queries: `queries-P02.sql`.

## Question

Which tags, tag combinations and tag counts go with more viewers, more first-time chatters (ftc), more chat per viewer and follower gains? Which tags do German streamers underuse while they correlate with viewers within channel across the category? What happens when a streamer changes its tag set?

## Data used

- **twitch_stream_sessions**, German Deadlock universe, 2026-03-12 to 2026-10-25. One row per stream_id after dedupe (max duration, then max id). 9,658 sessions, 1,531 streamers, 1,047 sessions with empty tags, 2,473 with null follower_delta.
- **Main analysis:** sessions of at least 30 minutes: 8,421 sessions, 1,448 streamers (138 with 12 or more sessions).
- **Monthly sessions:** Mar 1,188; Apr 1,420; May 1,233; Jun 696; Jul 791; Aug 1,196; Sep 967; Oct (to 25th) 2,167.
- **Chat counts:** the session fields `unique_chatters` and `first_time_chatters` are not used. They include lurker rows (P01): in the chat window the field sums to 66,452 chatters, while only 18,622 logins sent at least one message. Chat counts come from twitch_session_chatters instead: chatters = logins with `messages >= 1`, first-time chatters = the same with `is_first_time_streamer`, bots excluded (logins present in 30 or more channels in the window). Sessions without any tracked chatter row (131) count as missing, not zero. Chat window: sessions from 2026-06-22 with at least 30 minutes, 4,359 sessions, 971 streamers.
- **follower_delta:** available from March 2026.
- **Category:** category_snapshots_normalized, time-weighted viewers (sum of viewer_count times sample_seconds over sum of sample_seconds). Rows exist only for 2026-10-09 to 2026-10-25 (17 days). Per-day exports for 2026-09-27 to 2026-10-08 returned no rows. Universe: 36,223 channels, 544 German.
- **twitch_stats_category tag pairs:** exported monthly (Feb to Oct) for a census. Not used in the reported effects.

## Method

- **Within-streamer design.** Each session is compared with the same streamer's other sessions. Avg and peak viewers are divided by the streamer's own median. ftc is divided by the streamer's own mean. Unique chatters per avg viewer (uc/avg) is divided by its own median.
- **Effect:** per streamer, median (or mean) of tagged minus untagged normalised values; then median over streamers.
- **Inference:** two-sided sign test over streamers with both groups (at least 2 sessions each); bootstrap 95% CI over streamers (1,000 resamples). Same-ISO-week control for singles and families.
- **Size classes** by streamer median avg viewers: A 0-4, B 5-19, C 20-99, D 100+. Class D had no estimable group.
- **Tag families** by regex on tag text. Single tags use the exact tag. Tag count buckets 0, 1-3, 4-7, 8-10 are each compared with the rest of the same class.
- **Category:** per channel, time-weighted viewers with the tag vs. the same channel without it (both sides at least 600 s); log ratio. 958 tags have at least 20 channels; 131 were tested (at least 10 channels with both sides). Benjamini-Hochberg across the 131.
- **Tag-set change:** consecutive sessions with Jaccard below 0.5; each side stable for at least 5 sessions; no other change within 5 sessions. Residual of log1p(avg viewers) against a linear pre-trend on the 10 prior sessions, averaged over the 10 following sessions. Placebo: the same window 10 sessions earlier.
- **Follower:** within-streamer percentile of follower_delta from 2026-03-12.

## Results

### 1. Named families, avg viewers (main universe, within-streamer)

| Family | Streamers | Effect | 95% CI | Share positive | p (sign) |
|---|---|---|---|---|---|
| DropsEnabled | 14 | +0.163 | [+0.078, +0.466] | 0.79 | 0.057 |
| Deutsch (tag) | 53 | -0.083 | [-0.205, +0.052] | 0.40 | 0.169 |
| German (tag) | 17 | -0.047 | [-0.451, +0.075] | 0.47 | 1.000 |
| English (tag) | 34 | -0.040 | [-0.138, +0.075] | 0.47 | 0.864 |
| Deadlock (tag) | 48 | -0.036 | [-0.189, +0.113] | 0.42 | 0.312 |
| Gaming (tag) | 18 | -0.115 | [-0.342, +0.056] | 0.39 | 0.481 |

Class split:
- DropsEnabled: A n=9, +0.287 (CI [+0.099, +1.103], p=0.039, uncorrected); B n=2, +0.154; C n=1, -0.285; D n=2, +0.017.
- Deutsch: A n=45, -0.080 (p=0.371); B n=6, -0.126 (all six negative, p=0.031, uncorrected); C n=2, +0.553.

### 2. Single tags, avg viewers (within-streamer)

Only 11 of 72 candidate tags had at least 10 streamers with both groups. Streamer counts are as shown in the table; the family and single-tag definitions differ.

| Tag | Streamers | Effect | 95% CI | p | Same-week effect (n, p) |
|---|---|---|---|---|---|
| dropsaktiviert | 51 | +0.096 | [-0.030, +0.212] | 0.322 | +0.102 (35, 0.175) |
| german | 28 | +0.047 | [-0.181, +0.166] | 1.000 | 0.000 (18, 1.000) |
| ranked | 17 | +0.046 | [-0.085, +0.155] | 0.332 | +0.158 (10, 0.344) |
| english | 50 | +0.041 | [-0.079, +0.119] | 0.480 | -0.014 (35, 1.000) |
| chill | 15 | +0.007 | [-0.272, +0.039] | 1.000 | -0.238 (9, 0.508) |
| deutsch | 96 | +0.001 | [-0.083, +0.075] | 1.000 | +0.023 (81, 0.824) |
| deadlock | 76 | -0.037 | [-0.163, +0.074] | 0.248 | -0.062 (52, 0.065) |

### 3. Tag count, avg viewers (bucket vs. rest of same class)

| Tags | A: n | A: effect | A: 95% CI | A: p | B: n | B: effect | B: p |
|---|---|---|---|---|---|---|---|
| 0 | 38 | +0.109 | [-0.065, +0.270] | 0.418 | 6 | +0.126 | 0.031 |
| 1-3 | 35 | -0.001 | [-0.186, +0.146] | 1.000 | 8 | -0.111 | 0.289 |
| 4-7 | 63 | -0.119 | [-0.237, +0.046] | 0.207 | 10 | -0.010 | 0.754 |
| 8-10 | 26 | +0.019 | [-0.082, +0.151] | 0.557 | 8 | +0.112 | 0.727 |

Class C buckets have 1 to 3 streamers each and are not interpretable.

### 4. Chat window (sessions from 2026-06-22), class A

ftc and uc are message-based counts (see Data used).

| Tags | ftc effect (n, p) | uc/avg effect (n, p) |
|---|---|---|
| 0 | +0.000 (27, 1.000) | +0.081 (24, 0.839) |
| 1-3 | -0.039 (16, 0.424) | +0.178 (16, 0.804) |
| 4-7 | +0.044 (34, 0.860) | -0.129 (29, 0.265) |
| 8-10 | -0.178 (12, 0.774) | -0.129 (9, 1.000) |

Named families, all classes, chat window (median effect, n, p):

| Family | ftc (n, effect, p) | uc/avg (n, effect, p) |
|---|---|---|
| Deutsch | 72, +0.311, 0.120 | 60, +0.122, 0.519 |
| German | 20, +0.029, 1.000 | 15, +0.156, 0.607 |
| English | 28, +0.145, 0.701 | 20, +0.154, 0.824 |
| Deadlock | 41, +0.000, 1.000 | 34, +0.149, 0.864 |
| Gaming | 18, +0.203, 0.481 | 14, +0.270, 0.180 |
| Ranked | 10, +0.205, 0.344 | 6, +0.319, 0.688 |
| DropsEnabled | 37, -0.159, 0.175 | 33, -0.567, 0.296 |

DropsEnabled control: the window holds 111 DropsEnabled sessions (58 streamers), 53 of them in October. Across 50 streamer-months with both kinds of session, the ftc median raw difference is -0.41 (18 positive, 26 negative, p=0.291). The chatter count (uc) median raw difference is -1.0 (11 positive, 33 negative, p=0.001, uncorrected): within the same streamer and month, DropsEnabled sessions have about one real chatter fewer. Raw medians: ftc 1 vs 1; uc 2 vs 2.

### 5. Category, within-channel (2026-10-09 to 2026-10-25)

Universe: 36,223 channels, 544 German. 131 tags tested; all BH q = 1.00.

| Tag | Channels using tag | Share of German channels | Share of other channels | Tested channels | Within-channel change | 95% CI (pts) | p |
|---|---|---|---|---|---|---|---|
| rerun | 534 | 0.002 | 0.015 | 71 | +5.8% | [+0.2, +13.8] | 0.022 |
| arcraiders | 115 | 0.000 | 0.003 | 11 | +25.6% | [+0.2, +48.7] | 0.065 |
| lash | 70 | 0.000 | 0.002 | 13 | +18.7% | [+2.3, +29.5] | 0.092 |
| leagueoflegends | 173 | 0.017 | 0.005 | 18 | +19.3% | [-5.5, +32.8] | 0.096 |

- 36 tags have a positive change and a lower German share than other-channel share (underused candidates). None is significant after correction.
- moba (-3.4%, p=0.023) is the only other raw p below 0.05, and it is negative.
- leagueoflegends is overused in German channels, not underused.

### 6. Tag-set changes

- 17 events in 16 streamers (class A 13, B 3, C 1). Positive 7, negative 10.
- Median residual -0.086 (log1p scale); bootstrap 95% CI over events [-0.238, +0.165]; sign p=0.63.
- Median of streamer medians -0.058 (7 positive, 9 negative, sign p=0.80).
- Placebo (n=6 valid): median +0.042, so the placebo does not separate from the event effect.
- One event has a residual of +3.89 and is not interpreted. Without events with |residual| of 3 or more, the median is -0.104 (n=16).

### 7. Follower gains

- Follower-delta percentile medians are 0 in every tag-count bucket (0: n=44; 1-3: n=35; 4-7: n=61; 8-10: n=28), with CIs [0, 0].
- Mean effects between -0.018 and +0.037, p between 0.774 and 1.000. Ties dominate, so this metric carries no tag signal.

## Actionable tips

1. **Do not change tags to gain viewers.** No tag or count shows a measurable viewer effect (sections 1 to 3). Closest candidate: DropsEnabled, +0.163 (n=14, p=0.057). Confidence: low.
2. **The German tag does not measurably help discovery or viewers.** Viewers -0.083 (n=53, CI [-0.205, +0.052]); ftc +0.311 (n=72, CI [-0.020, +0.603], p=0.120). Confidence: medium that no large viewer effect exists; low for the direction of any chat effect.
3. **DropsEnabled is an observation, not a lever.** Class A shows +0.287 viewers (n=9, p=0.039, uncorrected) and the same-week control gives +0.102 (n=35, p=0.175). In the same streamer-month, DropsEnabled sessions have about one real chatter fewer (median -1.0, 11 vs 33 streamer-months, p=0.001, uncorrected). Drops may add watchers who do not talk. Confidence: low for viewers, low to medium for fewer chatters.
4. **No tag-count target.** Class A 4-7 tags -0.119 (n=63, p=0.207); 0 tags +0.109 (n=38, p=0.418). Confidence: low.
5. **No tag count changes chat per viewer.** With message-based chatter counts, 0-tag sessions show +0.081 uc/avg (n=24, p=0.839) and 4-7 tags -0.129 (n=29, p=0.265). The earlier 0-tag signal (p=0.036) came from the lurker-inflated session field and is gone. Confidence: low to medium that no large effect exists.
6. **Underused category tags (rerun, arcraiders, lash) are candidates for a later controlled check, not for action.** Rerun is the only raw p below 0.05 among positive candidates, and it does not survive correction. Confidence: low.
7. **Tag edits have no measurable viewer cost or gain.** Median -0.086 with CI [-0.238, +0.165]. Confidence: low to medium that there is no large effect.
8. **No tag tip for follower gains.** The metric is tie-dominated.

## Caveats

- Association only. Tags are chosen by the streamer, not randomised.
- Time confound: DropsEnabled sessions cluster in October, where most chat-window data sits.
- Chat coverage starts 2026-06-22. Chat results rest on 4,359 sessions, most in October. Chat counts are message-based from twitch_session_chatters; the session fields are not used.
- Category covers 17 days only (2026-10-09 to 2026-10-25).
- Size: support is mostly class A (13 of 17 events; 36 of 58 DropsEnabled streamers). Classes B and C are thin; class D is not estimable. Results mainly describe channels under about 20 average viewers.
- Multiple comparisons: only the category run is corrected (BH over 131). Other p-values are uncorrected. Cells with p from 0.03 to 0.09 must not be read as findings.
- Definitions differ: exact single tag (deutsch +0.001) vs. regex family (Deutsch -0.083) use different streamer sets.
- Event residuals are on the log1p scale, not percentages.

## Dead ends

- Early German discovery effect (ftc -0.408) was an artifact of empty-tag rows and untracked chat fields. Dropped.
- Single-tag ftc column is invalid (tied percentile ranks, median 0). Replaced by the mean-normalised window run (section 4).
- Session chat fields: `unique_chatters` and `first_time_chatters` include lurker rows (66,452 field chatters vs 18,622 message senders in the window). The earlier chat results built on them (DropsEnabled ftc -0.810, p=0.007; 0-tag uc/avg +0.081, p=0.036) are withdrawn and replaced by the message-based run in section 4.
- Follower delta: tie-dominated, no signal.
- Category: no tag survives BH.
- Tag-set change placebo does not separate from the event effect.
- Earlier tag-change figures (p=0.064 and two sensitivity variants, -0.136 and +0.099) could not be reproduced in the re-check. They are not reported.
- Lurker baselines: not computed (see summary, point 10).

## Files

- Queries: /home/nathanael/.worktrees/twitch-reichweite/.tasks/2026-10-25-reichweite-auswertung/queries-P02.sql (sessions export, message-based chatter counts per session, tag-pair template, per-day category export template; date windows as placeholders).
- FINDINGS-P02.md: not written (Write rejected). This text is the deliverable.
- Scratch exports and scripts (not deliverables): /tmp/reichweite-scratch/P02/
