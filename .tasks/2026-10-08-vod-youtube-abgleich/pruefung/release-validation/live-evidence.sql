BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;
WITH current_checks AS (
    SELECT c.*
    FROM twitch_vod_archive_vods v
    JOIN LATERAL (
        SELECT a.id,a.platform_user_id,md5(COALESCE(a.refresh_token_enc::text,'')||COALESCE(a.platform_user_id,'')||COALESCE(a.authorized_at::text,'')) AS revision
        FROM social_media_platform_auth a
        WHERE a.twitch_user_id=v.twitch_user_id AND a.platform='youtube' AND a.enabled=1
        ORDER BY a.authorized_at DESC,a.id DESC LIMIT 1
    ) a ON TRUE
    JOIN twitch_vod_youtube_checks c ON c.vod_id=v.id AND c.auth_id=a.id AND c.auth_revision=a.revision
        AND c.channel_id IS NOT NULL AND (a.platform_user_id IS NULL OR c.channel_id=a.platform_user_id)
        AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(c.observations) o WHERE o->>'source_twitch_id' IS DISTINCT FROM v.twitch_id OR o->'source_duration_sec' IS DISTINCT FROM to_jsonb(v.duration_sec))
        AND c.upload_snapshot=(SELECT COALESCE(jsonb_agg(jsonb_build_object('id',p.id,'index',p.part_index,'status',p.status,'video_id',p.youtube_video_id,'updated_at',p.updated_at) ORDER BY p.part_index),'[]'::jsonb) FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id)
)
SELECT jsonb_build_object(
    'proof','current_evidence_quality','observed_at',NOW(),
    'state_counts',(SELECT jsonb_object_agg(state,n) FROM (SELECT state,COUNT(*) AS n FROM current_checks GROUP BY state) grouped),
    'confirmed_complete',COUNT(*) FILTER (WHERE state='confirmed' AND complete AND last_error IS NULL),
    'confirmed_empty',COUNT(*) FILTER (WHERE state='confirmed' AND complete AND last_error IS NULL AND jsonb_array_length(observations)=0),
    'confirmed_nonempty_processed',COUNT(*) FILTER (
        WHERE state='confirmed' AND complete AND last_error IS NULL AND jsonb_array_length(observations)>0
        AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(observations) o WHERE o->>'state' IS DISTINCT FROM 'processed' OR NULLIF(o->>'video_id','') IS NULL)
    ),
    'visibility_counts',(SELECT jsonb_object_agg(privacy,n) FROM (
        SELECT COALESCE(o->>'privacy','unknown') AS privacy,COUNT(*) AS n
        FROM current_checks c CROSS JOIN LATERAL jsonb_array_elements(c.observations) o
        GROUP BY COALESCE(o->>'privacy','unknown')
    ) grouped)
) FROM current_checks;
COMMIT;
