\pset tuples_only on
\pset format unaligned
\pset fieldsep '\t'
SELECT s.twitch_login,s.enabled,s.strategy,s.action_lead_seconds,st.is_live,st.next_ad_at,st.last_ad_at,st.snooze_count,st.observed_at,st.worker_heartbeat_at,st.last_decision_reason,st.match_active,st.match_started_at,st.match_ended_at,st.avg_match_seconds,st.avg_queue_seconds,st.last_action_kind,st.last_action_outcome,st.last_action_at
FROM twitch_ad_manager_settings s
LEFT JOIN twitch_ad_manager_state st USING (twitch_user_id)
WHERE LOWER(s.twitch_login)='earlysalty';
