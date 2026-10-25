# P01: Stream titles and reach

Saved by the orchestrator from the P01 agent's final report (the agent's Write call was rejected). Queries: `queries-P01.sql`.

## Question

Which title words, phrases and features go with more average viewers (discovery), more first-time chatters (discovery) and more chat interaction (unique chatters per average viewer), compared within each streamer? Does changing the title during a stream lift viewers? Do opening titles or title-generator keywords matter?

## Data used

| Source | Time range | Rows | Streamers | Sessions or segments |
|---|---|---|---|---|
| Stats segments (`twitch_stats_category`, German filter) | 2025-10-10 to 2026-10-25 | 14,907 segments | 2,508 channels; 2,182 with Deadlock label (12,376 Deadlock segments) | 1,039 segments with a title change |
| Title change events | Feb to 25 Oct 2026 | 1,315 events (Feb 141, Mar 206, Apr 153, May 127, Jun 100, Jul 100, Aug 153, Sep 107, Oct 228) | 196 in clean DiD sample (568 clean events) | 1,039 segments |
| Sessions (`twitch_stream_sessions`, German Deadlock, with title) | from 2026-01-09 | 12,258 sessions | 1,748 | 12,258 sessions |
| Chat (`twitch_session_chatters`, tracked) | 2026-07-13 to 2026-10-25 | 5,265 sessions in window; 5,132 with tracked chatter rows; 5,109 in test base | 979 | 5,109 sessions |
| Title generator (`title_generator_knowledge`) | added 2026-04-19 to 2026-10-25 | 1,787 titles (1,781 with keywords) | 209 source streamers | not applicable |

Coverage notes:
- Probe 2025-11-03: 477 rows, 0 with a title, 0 with the Deadlock label. Probe 2026-03-20: 47,182 rows, all titled and labelled Deadlock. Title analyses therefore start in February 2026.
- Tracked chatter rows exist for 96 to 99% of sessions in each week. At least one non-bot chatter with messages >= 1 exists for 49 to 69% of sessions per week.
- Session chat fields are unusable: zero from 2026-02-23 to 2026-06-01 (earlier probe), and `unique_chatters` includes lurker rows (earlier probe, 10 to 23 Aug: field sum 7,019 vs 1,855 chatters with messages >= 1).

## Method

- Outcomes. Session viewers: y_avg = log2(avg_viewers + 1). Segment viewers: time-weighted average viewers. Chat: uc = non-bot chatters with messages >= 1; ftc = their first-time-streamer flags. y_int = log2((uc+1)/(avg+1)); y_ftc_rate = log2((ftc+1)/(avg+1)); y_ftc = first-time chatter outcome on a log2(ftc+1) scale.
- Bots: 51 logins seen in 30 or more channels, excluded from chat counts only.
- Segments: a gap over 1,800 s starts a new segment. A row's time weight is the gap to the next row when that gap is at most 600 s, else 0. A title change is a title that differs from the previous row inside a segment.
- Within-streamer lift: per streamer, mean outcome with the feature minus mean without (at least 2 sessions per group). Reported as the median across streamers, the share of positive streamers, a two-sided exact sign test, and Benjamini-Hochberg q within each test family. Percent = (2^d - 1) x 100 on the median d.
- Word and bigram tests: lowercase tokens of length 2 or more; a term is tested when at least 10 streamers use it.
- Week adjustment: outcome minus the ISO-week mean. Applied to word tests and to the robustness rows below.
- Size classes from streamer median viewers: 0-4, 5-19, 20-99, 100+.
- Title-change DiD: a change is clean when no other change lies within 40 min. Pre and post windows are 20 one-minute bins before and after the change (at least 10 bins each). Controls are other minutes of the same segment at least 41 min from any change (at least 5 per segment). DiD = (post - pre) minus the mean control change, in log2 viewers, averaged per streamer, then median across streamers, with a bootstrap CI (2,000 resamples) and a sign test. Pre-trend = the same difference for the 20 bins before the pre window.
- Title generator: keyword present vs absent within the source streamer, outcomes log2(relative_perf) and log2(engagement_rate + 0.001); 48 keyword tests (keywords used by at least 5 source streamers).
- Feature families tested (`features_all_tests.csv`): length buckets, emoji, caps, exclamation and question marks, digits, bang commands (at least 3 streamers), brackets, language tags, pipe separator, audience address, rank names, typed hero list, Road to, streak counters, chill, ranked, neu/new, guide, drops, giveaway, scrim, patch, first-time phrases, humor/clickbait, help words, questions to the audience. The 1v1 and viewer-game families are not in the output and were not tested.

## Results

### 1. Session average viewers, raw, within streamer (from 2026-01-09)

| Feature | Streamers | Sessions with / without | Effect | Share positive | p (sign) | q (BH) |
|---|---|---|---|---|---|---|
| Title under 30 characters | 207 | 2,323 / 3,843 | -4.1% | 0.38 | 0.001 | 0.87 |
| Bang command in title | 50 | 792 / 1,500 | +9.7% | 0.68 | 0.015 | 1.00 |
| Bracket tag | 40 | 565 / 1,048 | +14.0% | 0.65 | 0.081 | 1.00 |
| Word "chill" | 35 | 175 / 1,502 | -12.5% | 0.31 | 0.041 | 1.00 |
| Word "discord" | 22 | 302 / 695 | +23.0% | 0.73 | 0.052 | 1.00 |
| Word "we" | 16 | 57 / 1,008 | +13.2% | 0.75 | 0.077 | 1.00 |
| Bang command "!discord" | 17 | 274 / 387 | +20.4% | 0.71 | 0.14 | 1.00 |
| Language tag [DE]/[ENG] | 17 | 378 / 229 | +9.3% | 0.65 | 0.33 | 1.00 |
| Word "patch" | 31 | 168 / 2,220 | -5.9% | 0.39 | 0.28 | 1.00 |
| Road to | 30 | 383 / 978 | +0.8% | 0.53 | 0.86 | 1.00 |
| Emoji | 67 | 978 / 1,807 | +3.8% | 0.57 | 0.33 | 1.00 |
| Caps-heavy title | 112 | 1,022 / 3,229 | -2.0% | 0.47 | 0.64 | 1.00 |
| Question mark | 96 | 494 / 3,816 | -1.4% | 0.47 | 0.61 | 1.00 |
| Digits | 159 | 1,851 / 3,936 | +3.2% | 0.56 | 0.15 | 1.00 |
| Rank names | 69 | 917 / 2,761 | +1.0% | 0.54 | 0.63 | 1.00 |
| Ranked word | 91 | 771 / 3,798 | -1.1% | 0.47 | 0.68 | 1.00 |
| Hero name (typed list) | 96 | 808 / 3,411 | +4.1% | 0.54 | 0.48 | 1.00 |
| Word "neu"/"new" | 95 | 551 / 4,030 | +0.1% | 0.52 | 0.84 | 1.00 |
| Humor/clickbait word | 21 | 83 / 991 | +4.6% | 0.62 | 0.38 | 1.00 |

### 2. Robustness: week-adjusted and full-history segments

| Feature | Session viewers, raw | Session viewers, week-adjusted | Segment viewers, raw (full history) | Segment viewers, week-adjusted |
|---|---|---|---|---|
| Bang command in title | +9.7% (p 0.015) | +15.1% (share 0.72, p 0.003, q 0.87) | +8.5% (55 streamers, 747 / 1,360 segments, share 0.67, p 0.014) | +17.0% (share 0.71, p 0.003) |
| Bracket tag | +14.0% (p 0.081) | +18.9% (share 0.63, p 0.15) | +13.8% (41 streamers, 523 / 1,017, share 0.68, p 0.028) | +20.7% (share 0.71, p 0.012) |
| Title under 30 characters | -4.1% (p 0.001) | -3.5% (share 0.43, p 0.037) | -2.8% (200 streamers, 2,123 / 3,399, share 0.43, p 0.056) | not checked |
| Word "chill" | -12.5% (p 0.041) | -10.4% (share 0.37, p 0.18) | -10.4% (35 streamers, 163 / 1,233, share 0.34, p 0.090) | not checked |
| Word "discord" | +23.0% (p 0.052) | +28.5% (22 streamers, share 0.77, p 0.017) | +28.1% (22 streamers, 306 / 624, share 0.73, p 0.052) | not checked |
| Road to | +0.8% (p 0.86) | +0.8% (p 0.86) | -6.2% (32 streamers, 218 / 980, share 0.31, p 0.050) | not checked |
| Word "neu"/"new" | +0.1% (p 0.84) | -0.2% (p 1.00) | +0.2% (86 streamers, 505 segments, share 0.50, p 1.00) | not checked |

Word tests, week-adjusted (2,404 tests, minimum q 1.00): "we" +15.6% (16 streamers, 57 sessions, share 0.88, p 0.004); "update" -8.1% (26 streamers, 145 sessions, share 0.27, p 0.029). Both have q 1.00.

### 3. Chat interaction and first-time chatters (tracked sessions, 2026-07-13 to 2026-10-25)

696 tests, minimum q 1.00. No row survives correction.

| Feature | Outcome | Streamers | Sessions with / without | Effect | Share positive | p (sign) | q (BH) |
|---|---|---|---|---|---|---|---|
| Word "in" | first-time chatters (y_ftc) | 45 | 167 / 877 | -17.4% | 0.27 | 0.004 | 1.00 |
| Word "grind" | first-time chatters (y_ftc) | 26 | 176 / 694 | -13.6% | 0.23 | 0.015 | 1.00 |
| Word "update" | first-time chatters (y_ftc) | 17 | 98 / 577 | -12.9% | 0.18 | 0.013 | 1.00 |
| Guide word | first-time chatters (y_ftc) | 13 | 43 / 332 | +18.2% | 0.77 | 0.039 | 1.00 |
| Bang command in title | interaction (y_int) | 18 | 202 / 373 | +26.3% | 0.78 | 0.031 | 1.00 |
| Caps-heavy title | interaction (y_int) | 59 | 384 / 1,017 | +14.9% | 0.63 | 0.067 | 1.00 |
| Title under 30 characters | viewers (y_avg) | 100 | 881 / 1,130 | -3.7% | 0.40 | 0.070 | 1.00 |
| Title under 30 characters | interaction (y_int) | 100 | 881 / 1,130 | -4.0% | 0.45 | 0.37 | 1.00 |

### 4. Title changes during a stream (within-stream DiD, log2 viewers converted to percent)

| Group | Events | Streamers | DiD (streamer median) | 95% CI | Share positive | p | Pre-trend (log2) |
|---|---|---|---|---|---|---|---|
| All | 568 | 196 | +0.1% | -0.8% to +2.0% | 0.50 | 0.72 | -0.010 |
| Size 0-4 | 357 | 143 | 0.0% | -1.0% to +1.3% | 0.47 | 0.86 | -0.025 |
| Size 5-19 | 190 | 41 | +2.5% | -0.2% to +9.9% | 0.63 | 0.12 | +0.043 |
| Size 20-99 | 8 | 6 | -1.8% | -25.6% to +39.4% (not estimable) | 0.50 | 1.00 | -0.068 |
| Size 100+ | 13 | 6 | -4.7% | -8.9% to +3.1% (not estimable) | 0.33 | 0.69 | -0.025 |
| Adds a bang command | 21 | 16 | +0.9% | -16.4% to +5.6% | 0.56 | 0.80 | -0.010 |
| Removes a bang command | 14 | 13 | -0.2% | -11.4% to +8.1% | 0.46 | 1.00 | -0.024 |
| Other changes | 533 | 186 | +0.2% | -0.4% to +2.0% | 0.52 | 0.41 | -0.010 |

Sample funnel: 1,315 change events. 568 are clean. 192 had another change within 40 min. 189 sat in segments with fewer than 5 controls. The remaining 366 (by subtraction) lacked 10 of 20 minute bins in a window.

### 5. Title-generator cross-check (within source streamer, 1,787 titles)

48 tests, minimum q 1.00 on both outcomes.

| Keyword | Source streamers | With / without titles | Median log2 diff (relative_perf) | Share positive | p |
|---|---|---|---|---|---|
| "dann" | 5 | 13 / 226 | +0.411 | 1.00 | 0.063 |
| "discord" | 10 | 92 / 268 | +0.259 | 0.70 | 0.34 |
| "wird" | 5 | 44 / 185 | +0.034 | 0.80 | 0.38 |
| "patch" | 10 | 38 / 252 | -0.079 | 0.40 | 0.75 |

"discord" on engagement_rate: +0.238 log2, share 0.80, p 0.11.

## Actionable tips

Orchestrator note: V9 in `VERIFY-ORCHESTRATOR.md` compares each session with the same streamer's sessions within 14 days. Bang command +3.1 % (p 0.44), "discord" +5.7 % (p 0.057), bracket +0.5 %, "chill" -0.8 %, short titles -1.7 % (p 0.026). Tips 1, 2, 4 and 5 are mostly growth-phase trends and are downgraded to no robust effect; tip 3 holds at about 2 %.

1. Bang command in the title (for example "!discord"). Evidence: +9.7% session viewers raw (50 streamers, 792 sessions, share 0.68, p 0.015); +15.1% week-adjusted (share 0.72, p 0.003); +8.5% in full-history segments (55 streamers, p 0.014). No chat cost seen (+26.3% interaction, 18 streamers, nominal). Confidence: medium on direction (four cuts agree), low on size.
2. Mention "discord" in the title. Evidence: +23.0% session viewers (22 streamers, 302 sessions, share 0.73, p 0.052); +28.5% week-adjusted (share 0.77, p 0.017); +28.1% in full-history segments (22 streamers, p 0.052); title-generator relative performance +0.26 log2 (10 streamers, 92 vs 268 titles, share 0.70, p 0.34). Confidence: low (no corrected test survives).
3. Keep titles at 30 characters or more. Evidence: -4.1% for short titles (207 streamers, 2,323 sessions, share 0.38, p 0.001, q 0.87); -3.5% week-adjusted (p 0.037); -2.8% in full-history segments (p 0.056). Confidence: medium on direction; the effect is small.
4. Bracket tags for channels averaging 5 to 19 viewers. Evidence: +36.1% (11 streamers, 137 sessions with, 161 without, share 0.91, p 0.012). Overall +14.0% (40 streamers, p 0.081). No effect for 0 to 4 average viewers: -1.7% (25 streamers, p 1.00). Confidence: low (11 streamers).
5. Avoid "chill" in titles. Evidence: -12.5% (35 streamers, 175 sessions, share 0.31, p 0.041); -10.4% week-adjusted (p 0.18); -10.4% in full-history segments (p 0.090). Confidence: low.
6. Do not expect reach from mid-stream title changes. Evidence: DiD +0.1% (568 clean events, 196 streamers, p 0.72). The 5 to 19 class shows +2.5% (CI -0.2% to +9.9%), but its pre-trend is +0.043 log2 (about +3%), so the apparent gain was already present before the change. Confidence: medium.
7. No title feature raises chat interaction or first-time chatters after correction. Evidence: all 696 chat tests have q 1.00. The nominal "guide" word (+18.2% first-time chatters, 13 streamers, p 0.039) and bang command (+26.3% interaction, 18 streamers, p 0.031) are too few to act on. Confidence: low.
8. "neu"/"new" has no viewer or chat effect. Changing titles for it will not change reach. Confidence: medium that the null holds for this feature.

## Withdrawn or invalid results

These come from the first pass and must not be used:
- Chat results from session chat fields. Example: bang command -13.1% interaction (50 streamers, p 0.12). The session fields are invalid (zero stretches, lurker rows included).
- Word "in" on first-time chatters in the full-period pass: -6.3% (110 streamers, p 8.5e-6, q 0.026). The invalid zero months create a spurious within-streamer difference, so it is not evidence. The clean window gives -17.4% (45 streamers, q 1.00), reported above as nominal only.
- Word "dc" on first-time chatters: -28.1% (12 streamers, p 0.11), withdrawn with the invalid field.
- Earlier monthly title-change extraction: some months returned zero events. Replaced by a weekly extraction (55 weeks, all complete); the monthly files were discarded.

## Caveats and dead ends

- Multiple testing: no valid outcome has q below 0.85. The best viewer result (short titles, q 0.87) and the week-adjusted bang result (q 0.87) do not survive correction. All tips rest on nominal p-values from 11 to 50 streamers.
- Chat covers one tracked window (2026-07-13 to 2026-10-25) and describes tracked channels only. Bot logins are excluded from chat counts only.
- Confounders: schedule, time of day, patch days, raids and hosts, and streamer habits. The week adjustment controls only category-wide trends. Bracket and bang tags may mark events or tournaments, which could explain the size dependence.
- Title analysis covers February to October 2026 only.
- The hero list was typed from game knowledge, not from a database table. Its null result (+4.1%, 96 streamers, p 0.48) may miss names.
- Dead ends: emoji (+3.8%, p 0.33); caps (-2.0%, p 0.64); question marks (-1.4%, p 0.61); digits (+3.2%, p 0.15); rank names (+1.0%, p 0.63); "Road to" (+0.8% in sessions, -6.2% in segments, inconsistent); humor/clickbait (+4.6%, p 0.38); English mix (no effect); streak counters (-2.9%, 13 streamers, p 0.27); guide word on viewers (+3.3%, 30 streamers, p 0.58); drops, giveaway and scrim words (at most 8 streamers, no effect).
- Title generator: no keyword survives (48 tests, minimum q 1.00). "dann" (+0.41 log2) rests on 5 streamers and 13 titles.
- Not covered: 1v1 and viewer-game families (not in the output); follower deltas (not used in this package); opening-title share (computed in the segment export, but no test result is reported here).
