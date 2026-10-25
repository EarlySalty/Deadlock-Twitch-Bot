# Twitch reach analysis: common rules for every analysis package

Goal of the whole study: find out, from all data we have, what makes German Deadlock Twitch streams
reach new viewers, what turns lurkers into chatters, and what turns first-time visitors into
followers, subscribers and loyal returning viewers. Concrete, evidence-backed tips for streamers.
Use the FULL history of every table, not just recent weeks. Dig deep.

## Database access (read-only)

- Run SQL only through `/tmp/tq.sh`. It is read-only and has a 180 s statement timeout.
  - `/tmp/tq.sh -Atc "select ..."` (unaligned, tuples only, `|` separator)
  - `/tmp/tq.sh --csv -c "select ..." > /tmp/reichweite-scratch/<PKG>/file.csv`
  - multi-line: `/tmp/tq.sh -At <<'SQL' ... SQL`
- NEVER print, search for, or echo secrets, DSNs, env vars or the Infisical scripts. Do not run
  `export_gpt_secret.py` yourself. Do not dump `env`.
- This is the live production DB of running bots. Keep queries aggregated, always filter on the time
  column, run one heavy query at a time. If a query times out, split it by month and loop.
- Temp tables are not possible (read-only). Aggregate in SQL (CTEs, `percentile_cont`, `FILTER`,
  window functions); pull small CSVs to `/tmp/reichweite-scratch/<PKG>/` and post-process with
  `python3` stdlib only (no pandas/scipy installed). Bootstrap or simple sign tests in stdlib are fine.
- Timescale hypertables: `twitch_stats_category`, `category_chat_messages` (daily partitions, about
  800k rows per day since 2026-09-27), `category_chat_metrics` (13M rows since 2026-10-09),
  `twitch_session_viewers` (7.5M rows). Always use a time-range predicate on them.

## Key tables (verify columns with information_schema before use)

- `twitch_stats_category(ts_utc, streamer, viewer_count, is_partner, game_name, stream_title, tags,
  language, twitch_user_id)`: snapshots of German Deadlock streams since 2025-10-10.
- `twitch_stream_sessions`: one row per stream session since 2026-01-09 (now about 675 streamers per
  month): duration, start/peak/end/avg viewers, retention_5m/10m/20m, dropoff, unique_chatters,
  first_time_chatters, returning_chatters, followers_start/end, follower_delta, stream_title,
  notification_text, tags, game_name, had_deadlock_in_session, language.
- `twitch_session_viewers(session_id, ts_utc, minutes_from_start, viewer_count)`: viewer curve per session.
- `twitch_session_chatters(session_id, streamer_login, chatter_login, chatter_id, first_message_at,
  messages, is_first_time_streamer, confirmed_first_ever, seen_via_chatters_api, last_seen_at)`.
- `twitch_chat_messages(session_id, streamer_login, chatter_login, chatter_id, message_ts, is_command,
  content, moderation_action)`: chat of tracked channels since 2026-01-31 (466k rows).
- `twitch_chat_message_labels(message_id -> twitch_chat_messages.id, label)`: Statement, Reaction,
  Question, Game-Related, Social, Command, Hype, Other, Greeting, Feedback, Technical, System.
- `twitch_chatter_rollup`, `twitch_chat_word_groups`, `twitch_greeted_chatters`.
- `twitch_follow_events(streamer_login, twitch_user_id, follower_login, follower_id, followed_at)`.
- `twitch_subscription_events(session_id, twitch_user_id, event_type, user_login, tier, is_gift,
  gifter_login, cumulative_months, streak_months, total_gifted, received_at, viewer_user_id)`.
- `twitch_community_points_viewer_daily(twitch_user_id, channel_twitch_user_id, day, watch_minutes,
  chat_messages, ...)`: per viewer and channel per day, includes lurkers (watch time without chat).
- `twitch_raid_history`, `twitch_raid_retention`, `twitch_raid_arrival_tracking`,
  `partner_effort_shared_chat_observations` (shared chat / stream together).
- `category_snapshots_normalized(snapshot_at, stream_id, user_id, user_login, viewer_count, title,
  language, started_at, tags[], is_mature, sample_seconds)`: ALL languages of the Deadlock category
  since 2026-09-27 (about 23k channels per week). Use `sample_seconds` as the time weight.
- `category_channels(user_id, created_at, broadcaster_type, followers_total, broadcaster_language, ...)`.
- `category_chat_messages(sent_at, room_user_id, chatter_user_id, message_text, message_len,
  detected_lang, stream_language, emote_count, shared_chat_copy)` since 2026-09-27, whole category.
  The broadcaster's own messages are rows with `chatter_user_id = room_user_id`.
- `category_chat_metrics`, `category_chat_rollup(hour_at, room_user_id, language, messages, distinct_chatter)`.
- `exp_sessions`, `exp_game_transitions` (game switches, timestamps stored as text).
- `title_generator_knowledge` (scored titles, relative_perf, engagement_rate).
- Network membership: `twitch_streamers`, `twitch_partners`, `twitch_raid_auth`.

## Known data traps

- `twitch_stats_category` snapshot interval changed: about 300 s until 2026-01-17, 75 s until
  2026-03-11, 15 to 16 s afterwards. Never treat row counts as time or reach. Weight by time gap or
  aggregate per streamer and day or per session first.
- June 2026 incident 2026-06-10 to 2026-06-13: collector ran without language filter. Drop rows with
  `language IS NOT NULL AND language NOT LIKE 'de%'`, and drop NULL-language rows on those days.
  `language` is NULL before 2026-06-11 but those rows are German.
- Streamer size dominates every metric. NEVER compare raw averages across streamers. Compare WITHIN
  a streamer: normalize each session or day by that streamer's own median (or a rolling 28-day
  baseline), compare sessions with the feature vs without for the same streamer, then report the
  median of per-streamer differences, number of streamers, number of sessions, and share of streamers
  where the effect is positive. Require support (for example at least 10 to 15 streamers using a
  feature) before calling something a finding.
- Small numbers: many streams have 0 to 3 viewers. Report results separately per size class
  (for example baseline median viewers 0-4, 5-19, 20-99, 100+) where it matters.
- Correlation is not causation. Name selection effects and confounders explicitly (time of day,
  patch days, streamer motivation, big streamers' habits).
- `twitch_raid_history.id` is not unique; use `(id, executed_at)`.

## Privacy (the output folder is committed to a PUBLIC GitHub repo)

- No chatter logins or IDs, no streamer logins or IDs, no raw chat message quotes in any file in the
  task folder. Anonymize streamers as size class plus an index if you need examples.
- Title words, title patterns and tags may be named when at least 5 different streamers used them.
- Raw extracts and intermediate CSVs go to `/tmp/reichweite-scratch/<PKG>/` only.
- Do not send any data to external services. No web access needed.

## Output

Write in English, into `/home/nathanael/.worktrees/twitch-reichweite/.tasks/2026-10-25-reichweite-auswertung/`:

1. `FINDINGS-<PKG>.md` with: Question, Data used (tables, time range, n rows, n streamers, n sessions),
   Method, Results (tables with real numbers), Actionable tips (each with the evidence number, effect
   size, support n and confidence high/medium/low), Caveats and dead ends (what you tried that showed
   nothing; null results are valuable).
2. `queries-<PKG>.sql` with every final query you relied on.

Do not touch any other file, no git commands, no code changes, no service actions.
Do not invent numbers. Every number in the findings must come from a query you actually ran.
Final reply to the orchestrator: at most 20 lines with the strongest findings and their numbers.
