BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;

SELECT jsonb_build_object(
    'proof', 'archive_totals',
    'observed_at', NOW(),
    'vods', COUNT(*),
    'historical_completed', COUNT(*) FILTER (
        WHERE v.status IN ('uploaded','archived')
        AND EXISTS (SELECT 1 FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id)
        AND NOT EXISTS (SELECT 1 FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id AND (p.status IS DISTINCT FROM 'done' OR NULLIF(p.youtube_video_id,'') IS NULL))
    ),
    'current_checks', COUNT(c.vod_id),
    'confirmed_current', COUNT(*) FILTER (WHERE c.state='confirmed' AND c.complete AND attempt.last_error IS NULL),
    'processing', COUNT(*) FILTER (WHERE c.state='processing'),
    'unresolved', COUNT(*) FILTER (WHERE c.state='unresolved'),
    'errors', COUNT(*) FILTER (WHERE attempt.last_error IS NOT NULL)
)
FROM twitch_vod_archive_vods v
LEFT JOIN LATERAL (
    SELECT a.id, a.platform_user_id, md5(COALESCE(a.refresh_token_enc::text,'') || COALESCE(a.platform_user_id,'') || COALESCE(a.authorized_at::text,'')) AS revision
    FROM social_media_platform_auth a
    WHERE a.twitch_user_id=v.twitch_user_id AND a.platform='youtube' AND a.enabled=1
    ORDER BY a.authorized_at DESC,a.id DESC LIMIT 1
) a ON TRUE
LEFT JOIN twitch_vod_youtube_checks attempt ON attempt.vod_id=v.id AND attempt.auth_id=a.id AND attempt.auth_revision=a.revision
LEFT JOIN twitch_vod_youtube_checks c ON c.vod_id=v.id AND c.auth_id=a.id AND c.auth_revision=a.revision
    AND c.channel_id IS NOT NULL AND (a.platform_user_id IS NULL OR c.channel_id=a.platform_user_id)
    AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(c.observations) o WHERE o->>'source_twitch_id' IS DISTINCT FROM v.twitch_id OR o->'source_duration_sec' IS DISTINCT FROM to_jsonb(v.duration_sec))
    AND c.upload_snapshot=(SELECT COALESCE(jsonb_agg(jsonb_build_object('id',p.id,'index',p.part_index,'status',p.status,'video_id',p.youtube_video_id,'updated_at',p.updated_at) ORDER BY p.part_index),'[]'::jsonb) FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id);

SELECT jsonb_build_object(
    'proof', 'historical_case',
    'case_id', v.id,
    'legacy_state', v.status,
    'legacy_upload_time_present', v.uploaded_at IS NOT NULL,
    'duration_sec', v.duration_sec,
    'parts', (SELECT COUNT(*) FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id),
    'accepted_parts', (SELECT COUNT(*) FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id AND p.status='done' AND NULLIF(p.youtube_video_id,'') IS NOT NULL),
    'current_check', c.vod_id IS NOT NULL,
    'stored_check_present', EXISTS(SELECT 1 FROM twitch_vod_youtube_checks old WHERE old.vod_id=v.id),
    'state', CASE WHEN attempt.last_error IS NOT NULL THEN 'error' ELSE c.state END,
    'complete', CASE WHEN attempt.last_error IS NOT NULL THEN FALSE ELSE c.complete END,
    'error_present', attempt.last_error IS NOT NULL,
    'current_attempt_present', attempt.vod_id IS NOT NULL,
    'auth_id', c.auth_id,
    'actual_channel_observed', c.channel_id IS NOT NULL,
    'last_attempt_at', attempt.last_attempt_at,
    'last_success_at', c.last_success_at,
    'observations', COALESCE(jsonb_array_length(c.observations),0),
    'processed_observations', (SELECT COUNT(*) FROM jsonb_array_elements(COALESCE(c.observations,'[]'::jsonb)) o WHERE o->>'state'='processed'),
    'observed_duration_sec', (SELECT SUM((o->>'duration_sec')::bigint) FROM jsonb_array_elements(COALESCE(c.observations,'[]'::jsonb)) o),
    'oldest_observation_at', (SELECT MIN((o->>'observed_at')::timestamptz) FROM jsonb_array_elements(COALESCE(c.observations,'[]'::jsonb)) o),
    'newest_observation_at', (SELECT MAX((o->>'observed_at')::timestamptz) FROM jsonb_array_elements(COALESCE(c.observations,'[]'::jsonb)) o),
    'visibility', (SELECT jsonb_agg(DISTINCT o->>'privacy') FROM jsonb_array_elements(COALESCE(c.observations,'[]'::jsonb)) o),
    'scan_complete', s.complete,
    'scan_generation', s.generation,
    'scan_pending_page', s.cursor IS NOT NULL,
    'scan_last_success_at', s.last_success_at,
    'snapshot_current', c.vod_id IS NOT NULL
)
FROM twitch_vod_archive_vods v
LEFT JOIN LATERAL (
    SELECT a.id, a.platform_user_id, md5(COALESCE(a.refresh_token_enc::text,'') || COALESCE(a.platform_user_id,'') || COALESCE(a.authorized_at::text,'')) AS revision
    FROM social_media_platform_auth a
    WHERE a.twitch_user_id=v.twitch_user_id AND a.platform='youtube' AND a.enabled=1
    ORDER BY a.authorized_at DESC,a.id DESC LIMIT 1
) a ON TRUE
LEFT JOIN twitch_vod_youtube_checks attempt ON attempt.vod_id=v.id AND attempt.auth_id=a.id AND attempt.auth_revision=a.revision
LEFT JOIN twitch_vod_youtube_checks c ON c.vod_id=v.id AND c.auth_id=a.id AND c.auth_revision=a.revision
    AND c.channel_id IS NOT NULL AND (a.platform_user_id IS NULL OR c.channel_id=a.platform_user_id)
    AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(c.observations) o WHERE o->>'source_twitch_id' IS DISTINCT FROM v.twitch_id OR o->'source_duration_sec' IS DISTINCT FROM to_jsonb(v.duration_sec))
    AND c.upload_snapshot=(SELECT COALESCE(jsonb_agg(jsonb_build_object('id',p.id,'index',p.part_index,'status',p.status,'video_id',p.youtube_video_id,'updated_at',p.updated_at) ORDER BY p.part_index),'[]'::jsonb) FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id)
LEFT JOIN twitch_vod_youtube_scans s ON s.twitch_user_id=v.twitch_user_id AND s.auth_id=a.id AND s.auth_revision=a.revision
    AND s.channel_id IS NOT NULL AND (a.platform_user_id IS NULL OR s.channel_id=a.platform_user_id)
    AND (c.channel_id IS NULL OR s.channel_id=c.channel_id)
WHERE v.id IN (10,11,2941,2995,2996)
ORDER BY v.id;

SELECT jsonb_build_object(
    'proof', 'scan_account',
    'auth_id', s.auth_id,
    'current_account', s.auth_id=a.id AND s.auth_revision=a.revision AND s.channel_id IS NOT NULL AND (a.platform_user_id IS NULL OR s.channel_id=a.platform_user_id),
    'generation', s.generation,
    'complete', s.complete,
    'pending_page', s.cursor IS NOT NULL,
    'last_attempt_at', s.last_attempt_at,
    'last_success_at', s.last_success_at,
    'last_error_present', s.last_error IS NOT NULL,
    'source_marked_inventory', (SELECT COUNT(*) FROM twitch_vod_youtube_inventory i WHERE i.twitch_user_id=s.twitch_user_id AND i.generation=s.generation)
)
FROM twitch_vod_youtube_scans s
LEFT JOIN LATERAL (
    SELECT a.id, a.platform_user_id, md5(COALESCE(a.refresh_token_enc::text,'') || COALESCE(a.platform_user_id,'') || COALESCE(a.authorized_at::text,'')) AS revision
    FROM social_media_platform_auth a
    WHERE a.twitch_user_id=s.twitch_user_id AND a.platform='youtube' AND a.enabled=1
    ORDER BY a.authorized_at DESC,a.id DESC LIMIT 1
) a ON TRUE
ORDER BY s.auth_id;

COMMIT;
