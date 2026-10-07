SELECT jsonb_build_object(
    'proof', 'archive_totals',
    'vods', COUNT(*),
    'historical_completed', COUNT(*) FILTER (
        WHERE v.status IN ('uploaded','archived')
        AND EXISTS (SELECT 1 FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id)
        AND NOT EXISTS (SELECT 1 FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id AND (p.status IS DISTINCT FROM 'done' OR NULLIF(p.youtube_video_id,'') IS NULL))
    ),
    'checks', COUNT(c.vod_id),
    'confirmed_current', COUNT(*) FILTER (WHERE c.state='confirmed' AND c.complete),
    'processing', COUNT(*) FILTER (WHERE c.state='processing'),
    'unresolved', COUNT(*) FILTER (WHERE c.state='unresolved'),
    'errors', COUNT(*) FILTER (WHERE c.state='error')
)
FROM twitch_vod_archive_vods v
LEFT JOIN twitch_vod_youtube_checks c ON c.vod_id=v.id;

SELECT jsonb_build_object(
    'proof', 'historical_case',
    'case_id', v.id,
    'legacy_state', v.status,
    'legacy_upload_time_present', v.uploaded_at IS NOT NULL,
    'duration_sec', v.duration_sec,
    'parts', (SELECT COUNT(*) FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id),
    'accepted_parts', (SELECT COUNT(*) FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id AND p.status='done' AND NULLIF(p.youtube_video_id,'') IS NOT NULL),
    'state', c.state,
    'complete', c.complete,
    'error', c.last_error,
    'auth_id', c.auth_id,
    'actual_channel_observed', c.channel_id IS NOT NULL,
    'last_attempt_at', c.last_attempt_at,
    'last_success_at', c.last_success_at,
    'observations', COALESCE(jsonb_array_length(c.observations),0),
    'processed_observations', (SELECT COUNT(*) FROM jsonb_array_elements(COALESCE(c.observations,'[]'::jsonb)) o WHERE o->>'state'='processed'),
    'observed_duration_sec', (SELECT SUM((o->>'duration_sec')::bigint) FROM jsonb_array_elements(COALESCE(c.observations,'[]'::jsonb)) o),
    'visibility', (SELECT jsonb_agg(DISTINCT o->>'privacy') FROM jsonb_array_elements(COALESCE(c.observations,'[]'::jsonb)) o),
    'scan_complete', s.complete,
    'scan_generation', s.generation,
    'scan_pending_page', s.cursor IS NOT NULL,
    'scan_last_success_at', s.last_success_at,
    'snapshot_current', c.upload_snapshot=(SELECT COALESCE(jsonb_agg(jsonb_build_object('id',p.id,'index',p.part_index,'status',p.status,'video_id',p.youtube_video_id,'updated_at',p.updated_at) ORDER BY p.part_index),'[]'::jsonb) FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id)
)
FROM twitch_vod_archive_vods v
LEFT JOIN twitch_vod_youtube_checks c ON c.vod_id=v.id
LEFT JOIN twitch_vod_youtube_scans s ON s.twitch_user_id=v.twitch_user_id
WHERE v.id IN (10,11,2941,2995,2996)
ORDER BY v.id;

SELECT jsonb_build_object(
    'proof', 'scan_account',
    'auth_id', s.auth_id,
    'generation', s.generation,
    'complete', s.complete,
    'pending_page', s.cursor IS NOT NULL,
    'last_attempt_at', s.last_attempt_at,
    'last_success_at', s.last_success_at,
    'last_error', s.last_error,
    'source_marked_inventory', (SELECT COUNT(*) FROM twitch_vod_youtube_inventory i WHERE i.twitch_user_id=s.twitch_user_id AND i.generation=s.generation)
)
FROM twitch_vod_youtube_scans s
ORDER BY s.auth_id;

SELECT jsonb_build_object(
    'proof', 'migration',
    'version', version,
    'success', success,
    'checksum_sha384', encode(checksum,'hex')
)
FROM _sqlx_migrations
WHERE version=20261008003000;
