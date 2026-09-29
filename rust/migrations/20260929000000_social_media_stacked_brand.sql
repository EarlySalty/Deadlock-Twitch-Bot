ALTER TABLE social_media_streamer_layout ALTER COLUMN mode SET DEFAULT 'stacked';
ALTER TABLE social_media_streamer_layout DROP CONSTRAINT IF EXISTS social_media_layout_mode_chk;
ALTER TABLE social_media_streamer_layout ADD CONSTRAINT social_media_layout_mode_chk CHECK (mode IN ('pip', 'stacked', 'blur_pad'));

UPDATE social_media_streamer_layout
SET mode = 'stacked',
    layout_json = jsonb_set(
        jsonb_set(layout_json, '{cam_position}', '{"x":0,"y":0,"w":1080,"h":600}'::jsonb),
        '{game_crop}', jsonb_build_object('x', 0, 'y', 0, 'w', (layout_json->'source'->>'width')::int, 'h', (layout_json->'source'->>'height')::int)
    )
WHERE LOWER(streamer_login) = 'earlysalty' AND mode = 'pip' AND cam_enabled;

UPDATE twitch_clips_social_media
SET layout_override_json = jsonb_set(
    jsonb_set(
        jsonb_set(layout_override_json, '{mode}', '"stacked"'::jsonb),
        '{cam_position}', '{"x":0,"y":0,"w":1080,"h":600}'::jsonb
    ),
    '{game_crop}', jsonb_build_object('x', 0, 'y', 0, 'w', (layout_override_json->'source'->>'width')::int, 'h', (layout_override_json->'source'->>'height')::int)
)
WHERE LOWER(streamer_login) = 'earlysalty'
  AND layout_override_json->>'mode' = 'pip'
  AND COALESCE(layout_override_json->>'cam_enabled', 'true') = 'true';
