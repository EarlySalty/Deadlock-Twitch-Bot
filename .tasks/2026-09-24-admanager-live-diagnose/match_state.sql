\pset tuples_only on
\pset format unaligned
\pset fieldsep '\t'
SELECT channel_login,hero_name,match_id,match_started_at,last_synced_at,is_live FROM twitch_channel_match_state WHERE LOWER(channel_login)='earlysalty';
SELECT channel_login,enabled,COALESCE(NULLIF(BTRIM(steam_id),''),'NONE') FROM twitch_engagement_settings WHERE LOWER(channel_login)='earlysalty';
