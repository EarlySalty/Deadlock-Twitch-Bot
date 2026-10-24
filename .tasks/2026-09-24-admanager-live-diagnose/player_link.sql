\pset tuples_only on
\pset format unaligned
\pset fieldsep '\t'
SELECT l.twitch_user_id,l.lookup_enabled,COALESCE(l.steam_id64::text,'NONE'),l.updated_at FROM twitch_player_steam_links l JOIN twitch_ad_manager_settings s USING (twitch_user_id) WHERE LOWER(s.twitch_login)='earlysalty';
SELECT a.steam_id64,a.is_primary,a.verified,a.friend_state,a.updated_at FROM twitch_player_steam_accounts a JOIN twitch_ad_manager_settings s USING (twitch_user_id) WHERE LOWER(s.twitch_login)='earlysalty' ORDER BY a.is_primary DESC,a.updated_at DESC;
