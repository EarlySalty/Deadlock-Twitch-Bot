BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;
SELECT jsonb_build_object(
    'proof','release_before_totals','observed_at',NOW(),'vods',COUNT(*),
    'historical_completed',COUNT(*) FILTER (
        WHERE v.status IN ('uploaded','archived')
        AND EXISTS (SELECT 1 FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id)
        AND NOT EXISTS (SELECT 1 FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id AND (p.status IS DISTINCT FROM 'done' OR NULLIF(p.youtube_video_id,'') IS NULL))
    )
) FROM twitch_vod_archive_vods v;
SELECT jsonb_build_object(
    'proof','release_before_case','case_id',v.id,'legacy_state',v.status,
    'legacy_upload_time_present',v.uploaded_at IS NOT NULL,'duration_sec',v.duration_sec,
    'parts',(SELECT COUNT(*) FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id),
    'accepted_parts',(SELECT COUNT(*) FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id AND p.status='done' AND NULLIF(p.youtube_video_id,'') IS NOT NULL)
) FROM twitch_vod_archive_vods v WHERE v.id IN (10,11,2941,2995,2996) ORDER BY v.id;
COMMIT;
