UPDATE social_media_autopilot_settings
SET distribution_policy = CASE WHEN schedule_scope = 'per_platform' THEN 'independent_clips' ELSE 'same_clip' END,
    policy_revision = policy_revision + 1
WHERE distribution_policy <> CASE WHEN schedule_scope = 'per_platform' THEN 'independent_clips' ELSE 'same_clip' END;
