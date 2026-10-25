# Orchestrator verification checks

Own queries against the production DB (read-only), run 2026-10-25. Queries: `queries-VERIFY.sql`.

## V1. Late sampling in `twitch_session_viewers`

| Sessions started | Sessions | First sample after min 5 | After min 15 | After min 30 |
|---|---|---|---|---|
| 2026-01 to 2026-07 | 8,847 | 58.8 % | 25.3 % | 18.4 % |
| since 2026-08 | 5,691 | 55.5 % | 23.9 % | 22.0 % |

The earlier brief note "about 60 % after minute 15" was wrong; the correct figure is about 25 %. Confirms P11.

## V2. Does a broadcaster chat reply bring newcomers back?

First-time chatters (`messages >= 1`) from 2026-02-20 to 2026-09-24 (30-day follow-up), real first message time from `twitch_chat_messages`: 8,477 newcomers, 583 channels. "Back" = present again (chat or lurk) in another session of the same channel within 30 days.

| Newcomer messages in first session | Broadcaster wrote within 3 min: n / back | Did not: n / back |
|---|---|---|
| 1 | 145 / 18.6 % | 2,907 / 10.3 % |
| 2-4 | 124 / 27.4 % | 2,250 / 18.6 % |
| 5+ | 158 / 36.7 % | 2,893 / 33.9 % |
| all | 427 / 27.9 % | 8,050 / 21.1 % |

Within channel and message bucket (Mantel-Haenszel, bootstrap over channels): back -0.7 pp (CI -6.1 to +5.3), chats again -2.1 pp (CI -7.3 to +3.8), 98 channels. A broadcaster chat message naming the newcomer within 10 min (134 newcomers, 25 channels): back -2.3 pp (CI -10.9 to +7.1).

Reading: the raw advantage is a channel effect (channels where the streamer chats actively also have more returning viewers). Within the same channel a chat reply shows no measurable effect. Spoken greetings are invisible in this data, so this does not test greeting by voice.

## V3. One-message newcomers: spam and commands

3,052 newcomers with exactly one message (same window). First message type and 30-day return:

| Type | n | Back within 30 days |
|---|---|---|
| ordinary message | 2,124 | 14.1 % |
| command (`!...`) | 392 | 4.8 % |
| link or promo pattern | 390 | 1.8 % |
| moderated | 146 | 0.0 % |

About 30 % of one-message newcomers are commands, spam or moderated. Ordinary one-message newcomers return at 14 %.

## V4. Lurker baseline is inflated by logins seen in many channels

First-time rows by group and by how many channels the same login appeared in since 2026-02-20:

| Group | Login in fewer than 10 channels: n / back | 10-29 channels | 30+ channels |
|---|---|---|---|
| lurker | 9,272 / 15.5 % | 1,343 / 32.8 % | 1,044 / 52.1 % |
| 1 message | 2,868 / 9.1 % | 85 / 36.5 % | 100 / 33.0 % |
| 2-4 messages | 2,140 / 17.4 % | 113 / 39.8 % | 121 / 29.8 % |
| 5+ messages | 2,574 / 31.3 % | 245 / 46.5 % | 233 / 51.1 % |

For ordinary viewers (fewer than 10 channels): a silent newcomer returns at 15.5 %, one who writes 5+ messages at 31.3 %. A single genuine message is not better than silence; a real conversation is. Logins seen in 30+ channels include bots and scene-wide surfers and should be excluded from lurker baselines.
