# P06: First-time chatter to regular

Saved by the orchestrator from the P06 agent's final report (the agent's Write call was rejected). Queries: `queries-P06.sql`.

## Question

What makes a first-time chatter come back to the same channel and become a regular?

## Short answer

- The strongest signal is the newcomer's own participation in the first session. Writing 2 or more messages there goes with a 14.1 pp higher 30-day return (95% CI +11.6 to +17.0). Writing 5 or more goes with +17.9 pp.
- A broadcaster reply within 3 minutes shows no detectable effect (-3.2 pp, CI -12.0 to +8.3). Replies are not a retention lever in this data.
- Following the channel before the first message goes with +10.2 pp (CI +3.2 to +16.0). Following afterwards (+23.6 pp) is most likely reverse causality.
- Lurkers (present, never wrote in their first session) return less often than chatters: 14.3% vs 26.4% by presence within 30 days, and 1.8% vs 15.8% by writing. Lurkers who later write return more, but that group is selected.
- In the German category sample, the gap in 7-day return (15.6% vs 28.9% for all languages) comes mostly from opportunity: only 46.1% of German newcomers have any later chat day in their room within 7 days, against 85.0% overall. Among newcomers with opportunity, return is nearly equal (33.8% vs 34.0%).
- Returns are fast. The median time to a later presence is about 3 days for both lurkers and chatters, and the median time to a later chat day in the German category sample is 2 days.

## Data used

- `twitch_chat_messages`: 466,839 rows, 1,025 streamers, 5,740 sessions, 2026-01-31 to 2026-10-25.
- `twitch_session_chatters`: 165,095 rows, 1,312 streamers, 9,173 sessions. Lurkers are rows with `messages = 0` (`seen_via_chatters_api`), 130,457 rows in total. Presence analyses use sessions since 2026-02-19 (163,666 rows).
- `twitch_chat_message_labels` (first-message label), `twitch_greeted_chatters` (greeting bot).
- `category_chat_messages` since 2026-09-27: 2,052,871 room-day rows, 990,524 room-chatter pairs, 29 days.
- Bots: 77 logins seen in 30 or more channels, excluded from all populations. Names are not recorded here. Exclusions: 1,660 lurker rows and 10,633 category pairs.
- Size classes from each streamer's median average viewers (Jul to Oct): 0-4, 5-19, 20-99, 100+. The 100+ class (9 streamers among Jul-Sep newcomers, 15 overall) consists of one-off event streams and is excluded from the main estimates.
- Coverage burn-in: a newcomer counts only if their first chat is at least 30 days after the streamer's first logged session (1,310 streamers in coverage).

## Method

- Chat newcomer: first chat message of a (streamer, chatter) pair in the logged data, with no earlier chat. Presence newcomer: first session in `twitch_session_chatters` with no earlier presence. Presence-first chatters are split into lurkers (0 messages in the first session) and chatters (at least 1).
- Outcomes:
  - `ret30`: any later chat session within 30 days, excluding the first session.
  - `reg30`: at least 3 later chat sessions within 30 days.
  - Presence return: any later session in `twitch_session_chatters` within 30 days.
  - Silent return: a later session with 0 messages.
  - `ret7` (category): any later chat day in the same room within 7 days.
  - Opportunity (category): the room had any chat on a later day within 7 days.
- Within-channel estimator: Mantel-Haenszel risk difference, stratified by streamer. The strata count and positive-stratum share use strata with at least 3 newcomers per arm (`mh.py`). The CIs are a cluster bootstrap over streamer strata, 2,000 resamples, seed 20261025, using all strata with both arms (`boot.py`). Point estimates agree between the two scripts; the strata counts do not.
- Category window: first observation 2026-09-28 to 2026-10-17 (main). Lookback check: first observation 2026-10-04 to 2026-10-17.
- All queries are in `queries-P06.sql`. Raw extracts are in `/tmp/reichweite-scratch/P06/` only.

## Results

### 1. Return by size class (chat newcomers, Jul-Sep 2026, bots excluded)

| Class (median avg viewers) | n (30 d observed) | Streamers | ret30 % | reg30 % | ret7 % |
|---|---|---|---|---|---|
| 0-4 | 2,385 | 284 | 12.3 | 3.7 | 8.9 |
| 5-19 | 803 | 58 | 22.7 | 6.5 | 12.4 |
| 20-99 | 420 | 19 | 15.2 | 3.6 | 7.9 |
| 100+ (excluded) | 1,037 | 9 | 1.1 | 0.0 | 0.6 |
| Excl. 100+, no burn-in | 3,608 | 361 | 15.0 | 4.3 | 9.6 |
| Excl. 100+, burn-in 30 d | 1,945 | 126 | 15.4 | 5.1 | 10.2 |

ret7 uses the 7-day-observed set, which differs by at most 23 newcomers per row.

### 2. Return by messages in the first chat session (burn-in 30 d, excl. 100+, bots excluded)

| Messages in first session | n | Streamers | ret30 % | reg30 % | ret7 % |
|---|---|---|---|---|---|
| 1 | 547 | 98 | 4.2 | 0.9 | 2.5 |
| 2-4 | 636 | 83 | 10.7 | 3.1 | 6.5 |
| 5+ | 762 | 89 | 27.4 | 9.8 | 18.8 |

### 3. Lurkers vs chatters (presence-first newcomers, burn-in 30 d, excl. 100+, bots excluded, sessions since 2026-02-19)

| Group | n | Streamers | Presence return 30 d % | Chat return 30 d % | Silent return 30 d % |
|---|---|---|---|---|---|
| Lurkers (0 messages) | 21,283 | 146 | 14.3 | 1.8 | n/a |
| Chatters (1+ messages) | 2,374 | 138 | 26.4 | 15.8 | 18.8 |
| Chatters, 1 message | 674 | | 10.8 | 4.6 | 8.6 |
| Chatters, 2-4 messages | 735 | | 21.1 | 10.9 | 15.9 |
| Chatters, 5+ messages | 965 | | 41.2 | 27.5 | 28.2 |

- Within channel (streamers with at least 5 in each group): median presence return is 15.7% for lurkers vs 21.2% for chatters (74 streamers, median difference -7.7 pp, positive in 32% of streamers). For 1 message vs 2+ messages: 11.3% vs 31.0% (33 streamers).
- Lurkers who later write: 375 of 21,283 (1.8%). Their silent return is 66.1%, against 12.7% for the 20,908 lurkers who never write. Presence among later writers is 98.6% by construction and is not used. The comparison is selected and not causal.
- Time to first later presence (days): lurkers n=3,033, p25 1.05, median 3.52, p75 10.97, p90 20.07. Chatters n=626, p25 1.02, median 3.07, p75 8.68, p90 17.93.

### 4. Baseline reconciliation (chatters with at least 1 message, presence-first, bots excluded)

Presence and chat outcomes are shown separately. Chat outcomes come from `twitch_chat_messages`. `final_lurk.py` derives them from `session_chatters.messages > 0` and differs by at most 0.2 pp.

| Definition | n (1 msg / 2-4 / 5+) | 1 msg: presence / chat % | 2-4: presence / chat % | 5+: presence / chat % |
|---|---|---|---|---|
| Burn-in 30 d, excl. 100+ | 674 / 735 / 965 | 10.8 / 4.6 | 21.1 / 10.7 | 41.2 / 27.4 |
| No burn-in, excl. 100+ | 2,058 / 1,975 / 2,897 | 12.3 / 7.8 | 19.3 / 12.7 | 32.0 / 24.4 |
| Burn-in 30 d, incl. 100+ | 864 / 851 / 1,011 | 9.0 / 3.9 | 19.2 / 10.1 | 39.9 / 26.4 |
| No burn-in, incl. 100+ | 3,332 / 2,577 / 3,217 | 11.2 / 8.3 | 18.1 / 12.9 | 31.2 / 24.4 |

The brief's baseline of 9.5 / 17.2 / 30.5 is not reproduced by any single row. It lies between the chat and presence definitions. The closest is the no-burn-in, incl. 100+ presence row (11.2 / 18.1 / 31.2).

### 5. Broadcaster reply (exact definition)

Definition: a message from the streamer's own account within 3 minutes after the newcomer's first chat message, in the same session.

| Population | n | With reply | Outcome | Effect (MH, pp) | CI (cluster bootstrap) |
|---|---|---|---|---|---|
| Jul-Sep newcomers, excl. 100+, no burn-in | 3,631 | 247 (6.8%) | ret30 | +2.8 | -4.8 to +12.6 |
| Burn-in 30 d, excl. 100+ | 1,945 | 141 (7.2%) | ret30 (14.9% vs 15.5%) | -3.2 | -12.0 to +8.3 |
| Same, within 5 minutes | 1,945 | 173 | ret30 | -1.9 | -8.7 to +7.9 |

- Full history, any session, within 3 minutes of the first message: 974 of 18,421 newcomers (5.3%).
- The orchestrator's MH -0.7 pp (CI -6.1 to +5.3) is consistent with these estimates. Both intervals include zero. The brief's count of 602 cases is not reproduced; the nearest counts are 247 and 974.

### 6. Predictors (chat newcomers, burn-in 30 d, excl. 100+, bots excluded; outcome ret30; n=1,945)

| Predictor | Yes: n (ret30) | No: n (ret30) | MH-RD pp | 95% CI | Strata (3+ per arm) | Positive share |
|---|---|---|---|---|---|---|
| 2+ messages in first session | 1,398 (19.8%) | 547 (4.2%) | +14.1 | +11.6 to +17.0 | 41 | 0.76 |
| 5+ messages in first session | 762 (27.4%) | 1,183 (7.7%) | +17.9 | not bootstrapped | 45 | 0.71 |
| Followed before first chat | 140 (28.6%) | 1,805 (14.4%) | +10.2 | +3.2 to +16.0 | 14 | 0.86 |
| Followed within 30 d after first chat | 207 (36.2%) | 1,738 (12.9%) | +23.6 | not bootstrapped | 20 | 0.90 |
| First-message label Statement | 321 (22.1%) | 1,624 (14.1%) | +7.6 | not bootstrapped | 33 | 0.61 |
| First-message label Greeting | 393 (21.4%) | 1,552 (13.9%) | +6.7 | not bootstrapped | 32 | 0.56 |
| First-message label Question | 218 (14.2%) | 1,727 (15.6%) | -3.4 | not bootstrapped | 25 | 0.28 |
| At least 60 min of stream left at arrival | 1,439 (16.8%) | 506 (11.5%) | +4.3 | not bootstrapped | 41 | 0.59 |
| Arrived in first 30 min of stream | 322 (18.0%) | 1,623 (14.9%) | +3.1 | not bootstrapped | 33 | 0.42 |
| Chat speed 5 min before arrival at or above streamer median | 1,530 (14.0%) | 415 (20.7%) | -3.4 | not bootstrapped | 24 | 0.29 |
| Mention of login within 10 min | 294 (18.7%) | 1,651 (14.8%) | +1.8 | -2.7 to +6.4 | 23 | 0.48 |
| Another chatter wrote within 1 min | 751 (16.5%) | 1,194 (14.7%) | -1.3 | not bootstrapped | 40 | 0.28 |
| Viewers at arrival at or above streamer median | 1,042 (15.6%) | 901 (15.2%) | +1.8 | not bootstrapped | 55 | 0.33 |
| Broadcaster reply within 3 min | 141 (14.9%) | 1,804 (15.5%) | -3.2 | -12.0 to +8.3 | 9 | 0.33 |
| Bot greeting near arrival | 33 (12.1%) | 1,912 (15.5%) | -5.1 | not bootstrapped | 4 | 0.50 |
| `is_first_time_streamer` flag set | 1,672 (14.4%) | 273 (21.6%) | -4.4 | -8.4 to -0.4 | 22 | 0.45 |

Note: the `is_first_time_streamer` flag is set for 95.6% of chatters and 28.1% of lurkers (full history). It tracks writing rather than being an independent predictor (see caveats).

No detectable effect (CI includes zero, or positive share near 0.5): mention, arrival time, viewer level, other chatter writing. First-message labels Reaction (+1.8 pp) and Game-Related (+1.7 pp) are also near zero.

Robustness of the first-session message effect without burn-in (Jul-Sep newcomers, n=3,631): +11.4 pp (CI +9.2 to +13.5, 218 strata).

### 7. Category: German vs all rooms (bots excluded; first observation 2026-09-28 to 2026-10-17)

| Scope | Newcomers | Rooms | ret7 % | With opportunity: n (share) | ret7 % among opportunity |
|---|---|---|---|---|---|
| All languages | 698,013 | 26,214 | 28.9 | 593,235 (85.0%) | 34.0 |
| German | 5,041 | 360 | 15.6 | 2,323 (46.1%) | 33.8 |
| English | 487,320 | 17,108 | 29.5 | 416,894 (85.5%) | 34.5 |

Among newcomers with opportunity:

| Predicate (first day) | All languages: yes / no ret7 % (MH pp, strata, positive share) | German: yes / no ret7 % (MH pp, strata, positive share) |
|---|---|---|
| 2+ messages | 40.7 / 23.8 (+16.9, 10,518, 0.65) | 39.3 / 19.5 (+14.9, 129, 0.63) |
| 5+ messages | 49.1 / 27.8 (+21.9, 11,180, 0.65) | 45.5 / 25.2 (+16.9, 145, 0.59) |
| Broadcaster message within 2 min | 34.2 / 34.0 (-1.0, 2,999, 0.39) | 25.5 / 34.0 (-10.8, 26, 0.27; n=51) |

- German return gap, newcomers who return within 7 days (n=789): median 2 days, p25 1, p75 5, next-day share 38.0%.
- Lookback (first observation 2026-10-04 to 2026-10-17): German n=2,578, ret7 21.8%, opportunity 65.1% (ret7 33.5%). 2+ messages: 39.0 vs 19.7 (+15.9, 109 strata). 5+ messages: 45.9 vs 24.6 (+19.2, 128 strata). All languages: n=449,293, ret7 24.3%, opportunity 82.8% (ret7 29.3%), 2+ messages +14.2, 5+ messages +18.9.
- The first-day message effect holds in both windows. Absolute ret7 levels move with the window (German 15.6% vs 21.8%; all languages 28.9% vs 24.3%), so compare within a window only.

## Actionable tips

| Tip | Evidence | Effect | Support | Confidence |
|---|---|---|---|---|
| Treat the newcomer's second message in the first session as the signal that matters. The data do not show which streamer behaviour produces it. | Chat, burn-in 30 d | 1 msg 4.2%, 2-4 10.7%, 5+ 27.4% ret30; MH +14.1 pp (2+), +17.9 pp (5+) | 1,945 newcomers, 126 streamers, 41 strata | Medium. Partly the same engagement, so a nudge may not deliver the full effect. |
| Do not expect a broadcaster reply to change return. | Chat, 3-minute reply | -3.2 pp (CI -12.0 to +8.3); Jul-Sep +2.8 (CI -4.8 to +12.6); category German -10.8 (n=51) | 141 replies in the burn-in set | Medium for "no large effect"; low for direction. |
| Keep the room active on consecutive days. Unconditional German return is limited by opportunity, not newcomer behaviour. | Category | German opportunity 46.1% vs 85.0% overall; ret7 among opportunity 33.8% vs 34.0% | 5,041 German newcomers, 360 rooms | Medium. Opportunity share moves with the window (65.1% in the lookback). |
| Use the 1-3 day window after a first visit, which is when returns cluster. | Presence and category | Median time to later presence about 3 days; German chat return median 2 days, next-day 38% | 3,033 lurkers, 626 chatters, 789 German returners | Medium on timing. The effect of an action in that window is untested. |
| Test a follow prompt before the first message only as an experiment. | Chat, follows | Followed before first chat +10.2 pp (CI +3.2 to +16.0); after +23.6 pp, likely reverse causality | 140 and 207 newcomers, 14 and 20 strata | Low to medium. Not a proven lever. |
| Do not optimise for questions as first messages. | Chat, labels | Question -3.4 pp (positive share 0.28); Statement +7.6, Greeting +6.7 | 218 question newcomers | Low. |
| Do not optimise for mentions or viewer level. | Chat | Mention +1.8 (CI -2.7 to +6.4); viewers at or above median +1.8 (positive share 0.33) | 294 mention cases, 1,042 above median | Medium that the effect is small. |

## Caveats and dead ends

- Coverage: logging gaps differ by streamer. Burn-in of 30 days is used throughout. Earlier pre-bot sensitivity runs (burn-in 0, 14, 30 d) gave the same ordering (1 message 6.3, 4.9, 4.5%; 5+ 24.5, 28.0, 28.2%), but were not rerun after bot exclusion.
- Small samples: 1,945 chat newcomers and 141 broadcaster replies. Only about 7% of newcomers receive a reply within 3 minutes.
- The first-session message effect is probably partly the newcomer's own engagement, not a causal lever.
- Follow effects are likely reverse causality (followed after first chat).
- The lurker-writer comparison is selected (1.8% of lurkers ever write), and presence among writers is 98.6% by construction.
- `is_first_time_streamer` has an unverified meaning. It is set for 95.6% of chatters vs 28.1% of lurkers, so it tracks writing and is not a clean predictor. Unflagged lurker rows (26,965 across 570 streamers) are concentrated: the top 10 streamers hold 45.7%, which points to per-streamer data gaps for the flag.
- Chat-speed predictor: the split is weak because counts are small integers and tie easily at the median (1,530 yes vs 415 no).
- Category: the first-observation window is sensitive (see section 7). The data start 2026-09-27, so earlier history is missing. The broadcaster-reply proxy counts any message from the broadcaster account.
- 100+ event streams: 1.1% ret30 among Jul-Sep newcomers (9 streamers). Excluded from main estimates.
- Figures in the brief that were not reproduced: 602 broadcaster-reply cases (nearest 247 and 974); within-channel baseline of 20.1 / 9.1 / 30.0% (my medians are 15.7 / 21.2% and 11.3 / 31.0% on other channel sets); "about 25% of arrivals after minute 15" (measured 90.8% after minute 15, 67.1% after minute 60, median 105 minutes into the stream); the 3-day / 33.0% German return gap (n=1,108), replaced by the verified 2 days / 38.0% (n=789).
- Dead ends: `confirmed_first_ever` is always false. The `chatter_login` fallback is unusable. The greeting bot table has 148 rows and 38 matches, so greeting-bot results rest on 33 newcomers. Question-answered cases are only 19. `nc_features.sql` timed out at 180 s (correlated subqueries) and was replaced by the set-based CTEs in `queries-P06.sql`. `presence_lurk.sql` is superseded.
