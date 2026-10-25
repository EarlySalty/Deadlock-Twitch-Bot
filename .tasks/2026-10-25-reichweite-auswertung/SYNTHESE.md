# Synthesis: reach, discovery and community growth for German Deadlock streams on Twitch

State 2026-10-25. Built by the orchestrator from twelve analysis packages (P01 to P12, Haiku agents) and eleven own verification checks (V1 to V11, `VERIFY-ORCHESTRATOR.md`). Where a package claim and a verification disagree, the verification wins and the package file carries an orchestrator note.

Data: production Twitch analytics DB, read-only. Sessions and viewer curves since January 2026, category polls since October 2025, chat messages and chatter lists since February 2026, follows and subscriptions since February 2026 (no follow rows for April and May 2026), all-language category snapshots only for 2026-10-09 to 2026-10-25. Logins seen in 30 or more channels (bots and scene-wide surfers) are excluded from chat and lurker figures. This file holds aggregates only: no logins, no channel names, no chat quotes.

Method in one line: every effect is a comparison inside the same streamer (or the same session), with bootstrap intervals over streamers; raw pooled differences are not reported as effects because channel size and growth phase dominate them.

## Bottom line

1. **Twitch does not push a German Deadlock stream to new people in its first minutes.** The opening burst is regulars answering the go-live notification: 90 % of arrivals in the first 10 minutes have been in the channel before. New people arrive at a flat 1.5 to 2.3 per 100 stream minutes over the first two hours (V6). Discovery accrues with time live.
2. **Time live is the main controllable reach lever.** Sessions of 4 hours or more run 14 % above the same streamer's average viewers (P03); in 5 to 19 viewer channels hour 5 holds 1.17 to 1.24 times the audience of hour 2 (P04). Part of this is self-selection: streamers stay on when it goes well.
3. **Streaming on consecutive days is what turns first visits into returns.** German newcomers return within 7 days at 15.6 % against 28.9 % across all languages. Almost the whole gap is opportunity: only 46 % of German newcomers find another live day with chat in that room within 7 days, against 85 % overall. Given an opportunity, return is the same (33.8 % vs 34.0 %). Returns cluster 2 to 3 days after the first visit (P06).
4. **Raids are the only external exposure with a measured gain, and it is small and short.** A 1 to 4 viewer channel keeps 0.185 extra viewers per raider after an hour, plus about one silent new viewer and 0.1 new chatters per raid; raid viewers return at the organic rate and the next stream starts no higher (P09).
5. **Packaging does not move reach.** Tags, title words, start hour, weekday, number of German streams live, the live notification, shared chat and mid-stream title changes all come out null inside the same streamer (P01, P02, P03, P09, P11, V5, V9). Only titles under 30 characters show a small cost of about 2 %.
6. **Loyalty comes from conversation, and typed replies alone do not show it.** Newcomers who write 2 or more messages in their first session return 14 pp more often (P06); a chatter follows about 9 times as often per session as a silent viewer (P08). Within the same channel, a typed broadcaster reply changes neither return (V2) nor follows (V7), and neither a broadcaster chat message nor an already moving chat brings a new chatter in the next 5 minutes (V10). No message type makes chat answer more once chat speed is held equal (V11). Spoken interaction is invisible to this data, which is the likely real mechanism.
7. **Ending near the peak lifts the next start a little.** About +6 to 9 % of the usual audience at the next start, robust to level, duration and peak controls, placebo null (V8).
8. **Growth is rare and does not follow a visible habit.** 4.5 % of tracked channels grew at least 1.5x over 8 to 12 weeks (P04). Across all languages, 36 of 8,203 small channels grew strongly within 16 days, none German, and growers looked like non-growers on hours, frequency, length, tags and affiliate status (P10).

## When does Twitch show a channel more often?

What the data can see is the German directory rank, unexplained viewer jumps and arrival times.

| Question | Result | Source |
|---|---|---|
| When do unexplained viewer jumps happen? | Mostly in the first 20 to 100 minutes (median uptime at a jump 22 min vs 90 min base for 0 to 2 viewers, 2,683 events) | P12 |
| Are those jumps new discovery? | No. 90 % of first-10-minute arrivals are returning viewers; new arrivals stay flat over time | V6 |
| Does a high directory rank help? | Top 3 of the German list goes with more jumps (0.46 vs 0.25), but no rank boundary matters inside the same streamer (top 6 vs 7+: median 0.0, 627 streamers). The raw gap is channel size | P12 |
| Snowball or momentum? | None. Rising vs flat stretches: median 0.0 (658 streamers) | P12 |
| Does competition from other German streams cost viewers? | No. 6 to 10 vs 1 to 5 German streams live: +0.027 new presences per 15 min (CI -0.003 to +0.062, 329 streamers) | V5 (corrects P12) |
| Does the go-live notification lift the start? | No measurable start lift (+0.007, 37 streamers) | P11 |
| How does a stream ramp up? | 0.19 of the streamer's median viewers at start, 0.65 at minute 15, 0.83 at minute 30. Judge a stream from minute 15 on | P11 |
| Which slots have the most viewers per German stream? | Afternoons 12 to 16 h (21.9 to 24.6 per stream); the largest German audience at 19 h (descriptive, dominated by big channels). For a given streamer, an evening start is not better (index 0.997 for 0 to 4 viewers) | P03 |
| Patch days, holidays, weekends? | No reliable effect. Saturdays bring about 16 % fewer first-time chatters per hour than the streamer's own average, with no viewer penalty | P03 |

The practical reading: Twitch exposure for small German Deadlock channels behaves like a steady trickle per minute live, plus raids. There is no early window to hit and no slot to hide from competition.

## Prioritized tips

Ranked by strength of evidence and cost. Confidence reflects the verification, not the package's own rating.

| # | Tip | Evidence | Size | Confidence | Cost |
|---|---|---|---|---|---|
| 1 | Stream on more days per week, ideally consecutive ones, so a newcomer has a next stream within 1 to 3 days | German opportunity 46 % vs 85 %; return given opportunity equal; returns cluster 2 to 3 days, next-day 38 % of German chat returns (P06) | At the all-language opportunity rate, German 7-day return would sit near the all-language 29 % instead of 16 % | medium | time |
| 2 | Plan longer sessions (4 h or more) instead of several short ones | 1.14x average viewers (P03); hour 5 1.17 to 1.24x hour 2 for 5 to 19 viewers (P04); more sessions with a follower gain (+0.14 gain share, P08); new people arrive per minute live (V6) | 6 to 17 % above own average | medium (self-selection) | time |
| 3 | Get raided: build raid ties with channels of similar or larger size (outgoing raids alone show no own growth). For 1 to 4 viewer channels, raids of at least twice the own audience keep the most | +0.185 viewers per raider at +60 min for 1 to 4 viewer targets; ratio 2+ keeps 0.143; follows in the 10 minutes after a raid 6.7 vs 2.2 per 100 viewer-hours (P08, P09) | about 1 extra viewer per 5 raiders for an hour | medium | none |
| 4 | Use the minutes after an incoming raid to welcome and talk; the gain fades within the hour and raid viewers do not return more than others | return 22.2 % vs 20.9 % organic; next-stream level unchanged (P09) | short-lived | medium | none |
| 5 | End while the room is still full instead of streaming into an emptying room; do not cut streams short for it | V8: +0.067 of the median at the next start (CI +0.040 to +0.101, 80 streamers), placebo null | 6 to 9 % of the usual audience | medium | none |
| 6 | Draw newcomers into a real back-and-forth: a second and third message is the signal that matters, a single line is no better than silence | 2+ messages +14.1 pp 30-day return (P06); 5+ messages 31.3 % vs 15.5 % for silent newcomers, 1 message 9.1 % (V4) | large association | medium for the association, low for any specific action (partly the same engagement) | none |
| 7 | Expect follows late and at high points, not at the start; a follow ask fits the busy moments | Median follow at minute 130, 72 % after minute 60; 9.5 follows per 100 viewer-hours in the 5 minutes before the session peak vs 2.6 overall (P08) | descriptive | low to medium (peak is defined after the fact) | none |
| 8 | Treat gifted subs as a welcome moment, not as community growth | Gifted recipients: 14.5 % chat within 30 days, 0 resubs (gifts do not renew). New paid subs: 75.8 % chat, 37.1 % resub later; 60 % of resub streaks at 3 months or more (P08) | descriptive | low (small cohorts, 8 and 32 streamers) | none |
| 9 | Keep titles at 30 characters or more; otherwise pick titles freely | Under 30 characters -1.7 % (CI -4.4 to -0.1, 137 streamers) against sessions within 14 days (V9) | about 2 % | medium | none |
| 10 | Switching between Just Chatting and Deadlock carries no penalty | Just Chatting to Deadlock +0.107 vs placebo (25 streamers); mixed sessions vs pure +0.044, null (P11) | none to small | low | none |

## Lurker to chatter to regular: the funnel in numbers

| Step | Figure | Source |
|---|---|---|
| Silent share of viewers (session level) | 0.70 at 0 to 4 average viewers, 0.78 at 5 to 19, 0.86 at 20 to 99, 0.87 at 100+ | P05 |
| Chatters per average viewer | 0.83, 0.70, 0.36, 0.13 across the same classes; never benchmark big rooms against small ones | P05 |
| First messages that are not real conversation | about 30 % of one-message newcomers are commands, links or moderated; they almost never return (0 to 4.8 %) | V3 |
| 30-day return after the first visit (logins in fewer than 10 channels) | silent 15.5 %, 1 message 9.1 %, 2 to 4 messages 17.4 %, 5+ messages 31.3 % | V4 |
| Follows per session presence | silent 7.95 per 1,000, chatter 73.8 per 1,000 | P08 |
| Order of chat and follow | 35.5 % of followers never chatted; 33.8 % chatted before following, median 5 minutes from first message to follow | P08 |
| Follow before the first message | +10.2 pp return (CI +3.2 to +16.0); following afterwards is reverse causation | P06 |
| Follow conversion | 2.63 follows per 100 tracked viewer-hours | P08 |
| Followers per average viewer, 0 to 4 viewer channels | German 50.2, English 58.3, Russian 32.0 | P10 |

German channels convert viewers to followers at a rate close to English ones. The German gap is reach and repeat opportunity, not conversion.

## Chat content (P07)

What viewers write, and what follows. All associations; nothing here is a tested lever.

| Observation | Figure | Confidence |
|---|---|---|
| Typed broadcaster replies to viewer questions are rare | 2.8 % within 2 minutes; any reply (broadcaster or a mention) 12.4 %, 18.7 % within 5 minutes; 34,037 questions | high for the rates |
| A newcomer's first message predicts the stay | greeting 63.9 %, question 61.0 %, game talk 66.3 % write 3 or more messages in that session; a command 21.8 %, other 15.4 % | medium (association) |
| Short messages and plain text get more follow-up | 1 to 10 characters 1.07, 61+ characters 0.87; emote messages 0.89 of plain ones in the same room (German category chat, not bot-filtered, not adjusted for chat speed) | low |
| Commands viewers actually use | lurk, sr, gamble, watchtime, followage; raid, setgame and title are run by broadcasters | high for the counts |
| Starting viewer commands | channels with a first viewer command: relative activity 0.58 before, 1.18 after (48 channels); placebo flat (0.375 to 0.383, 36 channels) | low (selection, not bot-checked) |
| Greetings thin out in lively sessions | 4.4 % of viewer messages in the quietest quartile, 2.5 % in the liveliest | descriptive |

## What does not work (null results)

| Idea | Result | Source |
|---|---|---|
| Tags, tag count, the German tag | no measurable viewer, chat or follower effect | P02 |
| Title words (`!discord`, "discord", brackets, "chill", "neu") | effects shrink to near zero against the same streamer's sessions within 14 days | V9 (corrects P01) |
| Changing the title during a stream | +0.1 % (568 events, 196 streamers) | P01 |
| Start hour, weekday | null inside the same streamer | P03, P08, P11 |
| Avoiding busy slots | no cost of more German streams live | V5 |
| Patch days, holidays | null | P03 |
| Go-live notification | no start lift | P11 |
| Shared chat | viewer ratio 0.947, chatter effect not robust | P09 |
| Outgoing raids | no own growth; no effect on the next start | P09, P11 |
| Broadcaster typed reply to a newcomer | return -0.7 pp (CI -6.1 to +5.3); follows -0.9 pp after removing alert thank-yous | V2, V7 (corrects P08) |
| Broadcaster typing in chat, also to fill silence | +1.4 pp new chatters in the next slot (CI -2.3 to +5.1); in silent slots -0.4 pp | V10 (corrects P05) |
| A moving chat pulling in new chatters | +0.0 pp in the next slot inside the same session (CI -0.7 to +0.6) | V10 (corrects P05) |
| A target chat speed | chance of a new chatter flat at 0.30 to 0.40 per active slot up to 6.4 messages per minute | P05 |
| Message types that make chat answer (social talk vs game talk) | social messages draw no extra follow-up at equal chat speed (-0.8 %); game talk draws less (-19 %) | V11 (corrects P07) |
| Typed answers to viewer questions | asker writes +0.07 messages more in the next 10 minutes, 51 % of streamers positive | P07 |
| Schedule regularity, pauses of 7 to 30 days | no measurable effect on viewer level | P04 |
| Strong previous session, snowball, rank boundaries | null | P11, P12 |
| Being raided as a lasting boost | next-session level -0.047 median | P09 |

## Caveats

- Everything is observational. Comparisons inside the same streamer remove channel size and style, but not choices that follow success (staying live longer when it goes well).
- Spoken interaction, stream content, camera and audio are not in the data. Null results for typed broadcaster chat say nothing about talking to chat by voice.
- Viewer curves start late for about 25 % of sessions (first sample after minute 15, V1); start and ramp analyses use only sessions sampled early.
- Chat fields in the session table are zero from March to May 2026; follow events are missing for April and May 2026; first-message times for silent viewers are poll times, not arrival.
- All-language category data covers only 16 days (2026-10-09 to 2026-10-25); growth comparisons across languages are short-window.
- Tracked chat often starts late: in sessions with chat, the first tracked message comes after minute 15 in 62.9 % (39.9 % even with 200+ messages, P07). Slot analyses drop the first 10 minutes after tracking starts (V10). In return and follow analyses this can only move real chatters into the silent group, which weakens rather than creates the gradients.
- The June 2026 language incident (2026-06-10 to 06-13) is excluded from category figures.

## Package and check index

| File | Topic |
|---|---|
| `FINDINGS-P01.md` | titles |
| `FINDINGS-P02.md` | tags |
| `FINDINGS-P03.md` | timing, slots, competition |
| `FINDINGS-P04.md` | length, frequency, growth |
| `FINDINGS-P05.md` | lurkers vs chatters, chat activation |
| `FINDINGS-P06.md` | first-time chatter loyalty |
| `FINDINGS-P07.md` | chat content |
| `FINDINGS-P08.md` | follows and subs |
| `FINDINGS-P09.md` | raids, shared chat |
| `FINDINGS-P10.md` | all-language category benchmark |
| `FINDINGS-P11.md` | stream opening, game switches, stream end |
| `FINDINGS-P12.md` | exposure triggers, directory rank |
| `VERIFY-ORCHESTRATOR.md` | verification checks V1 to V11 |
| `queries-*.sql`, `verify-*.py` | reproduction |
