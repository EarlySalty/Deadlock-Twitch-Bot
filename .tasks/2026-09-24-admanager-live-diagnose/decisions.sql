\pset tuples_only on
\pset format unaligned
\pset fieldsep '\t'
SELECT d.decided_at,d.decision,d.reason,d.block_seconds FROM twitch_ad_manager_decisions d JOIN twitch_ad_manager_settings s USING (twitch_user_id) WHERE LOWER(s.twitch_login)='earlysalty' ORDER BY d.decided_at DESC LIMIT 30;
