# P07 Findings: Chat content and conversation dynamics

Saved by the orchestrator from the P07 agent's final report (the agent's Write call was rejected). Queries: `queries-P07.sql`.

## Question
Which message types, newcomer entry patterns, viewer questions, commands, emotes and message lengths go with more follow-up chat, longer newcomer stays and livelier sessions in German Deadlock streams?

## Data used
- `twitch_chat_messages`, 2026-02-01 to 2026-10-04: 339,444 messages, 690 streamers, 4,098 sessions with tracked chat, 11,008 logins. All messages in this window are labelled (Q0). Every label-based query (Q1 to Q3, Q7) reads messages only from this window; Q4 selects questions from this window and looks for replies in the minutes after them. The label-free command and activity queries (Q5a to Q5d) read messages to 2026-10-24; their 30-channel bot list comes from the labelled window.
- `category_chat_messages`, German subset, 2026-09-27 to 2026-10-24: 109,166 messages, 522 rooms, 6,043 chatter IDs (Q0). Not bot-filtered by login (see caveats).
- `twitch_stream_sessions`: `started_at` (Q9) and `avg_viewers` (size classes, Q5b).
- Bot exclusion ("x30"): role bot if the login matches `%bot`, `streamelements` or `streamlabs`, or is seen in 30+ channels. Six logins seen in 30+ channels hold 33,998 messages (10.02 percent). Only 2 of them match the name pattern (Q8).
- Not used: lurker data in `twitch_session_chatters`, the chat fields of `twitch_stream_sessions` (March to May 2026) and `follower_delta`. The addendum traps therefore do not affect these numbers.

## Method
- **Follow-up lift (Q1):** for each labelled viewer message, count follow-ups by other non-bot chatters in the same session within 2 minutes (f2) and 5 minutes (f5). Lift = type mean / session mean. Sessions need 30+ scored messages, type cells need 3+. Median lift per streamer and label, then share of streamers above 1. Exact two-sided binomial sign test against 50 percent, computed from 2-decimal shares, so p-values are approximate. Also: any-follow-up lift (share of messages with a follow-up within 2 minutes) and new-chatter lift (first-time-in-session chatters in the next 5 minutes).
- **Newcomer pre-context (Q2):** newcomer = a viewer's first message in the channel from 2026-03-01, after the first 10 minutes of the session. Speed = human messages in the 3 minutes before / 3. Baseline = the same session's minute grid (time-grid). Reported value = median per-streamer difference (pre-context minus baseline).
- **First-message stay (Q3, mature cohort, x30):** viewers whose first channel message falls between 2026-03-01 and 2026-09-05. "Stayed" = 3+ own messages in that session. "Returned" = wrote in the channel in a later session.
- **Questions (Q4):** viewer messages labelled Question, 2026-02-01 to 2026-10-04. "Answered" = broadcaster message or mention of the asker within 2 minutes. Asker follow-up = asker's messages in the 10 minutes after, answered versus unanswered within the same session.
- **Commands and interactivity (Q5a, Q5b):** messages starting with "!", grouped by first word. A channel is "interactive" if viewer commands appear in 20+ percent of its 3+ sessions. Size classes from median `avg_viewers`.
- **Activation (Q5c, Q5d):** first session with a viewer command; relative activity = session human messages / channel's median week; before versus after. Placebo: 36 channels without viewer commands, split at their own midpoint.
- **Emotes and length (Q6):** German category subset, per room: follow-up count within 2 minutes (all chatters) relative to room mean; messages with at least one emote (`emote_count > 0`) versus plain messages within the room (5+ each); length buckets; rooms with 20+ messages.
- **Liveliness quartiles (Q7):** session viewer messages relative to the streamer's own median session, split into quartiles (ntile 4). Label shares per quartile.

## Results

**R1. Follow-up lift by label (viewers, x30, streamer medians)**

| Label | Streamers | Sessions | Messages | Lift 2 min (median) | Share above 1 (2 min) | p (2 min) | Lift 5 min | Share above 1 (5 min) | p (5 min) | Any-follow-up lift 2 min | New-chatter lift 5 min |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Social | 233 | 978 | 12,663 | 1.10 | 0.60 | 0.003 | 1.05 | 0.57 | 0.036 | 1.09 | 0.74 |
| Feedback | 170 | 685 | 4,906 | 0.80 | 0.29 | 3e-8 | 0.84 | 0.34 | 4e-5 | 0.93 | 0.64 |
| Game-Related | 288 | 1,300 | 24,956 | 0.70 | 0.21 | 3e-24 | 0.79 | 0.26 | 2e-16 | 0.86 | 0.69 |
| Technical | 112 | 396 | 2,791 | 0.81 | 0.38 | 0.018 | 0.83 | 0.34 | 9e-4 | 0.99 | 0.63 |
| Other | 195 | 729 | 9,340 | 0.83 | 0.36 | 1e-4 | 0.87 | 0.37 | 3e-4 | 0.96 | 0.67 |
| Statement | 338 | 1,599 | 101,541 | 0.92 | 0.30 | 1e-13 | 0.94 | 0.35 | 3e-8 | 0.96 | 0.93 |
| Question | 318 | 1,468 | 27,378 | 0.88 | 0.37 | 5e-6 | 0.91 | 0.36 | 5e-7 | 0.94 | 0.84 |
| Reaction | 339 | 1,556 | 49,949 | 0.94 | 0.42 | 0.003 | 0.95 | 0.45 | 0.082 | 0.99 | 0.94 |
| Hype | 222 | 938 | 8,791 | 0.92 | 0.44 | 0.093 | 0.96 | 0.45 | 0.16 | 0.99 | 0.70 |
| Command | 171 | 665 | 10,479 | 0.95 | 0.46 | 0.36 | 0.92 | 0.43 | 0.092 | 1.04 | 0.78 |
| Greeting | 209 | 861 | 6,677 | 1.03 | 0.54 | 0.27 | 0.98 | 0.47 | 0.41 | 1.03 | 0.84 |

Both message sources are bounded to 2026-02-01 to 2026-10-04 before the follow-up windows are computed, so only fully labelled messages are scored and counted. Broadcaster and bot rows are excluded. The System label (14 streamers) is too small to read. The non-x30 run with the same bounds gives Social 1.10 (63 percent above 1) and Game-Related 0.74 (24 percent). An earlier run without the upper bound (messages to 2026-10-24, partly unlabelled) gave Social 1.12 and Game-Related 0.75; it is withdrawn.

**R2. Newcomer pre-context vs same-session minute grid (viewers, x30, paired)**

| Metric | Streamers | Sessions | Median difference | Share of streamers positive |
|---|---|---|---|---|
| Speed (messages per minute) | 396 | 1,740 | -0.070 | 0.28 |
| Question share | 272 | 923 | -0.042 | 0.25 |
| Statement share | 272 | 923 | -0.038 | 0.38 |
| Reaction share | 272 | 923 | -0.032 | 0.37 |
| Game-Related share | 272 | 923 | -0.030 | 0.20 |
| Greeting share | 272 | 923 | -0.010 | 0.16 |
| Social share | 272 | 923 | -0.010 | 0.18 |
| Hype share | 272 | 923 | -0.008 | 0.23 |
| Feedback share | 272 | 923 | -0.007 | 0.12 |
| Command share | 272 | 923 | 0.000 | 0.15 |

Unpaired absolute levels (Q2c, not a paired comparison): newcomer pre-contexts (n=1,802): speed 0.776, question share 0.103, statement share 0.334, game share 0.093. Time-grid minutes (n=2,918): speed 0.619, question 0.115, statement 0.340, game 0.110. The shares agree with the paired result (lower for newcomers). The speed level points the other way, so the speed effect is not robust. The message-sampled regular baseline (sensitivity only) gives the same sign for all types, but its speed value (-0.78) is inflated by sampling busy minutes and is not interpreted.

**R3. Newcomers' first message label, mature cohort (first message 2026-03-01 to 2026-09-05, x30)**

| First-message label | Newcomer first messages | Share of newcomers | Share of all viewer messages | Mean own messages in session | Stayed 3+ | Returned later | Median relative stay |
|---|---|---|---|---|---|---|---|
| Statement | 1,299 | 18.5% | 38.0% | 10.48 | 55.7% | 25.5% | 0.98 |
| Reaction | 1,222 | 17.4% | 18.7% | 8.49 | 48.9% | 22.9% | 1.00 |
| Greeting | 1,199 | 17.0% | 2.9% | 12.54 | 63.9% | 25.9% | 1.00 |
| Question | 881 | 12.5% | 10.3% | 9.81 | 61.0% | 22.5% | 0.95 |
| Other | 579 | 8.2% | 3.6% | 3.49 | 15.4% | 8.3% | 0.51 |
| Command | 559 | 7.9% | 4.1% | 3.14 | 21.8% | 12.5% | 0.69 |
| Game-Related | 442 | 6.3% | 9.9% | 11.68 | 66.3% | 23.1% | 1.01 |
| Hype | 393 | 5.6% | 3.6% | 8.15 | 55.5% | 20.6% | 0.95 |
| Social | 304 | 4.3% | 4.7% | 9.38 | 47.7% | 21.4% | 0.75 |
| Feedback | 88 | 1.3% | 2.2% | 14.27 | 61.4% | 27.3% | 0.86 |
| Technical | 60 | 0.9% | 1.4% | 9.93 | 56.7% | 30.0% | 1.05 |

Feedback and Technical are small cells. The all-cohort run (including newcomers from recent months) is not reported: its "returned" shares are biased low because recent newcomers have had little time to return. The non-x30 mature run gave Statement 56.7 percent stayed 3+ and 26.4 percent returned, so the bot exclusion moves these shares by about one point.

**R4. Viewer questions (Q4, not x30)**

| Measure | Value |
|---|---|
| Viewer questions / sessions / streamers | 34,037 / 2,968 / 545 |
| Broadcaster reply within 2 min | 2.8% |
| Any reply (broadcaster or mention) within 2 min | 12.4% |
| Any reply within 5 min | 18.7% |
| Median time to first reply (any / broadcaster) | 69.1 s / 111.0 s |
| Asker's follow-up in 10 min: answered minus unanswered, median | +0.069 messages (224 streamers, 827 sessions, 51% positive) |
| Change in asker follow-up rate | +0.116 (53% positive) |

Null: answering shows no measurable effect on the asker's follow-up.

**R5. Command words (x30, top by messages, 5+ channels)**

| Word | Messages | Channels | Viewer share | Broadcaster share | Bot share |
|---|---|---|---|---|---|
| lurk | 1,081 | 179 | 0.86 | 0.00 | 0.14 |
| clip | 451 | 42 | 0.36 | 0.45 | 0.19 |
| sr | 354 | 22 | 0.97 | 0.01 | 0.01 |
| gamble | 213 | 28 | 0.97 | 0.03 | 0.00 |
| so | 203 | 37 | 0.49 | 0.51 | 0.00 |
| dc | 195 | 72 | 0.59 | 0.31 | 0.10 |
| raid | 187 | 32 | 0.13 | 0.84 | 0.03 |
| rank | 175 | 35 | 0.55 | 0.17 | 0.27 |
| watchtime | 126 | 32 | 0.93 | 0.03 | 0.04 |
| tts | 119 | 7 | 0.93 | 0.00 | 0.07 |
| discord | 111 | 50 | 0.75 | 0.23 | 0.02 |
| followage | 106 | 29 | 0.95 | 0.05 | 0.00 |
| setgame | 88 | 32 | 0.40 | 0.60 | 0.00 |
| game | 72 | 31 | 0.44 | 0.56 | 0.00 |
| title | 42 | 11 | 0.33 | 0.67 | 0.00 |
| settitle | 23 | 6 | 0.44 | 0.57 | 0.00 |

Channel interactivity by size (x30; median of channel medians; channels with and without viewer commands):

| Avg viewers class | Group | Channels | Sessions | Median human messages | Median chatters |
|---|---|---|---|---|---|
| 0-4 | interactive | 15 | 631 | 112.0 | 6.0 |
| 0-4 | no viewer commands | 246 | 2,910 | 11.0 | 2.0 |
| 5-19 | interactive | 31 | 577 | 80.0 | 8.0 |
| 5-19 | no viewer commands | 42 | 292 | 38.3 | 5.0 |
| 20-99 | interactive | 9 | 130 | 58.0 | 8.5 |
| 20-99 | no viewer commands | 6 | 27 | 106.0 | 16.5 |
| 100+ | interactive | 4 | 49 | 76.8 | 21.0 |
| 100+ | no viewer commands | 1 | 3 | 80.0 | 25.0 |

No consistent direction: the interactive channels have more messages in the two smallest classes and fewer in the 20-99 class. Selection-confounded.

Activation (Q5c, original, not x30): 48 channels. Median relative activity before the first viewer command 0.583, after 1.175. Median per-channel difference +0.429; 73 percent of channels higher after. Placebo (Q5d, original, 36 channels without viewer commands): 0.375 before, 0.383 after, median difference -0.002, 50 percent higher after (saved output re-run this session, reproduced exactly).

**R6. Emotes and message length (German category data, 2026-09-27 to 2026-10-24, not bot-filtered)**

| Feature | Rooms | Messages | Median ratio | Share of rooms above 1 |
|---|---|---|---|---|
| Message with at least one emote vs plain, same room | 206 | 97,609 | 0.887 | 0.37 |
| 1-10 characters | 304 | 25,793 | 1.066 | 0.64 |
| 11-25 characters | 320 | 29,801 | 1.032 | 0.62 |
| 26-60 characters | 322 | 35,809 | 0.979 | 0.43 |
| 61+ characters | 256 | 16,183 | 0.866 | 0.26 |

**R7. Liveliness quartiles and label mix (x30, shares of viewer-labelled messages)**

Quartile bounds (session viewer messages relative to the streamer's own median session): Q1 0.01 to 0.53, Q2 0.53 to 1.00, Q3 1.00 to 1.63, Q4 1.64 to 18.32. Each quartile has 528 sessions and 114 to 137 streamers. Viewer-labelled messages: 17,127 / 31,149 / 68,418 / 128,843.

| Label | Q1 | Q2 | Q3 | Q4 |
|---|---|---|---|---|
| Statement | 36.6% | 37.5% | 40.6% | 40.1% |
| Reaction | 19.7% | 19.8% | 19.5% | 18.9% |
| Question | 11.0% | 11.5% | 10.9% | 10.6% |
| Game-Related | 9.2% | 10.3% | 8.9% | 10.9% |
| Social | 4.7% | 4.5% | 4.8% | 4.8% |
| Greeting | 4.4% | 3.8% | 3.1% | 2.5% |
| Hype | 3.9% | 4.0% | 3.6% | 3.8% |
| Other | 3.7% | 3.0% | 2.8% | 3.2% |
| Feedback | 2.3% | 2.4% | 2.2% | 2.3% |
| Technical | 2.3% | 1.5% | 1.5% | 1.2% |
| Command | 1.6% | 1.4% | 1.3% | 1.5% |

Greetings fall steadily. Statement share is higher in the two livelier quartiles. Game-Related is not monotonic. This is a descriptive mix; word groups for topic analysis are empty, so labels are the topic proxy.

**R8. Data note: late start of tracked chat (Q9, not a result)**
- Of 12,378 sessions that started in the window, 4,096 (33.1 percent) have any tracked chat. The session-relative analyses cover only this subset.
- Among these, the first tracked message comes after minute 5 in 83.5 percent and after minute 15 in 62.9 percent. Share after minute 15 by size class (messages per session): under 10: 72.2 percent; 10 to 49: 67.5 percent; 50 to 199: 56.0 percent; 200+: 39.9 percent (median first message at 11.4 minutes).
- No tracked message precedes `started_at` (0.00 percent). The cause (logger join delay or `started_at` semantics) is not verified.

## Actionable tips

Orchestrator note: V11 in `VERIFY-ORCHESTRATOR.md` holds chat speed equal (same session, same number of messages by others in the 2 minutes before). The Social lift disappears (-0.8 %, CI -5.4 to +10.4): social messages are posted when chat is already fast. Game-Related stays below other messages (-19.2 %, CI -23.6 to -4.1). Tips 1 and 2 are therefore timing, not levers. The emote and length ratios use the same unadjusted design.

| Tip | Evidence | Effect | Support n | Confidence |
|---|---|---|---|---|
| Social-style viewer messages draw more follow-up than average. Expect quick chat responses after them. | Social lift 1.10 (2 min), 60% of streamers above 1 | Lift 1.10, p about 0.003 | 233 streamers, 978 sessions, 12,663 messages | Medium (direction robust to bot exclusion; association) |
| If the aim is quick chat response, prefer social prompts over game commentary. Game-related messages have the weakest follow-up. Whether swapping them changes long-term activity is not measured. | Game-Related lift 0.70, 21% above 1 | Lift 0.70, p about 3e-24 | 288 streamers, 1,300 sessions, 24,956 messages | Medium-high (direction); low (causal effect) |
| Greet newcomers and react to their first messages; a command as first message is the weakest entry. | First-message stay 3+: Greeting 63.9%, Question 61.0%, Game-Related 66.3% vs Command 21.8%, Other 15.4% | Association, not causal | 1,199 / 881 / 442 / 559 / 579 first messages | Medium |
| Do not expect answers to raise a viewer's follow-up. Reply rates are low, so reply speed is a capacity problem, not a measured lever. | 2.8% broadcaster reply within 2 min; asker follow-up answered vs unanswered +0.07 messages (51% positive) | Null | 34,037 questions, 224 streamers, 827 sessions | High (reply rates); medium (null) |
| Keep messages short if you want follow-up. Long messages (61+ characters) draw less. | Lift 1.07 (1-10 characters, 64% of rooms above 1) vs 0.87 (61+, 26%) | Ratio 1.07 vs 0.87 | 304 / 256 rooms; 25,793 / 16,183 messages | Low-medium (category data, not bot-filtered) |
| Messages with emotes are not conversation starters: plain text in the same room draws more follow-up. | Message with at least one emote (`emote_count > 0`) vs plain, median ratio 0.887, 37% of rooms above 1 | Ratio 0.89 | 206 rooms, 97,609 messages | Low-medium (same caveat) |
| Viewer commands in use are mostly lurk, sr, gamble, watchtime and followage. Raid, setgame, title and game are broadcaster-controlled. | Message and channel counts; broadcaster share 0.56 to 0.84 for the controlled words | Usage only; no effect measured | lurk 1,081 messages / 179 channels; sr 354 / 22; gamble 213 / 28 | High (counts); effect not measured |
| Do not promise activity gains from viewer commands. Activation is a hypothesis to test, not a result. | Channels with commands: 0.58 to 1.18 (73% higher after). Placebo: 0.375 to 0.383 | Possible, unvalidated | 48 channels (command), 36 channels (placebo) | Low (selection, no x30, no causal claim) |

## Caveats and dead ends
- **Bots and x30:** viewer tables use the 30-channel exclusion (6 logins, 10.02 percent of messages, 2 of 6 match the name pattern). Q1 lifts move by up to 0.04 and shares above 1 by up to 3 points against the non-x30 run; Q3 shares by about one point.
- **Late chat start (R8):** only 4,096 of 12,378 sessions have tracked chat. Tracked chat starts after minute 15 in 62.9 percent of these sessions; in the 200+ message class, 39.9 percent. Cause not verified. The coordinator's ~25 percent figure refers to the first viewer sample (V1), not chat, so it is not reconciled here.
- **Label coverage:** all messages to 2026-10-04 are labelled; later messages are partially labelled. The label source mix was not re-verified, so no source-mix figure is reported.
- **Q2 speed:** paired (-0.07) and unpaired absolute levels (0.78 vs 0.62) disagree in direction. Low confidence; only the share differences are consistent.
- **Regular-message baseline:** oversamples busy minutes; used only as sensitivity.
- **Q3 all-cohort returned shares:** biased low for recent newcomers; not reported.
- **Q4:** not re-run with the x30 bot exclusion (about 2 minutes, moderate risk). Asker and reply counts are not bot-filtered.
- **Q5c and Q5d x30:** not run. A Postgres crash ("terminating connection because of crash of another server process", then recovery mode) hit during the Q5c x30 run; retries stalled. Cause unknown; not attributed to my query. Original Q5c and Q5d results are provisional and not re-validated for bots.
- **Q5b:** selection-confounded, no consistent direction; the 100+ class has one channel without viewer commands.
- **Q6:** category data, not bot-filtered by login; follow-up counts are raw messages; medians of ratios, no significance test.
- **Q7:** the quartile is relative to each streamer's median session, so it is correlational.
- **Dead ends:** word-group topic analysis (empty; labels used instead). Distinct-chatter LATERAL query timed out at 180 s and was dropped. Sign-test p-values use 2-decimal shares, so they are approximate.
- **Confidence:** high = consistent across x30, p below 0.001 or direct counts; medium = significant but associational; low = confounded, sensitivity-limited or single-run.
