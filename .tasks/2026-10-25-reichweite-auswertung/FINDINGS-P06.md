# P06: First-time chatter to regular

Saved by the orchestrator from the P06 agent's final report (the agent's Write call was rejected). Queries: `queries-P06.sql`.

## Question

What makes a first-time chatter come back to the same channel and become a regular?

## Short answer

- The strongest signal is the newcomer's own participation in the first session. Writing 2 or more messages there goes with a 16.9 pp higher 30-day return (95% CI +13.7 to +20.2). Writing 5 or more goes with +19.8 pp.
- A broadcaster reply within 3 minutes shows no detectable effect (-7.3 pp, CI -13.3 to +1.1). Replies are not a retention lever in this data.
- Following the channel before the first message goes with +12.2 pp (CI +4.7 to +20.9). Following afterwards (+19.0 pp) is most likely reverse causality.
- 598 of 2,543 chat newcomers (23.5%) had been present silently in an earlier session. They return within 30 days far more often than chatters whose first presence is the first chat session (45.8% vs 15.4%, within channel +28.9 pp, CI +23.7 to +33.6). They had already come back once before writing, so this is selected, not a lever.
- Lurkers (present, never wrote in their first session) return less often than chatters: 14.3% vs 26.4% by presence within 30 days, and 1.8% vs 15.8% by writing. Lurkers who later write return more, but that group is selected.
- In the category sample with live days checked (first day 2026-10-09 to 10-17), German newcomers return within 7 days as often as all newcomers (23.3% vs 23.4%). The German difference is fewer later live days: the room is live in Deadlock again within 7 days for 72.1% of German newcomers, against 83.0% overall (median 1 vs 4 live days), and return is highest with 5 to 7 live days. The wider window from 2026-09-28 shows a larger German gap (15.9% vs 29.6%), but it mixes in the growth of the chat collector (section 7).
- Returns are fast. The median time to a later presence is about 3 days for both lurkers and chatters, and the median time to a later chat day in the German category sample is 2 days.

## Data used

- `twitch_chat_messages`: 466,839 rows, 1,025 streamers, 5,740 sessions, 2026-01-31 to 2026-10-25.
- `twitch_session_chatters`: 165,095 rows, 1,312 streamers, 9,173 sessions. Lurkers are rows with `messages = 0` (`seen_via_chatters_api`), 130,457 rows in total. Presence analyses use sessions since 2026-02-19 (163,666 rows).
- `twitch_chat_message_labels` (first-message label), `twitch_greeted_chatters` (greeting bot).
- `category_chat_messages` since 2026-09-27, shared-chat copies excluded: 1,990,755 room-day rows, 942,648 room-chatter pairs, 29 days. `category_snapshots_normalized` from 2026-10-09 for live days per room.
- Bots: 77 logins seen in 30 or more channels, excluded from all populations. Names are not recorded here. Exclusions: 1,660 lurker rows and 10,608 category pairs.
- Size classes from each streamer's median average viewers (Jul to Oct): 0-4, 5-19, 20-99, 100+. The 100+ class (9 streamers among Jul-Sep newcomers, 15 overall) consists of one-off event streams and is excluded from the main estimates.
- Coverage burn-in: a newcomer counts only if their first chat is at least 30 days after the streamer's first logged session (1,310 streamers in coverage).

## Method

- Chat newcomer: first chat message of a (streamer, chatter) pair in the logged data, with no earlier chat. Earlier silent presence does not exclude anyone; `prior_silent` marks chat newcomers with a presence row in an earlier session (598 of 2,543 in the burn-in set). Presence newcomer: first session in `twitch_session_chatters` with no earlier presence. Presence-first chatters are split into lurkers (0 messages in the first session) and chatters (at least 1).
- Outcomes:
  - `ret30`: any later chat session within 30 days, excluding the first session.
  - `reg30`: at least 3 later chat sessions within 30 days.
  - Presence return: any later session in `twitch_session_chatters` within 30 days.
  - Silent return: a later session with 0 messages.
  - `ret7` (category): any later chat day in the same room within 7 days.
  - Opportunity (category): the room had any chat on a later day within 7 days.
- Within-channel estimator: Mantel-Haenszel risk difference, stratified by streamer. The strata count and positive-stratum share use strata with at least 3 newcomers per arm (`mh.py`). The CIs are a cluster bootstrap over streamer strata, 2,000 resamples, seed 20261025, using all strata with both arms (`boot.py`). Point estimates agree between the two scripts; the strata counts do not.
- Category window: first observation 2026-09-28 to 2026-10-17 (main). Lookback check: first observation 2026-10-04 to 2026-10-17. Live-days check: first day 2026-10-09 to 2026-10-17 with live days from the snapshots (`p06-live-days.py`).
- All queries are in `queries-P06.sql`. Raw extracts are in `/tmp/reichweite-scratch/P06/` only.

## Results

### 1. Return by size class (chat newcomers, Jul-Sep 2026, bots excluded)

| Class (median avg viewers) | n (30 d observed) | Streamers | ret30 % | reg30 % | ret7 % |
|---|---|---|---|---|---|
| 0-4 | 2,798 | 290 | 16.5 | 5.5 | 11.8 |
| 5-19 | 1,118 | 59 | 28.9 | 10.0 | 16.7 |
| 20-99 | 608 | 19 | 27.5 | 11.5 | 16.0 |
| 100+ (excluded) | 1,087 | 9 | 1.4 | 0.0 | 0.9 |
| Excl. 100+, no burn-in | 4,524 | 368 | 21.0 | 7.4 | 13.6 |
| Excl. 100+, burn-in 30 d | 2,543 | 132 | 22.6 | 8.3 | 14.7 |

ret7 uses the 7-day-observed set, which differs by at most 25 newcomers per row.

### 2. Return by messages in the first chat session (burn-in 30 d, excl. 100+, bots excluded)

| Messages in first session | n | Streamers | ret30 % | reg30 % | ret7 % |
|---|---|---|---|---|---|
| 1 | 667 | 109 | 8.5 | 1.9 | 5.3 |
| 2-4 | 823 | 95 | 17.3 | 5.6 | 10.7 |
| 5+ | 1,053 | 98 | 35.6 | 14.4 | 23.8 |

### 2a. Earlier silent presence (same set)

| Group | n | Streamers | ret30 % (1 / 2-4 / 5+ messages) | ret30 % all | reg30 % | ret7 % |
|---|---|---|---|---|---|---|
| Present silently before (`prior_silent`) | 598 | 91 | 28.3 / 39.6 / 57.0 | 45.8 | 18.6 | 29.5 |
| No earlier presence | 1,945 | 126 | 4.2 / 10.7 / 27.4 | 15.4 | 5.1 | 10.2 |

- Within channel, `prior_silent` goes with +28.9 pp ret30 (CI +23.7 to +33.6, 85 strata; 46 strata with 3+ per arm, positive share 0.83). The group had already returned once before its first message, so the gap is selection, not an effect of lurking.
- The message gradient holds in both groups: 2+ messages +16.2 pp (CI +6.0 to +24.3, 44 strata) among prior lurkers and +14.1 pp (CI +11.6 to +17.0, 79 strata) among the rest.

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
| Jul-Sep newcomers, excl. 100+, no burn-in | 4,524 | 328 (7.3%) | ret30 (22.6% vs 20.9%) | -1.1 | -7.5 to +6.8 |
| Burn-in 30 d, excl. 100+ | 2,543 | 187 (7.4%) | ret30 (21.4% vs 22.7%) | -7.3 | -13.3 to +1.1 |
| Same, within 5 minutes | 2,543 | 227 (8.9%) | ret30 (22.5% vs 22.6%) | -6.1 | -11.6 to +1.5 |

- Full history, any session, within 3 minutes of the first message: 974 of 18,421 newcomers (5.3%).
- The orchestrator's MH -0.7 pp (CI -6.1 to +5.3) is consistent with these estimates. Both intervals include zero. The brief's count of 602 cases is not reproduced; the nearest counts are 328 and 974.

### 6. Predictors (chat newcomers, burn-in 30 d, excl. 100+, bots excluded; outcome ret30; n=2,543)

| Predictor | Yes: n (ret30) | No: n (ret30) | MH-RD pp | 95% CI | Strata (3+ per arm) | Positive share |
|---|---|---|---|---|---|---|
| 2+ messages in first session | 1,876 (27.6%) | 667 (8.5%) | +16.9 | +13.7 to +20.2 | 51 | 0.92 |
| 5+ messages in first session | 1,053 (35.6%) | 1,490 (13.4%) | +19.8 | not bootstrapped | 59 | 0.83 |
| Followed before first chat | 199 (36.7%) | 2,344 (21.4%) | +12.2 | +4.7 to +20.9 | 17 | 0.76 |
| Followed within 30 d after first chat | 240 (39.2%) | 2,303 (20.8%) | +19.0 | not bootstrapped | 21 | 0.90 |
| First-message label Statement | 432 (28.7%) | 2,111 (21.3%) | +6.4 | not bootstrapped | 44 | 0.57 |
| First-message label Greeting | 522 (28.5%) | 2,021 (21.0%) | +7.0 | not bootstrapped | 43 | 0.58 |
| First-message label Question | 293 (21.5%) | 2,250 (22.7%) | -3.2 | not bootstrapped | 33 | 0.39 |
| At least 60 min of stream left at arrival | 1,848 (23.7%) | 695 (19.6%) | +3.4 | not bootstrapped | 53 | 0.60 |
| Arrived in first 30 min of stream | 432 (26.2%) | 2,111 (21.8%) | +4.8 | not bootstrapped | 42 | 0.55 |
| Chat speed 5 min before arrival at or above streamer median | 1,939 (20.3%) | 604 (30.0%) | -3.3 | not bootstrapped | 26 | 0.38 |
| Mention of login within 10 min | 377 (26.0%) | 2,166 (22.0%) | -0.1 | -5.1 to +4.5 | 28 | 0.46 |
| Another chatter wrote within 1 min | 1,020 (25.2%) | 1,523 (20.8%) | -1.3 | not bootstrapped | 47 | 0.43 |
| Viewers at arrival at or above streamer median | 1,352 (22.7%) | 1,189 (22.5%) | +1.5 | not bootstrapped | 64 | 0.42 |
| Broadcaster reply within 3 min | 187 (21.4%) | 2,356 (22.7%) | -7.3 | -13.3 to +1.1 | 12 | 0.17 |
| Bot greeting near arrival | 39 (17.9%) | 2,504 (22.6%) | -5.3 | not bootstrapped | 5 | 0.60 |
| `is_first_time_streamer` flag set | 1,987 (19.0%) | 556 (35.4%) | -16.6 | -21.8 to -10.8 | 30 | 0.17 |

Note: the `is_first_time_streamer` flag is set for 95.6% of chatters and 28.1% of lurkers (full history). It tracks writing rather than being an independent predictor (see caveats). In this set it is unset for 556 newcomers, 283 of them prior lurkers (set for 52.7% of prior lurkers vs 86.0% of the rest), so its negative estimate largely mirrors `prior_silent` (section 2a).

No detectable effect (CI includes zero, or positive share near 0.5): mention, arrival time, viewer level, other chatter writing. First-message labels Reaction (+3.0 pp, positive share 0.52) and Game-Related (+0.1 pp) are also near zero.

Robustness of the first-session message effect without burn-in (Jul-Sep newcomers, n=4,524): +13.0 pp (CI +10.3 to +15.6, 241 strata).

### 7. Category: German vs all rooms (bots and shared-chat copies excluded)

Main window, first observation 2026-09-28 to 2026-10-17:

| Scope | Newcomers | Rooms | ret7 % | With opportunity: n (share) | ret7 % among opportunity |
|---|---|---|---|---|---|
| All languages | 664,082 | 26,116 | 29.6 | 567,424 (85.4%) | 34.6 |
| German | 4,775 | 352 | 15.9 | 2,129 (44.6%) | 35.7 |
| English | 459,578 | 17,036 | 30.3 | 395,667 (86.1%) | 35.2 |

Among newcomers with opportunity:

| Predicate (first day) | All languages: yes / no ret7 % (MH pp, strata, positive share) | German: yes / no ret7 % (MH pp, strata, positive share) |
|---|---|---|
| 2+ messages | 41.6 / 24.2 (+17.2, 10,373, 0.65) | 40.9 / 21.1 (+15.1, 126, 0.61) |
| 5+ messages | 50.2 / 28.4 (+22.4, 11,044, 0.65) | 47.5 / 26.7 (+17.9, 138, 0.62) |
| Broadcaster message within 2 min | 34.5 / 34.6 (-1.6, 2,916, 0.39) | 24.4 / 35.9 (-14.7, 23, 0.26; n=45) |

- German return gap, newcomers who return within 7 days (n=761): median 2 days, p25 1, p75 4, next-day share 38.2%.
- Lookback (first observation 2026-10-04 to 2026-10-17): German n=2,357, ret7 22.9%, opportunity 64.7% (ret7 35.4%). 2+ messages +16.5 pp (103 strata), 5+ messages +20.7 pp (120 strata). All languages: n=424,634, ret7 24.9%, opportunity 83.3% (ret7 29.9%), 2+ messages +14.5, 5+ messages +19.3.
- The first-day message effect holds in both windows. Absolute ret7 levels move with the window (German 15.9% vs 22.9%; all languages 29.6% vs 24.9%), so compare within a window only.
- Collector coverage explains much of the German opportunity gap in the main window. Rooms with chat per day: about 1,900 on 09-27, 4,200 on 10-09, 5,600 on 10-24. German rooms per day: 18 to 54 in late September and early October, 45 to 76 from 10-09 to 10-17, 77 to 94 from 10-18. A room the collector did not yet follow shows no later chat day, so "no opportunity" partly means "not yet collected".

Live-days check (`p06-live-days.py`, live days from `category_snapshots_normalized`, a day counts as live with at least 10 snapshots; first day 2026-10-09 to 10-17, when snapshots exist for every day of the 7-day window):

| Scope | Newcomers | Rooms | ret7 % | Room live on a later day within 7 d (ret7 %) | Later chat day within 7 d (ret7 %) | Median later live days (mean) | ret7 % by later live days 0 / 1-2 / 3-4 / 5-7 |
|---|---|---|---|---|---|---|---|
| All languages | 276,854 | 18,011 | 23.4 | 83.0% (28.2) | 82.3% (28.5) | 4 (3.59) | 0.2 / 23.7 / 29.8 / 30.0 |
| German | 1,648 | 245 | 23.3 | 72.1% (32.3) | 69.3% (33.6) | 1 (2.13) | 0.0 / 28.6 / 27.7 / 45.4 |
| English | 187,341 | 11,623 | 23.8 | 83.6% (28.4) | | 4 | 0.2 / 24.8 / 29.2 / 30.0 |

- The room was live in the category on the newcomer's first day for 99.7% of German and 97.9% of all newcomers. Live later but no recorded chat later: 4.4% of German and 1.3% of all live-opportunity newcomers. German newcomers by later live days: 27.9% at 0, 37.0% at 1-2, 18.2% at 3-4, 17.0% at 5-7.
- With a 7-day room burn-in (room in the chat data at least 7 days before the first day): German n=510 in 72 rooms, ret7 20.2%, live later 82.4% (ret7 24.5%), median 3 live days (mean 2.96), ret7 14.5 / 22.4 / 37.5% at 1-2 / 3-4 / 5-7 live days. All languages n=162,350 in 6,996 rooms, ret7 22.8%, live later 93.7%, median 5 live days, 16.0 / 22.5 / 26.9%. English ret7 23.2%.
- Reading: once the window has full coverage, German newcomers return as often as all newcomers. The German difference is fewer later live days. Return is highest with 5 to 7 live days (German 45.4% against 28.6% and 27.7% at 1-2 and 3-4), but return is measured through category chat, which only exists while the room is live in Deadlock: "not live later, 0% return" is mechanical (459 German newcomers), and part of the rise is more chances to be observed. The earlier figures for this section (German 15.6% vs 28.9% ret7, opportunity 46.1% vs 85.0%) mixed collector growth into the gap and are withdrawn.

## Actionable tips

| Tip | Evidence | Effect | Support | Confidence |
|---|---|---|---|---|
| Treat the newcomer's second message in the first session as the signal that matters. The data do not show which streamer behaviour produces it. | Chat, burn-in 30 d | 1 msg 8.5%, 2-4 17.3%, 5+ 35.6% ret30; MH +16.9 pp (2+), +19.8 pp (5+) | 2,543 newcomers, 132 streamers, 51 strata | Medium. Partly the same engagement, so a nudge may not deliver the full effect. |
| Do not expect a broadcaster reply to change return. | Chat, 3-minute reply | -7.3 pp (CI -13.3 to +1.1); Jul-Sep -1.1 (CI -7.5 to +6.8); category German -14.7 (n=45) | 187 replies in the burn-in set | Medium for "no large effect"; low for direction. |
| Stream on more days, ideally consecutive. German newcomers return as often as others; their rooms are live on fewer later days. | Category, live days from snapshots | ret7 23.3% German vs 23.4% all; room live again within 7 days 72.1% vs 83.0%; German ret7 28.6% at 1-2 later live days, 27.7% at 3-4, 45.4% at 5-7 | 1,648 German newcomers, 245 rooms | Medium. Return is only observable while the room is live, so part of the rise is more chances to be seen. |
| Use the 1-3 day window after a first visit, which is when returns cluster. | Presence and category | Median time to later presence about 3 days; German chat return median 2 days, next-day 38% | 3,033 lurkers, 626 chatters, 761 German returners | Medium on timing. The effect of an action in that window is untested. |
| Test a follow prompt before the first message only as an experiment. | Chat, follows | Followed before first chat +12.2 pp (CI +4.7 to +20.9); after +19.0 pp, likely reverse causality | 199 and 240 newcomers, 17 and 21 strata | Low to medium. Not a proven lever. |
| Do not optimise for questions as first messages. | Chat, labels | Question -3.2 pp (positive share 0.39); Statement +6.4, Greeting +7.0 | 293 question newcomers | Low. |
| Do not optimise for mentions or viewer level. | Chat | Mention -0.1 (CI -5.1 to +4.5); viewers at or above median +1.5 (positive share 0.42) | 377 mention cases, 1,352 above median | Medium that the effect is small. |

## Caveats and dead ends

- Coverage: logging gaps differ by streamer. Burn-in of 30 days is used throughout. Sensitivity runs with burn-in 0, 14 and 30 d (bots excluded, excl. 100+) give the same ordering: 1 message 10.2, 10.1, 8.5% ret30; 5+ messages 30.8, 35.8, 35.6% (n 4,524, 2,966, 2,543).
- Small samples: 2,543 chat newcomers and 187 broadcaster replies. Only about 7% of newcomers receive a reply within 3 minutes.
- The first-session message effect is probably partly the newcomer's own engagement, not a causal lever.
- Follow effects are likely reverse causality (followed after first chat).
- Prior lurkers (`prior_silent`, 598 newcomers) return far more often, but they had already come back at least once before writing; the gap is selection. Until 2026-10-26 these newcomers were wrongly excluded from the chat cohort; all section 1, 2, 5 and 6 figures were recomputed with them.
- The lurker-writer comparison is selected (1.8% of lurkers ever write), and presence among writers is 98.6% by construction.
- `is_first_time_streamer` has an unverified meaning. It is set for 95.6% of chatters vs 28.1% of lurkers, so it tracks writing and is not a clean predictor. Unflagged lurker rows (26,965 across 570 streamers) are concentrated: the top 10 streamers hold 45.7%, which points to per-streamer data gaps for the flag.
- Chat-speed predictor: the split is weak because counts are small integers and tie easily at the median (1,939 yes vs 604 no).
- Category: the first-observation window is sensitive (see section 7). The data start 2026-09-27, so earlier history is missing, and the collector followed far fewer rooms in late September than in October. The broadcaster-reply proxy counts any message from the broadcaster account. Shared-chat copies are excluded from the per-room counts.
- 100+ event streams: 1.4% ret30 among Jul-Sep newcomers (9 streamers). Excluded from main estimates.
- Figures in the brief that were not reproduced: 602 broadcaster-reply cases (nearest 247 and 974); within-channel baseline of 20.1 / 9.1 / 30.0% (my medians are 15.7 / 21.2% and 11.3 / 31.0% on other channel sets); "about 25% of arrivals after minute 15" (measured in the burn-in set, n=2,564: 90.8% after minute 15, 69.7% after minute 60, median 111 minutes into the stream); the 3-day / 33.0% German return gap (n=1,108), replaced by 2 days / 38.2% (n=761, shared-chat copies excluded).
- Dead ends: `confirmed_first_ever` is always false. The `chatter_login` fallback is unusable. The greeting bot table has 148 rows and 38 matches, so greeting-bot results rest on 33 newcomers. Question-answered cases are only 19. `nc_features.sql` timed out at 180 s (correlated subqueries) and was replaced by the set-based CTEs in `queries-P06.sql`. `presence_lurk.sql` is superseded.
