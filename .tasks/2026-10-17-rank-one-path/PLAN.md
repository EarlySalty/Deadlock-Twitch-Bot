# !rank one path

Goal: `!rank` resolves the Steam account from every identity source first, then reads the rank.

1. Identity
   - `!connect` link (twitch_player_steam_links / twitch_player_steam_accounts), honouring lookup_enabled and the revision recheck.
   - Discord link via twitch_streamer_identities.discord_user_id and the Steam bot central link.
   - Name search (`name_reply`) only for explicit lookups without any link. A failed link lookup never falls back to a name search.
2. Rank
   - Ask the Steam bot `/rank` (`steam_id` or `discord_id`, `refresh=1`, 30 s timeout) and the Deadlock API in parallel.
   - Newer timestamp wins: bot `rank_updated_at` vs API last ranked match start_time. Missing or tied timestamps go to the Steam bot.
   - Source and date labelled in chat.
   - Existing unverified / not friend / no rank texts only when no source produced a rank.

Dependency: Steam bot branch feat/rank-by-steam-id (additive `steam_id` and `refresh` on `/rank`) must be deployed first.
