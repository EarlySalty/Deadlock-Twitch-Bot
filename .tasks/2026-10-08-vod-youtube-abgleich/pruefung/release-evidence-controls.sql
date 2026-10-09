BEGIN READ ONLY;
WITH fixtures(name, observations) AS (
    VALUES
        ('empty', '[]'::jsonb),
        ('valid_part', '[{"video_id":"synthetic-existing","part_index":0,"state":"processed","source_twitch_id":"synthetic-source","source_duration_sec":120}]'::jsonb)
)
SELECT jsonb_build_object(
    'proof', 'reporter_control',
    'fixture', name,
    'original_source_guard_passes', NOT EXISTS (
        SELECT 1 FROM jsonb_array_elements(observations) o
        WHERE o->>'source_twitch_id' IS DISTINCT FROM 'synthetic-source'
           OR o->'source_duration_sec' IS DISTINCT FROM to_jsonb(120::bigint)
    ),
    'nonempty', jsonb_array_length(observations)>0,
    'processed_observations', (SELECT COUNT(*) FROM jsonb_array_elements(observations) o WHERE o->>'state'='processed')
)
FROM fixtures ORDER BY name;
COMMIT;
