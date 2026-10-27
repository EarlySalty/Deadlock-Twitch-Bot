\pset tuples_only on
\pset format unaligned
SELECT EXISTS(SELECT 1 FROM twitch_player_steam_links l JOIN twitch_ad_manager_settings s USING (twitch_user_id) WHERE LOWER(s.twitch_login)='earlysalty' AND l.lookup_enabled AND l.steam_id64 IS NOT NULL);
SELECT COUNT(*) FROM twitch_player_steam_accounts a JOIN twitch_ad_manager_settings s USING (twitch_user_id) WHERE LOWER(s.twitch_login)='earlysalty';
