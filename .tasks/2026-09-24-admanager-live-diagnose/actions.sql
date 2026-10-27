\pset tuples_only on
\pset format unaligned
\pset fieldsep '\t'
SELECT a.created_at,a.action,a.status,a.source,a.duration_seconds,a.completed_at FROM twitch_ad_manager_actions a JOIN twitch_ad_manager_settings s USING (twitch_user_id) WHERE LOWER(s.twitch_login)='earlysalty' ORDER BY a.created_at DESC LIMIT 30;
