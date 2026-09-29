ALTER TABLE social_media_streamer_layout ALTER COLUMN mode SET DEFAULT 'stacked';
ALTER TABLE social_media_streamer_layout DROP CONSTRAINT IF EXISTS social_media_layout_mode_chk;
ALTER TABLE social_media_streamer_layout ADD CONSTRAINT social_media_layout_mode_chk CHECK (mode IN ('pip', 'stacked', 'blur_pad'));

UPDATE social_media_streamer_layout
SET mode = 'stacked',
    layout_json = jsonb_set(
        jsonb_set(layout_json, '{cam_position}', '{"x":0,"y":0,"w":1080,"h":600}'::jsonb),
        '{game_crop}', jsonb_build_object('x', 0, 'y', 0, 'w', (layout_json->'source'->>'width')::int, 'h', (layout_json->'source'->>'height')::int)
    )
WHERE LOWER(streamer_login) = 'earlysalty'
  AND mode = 'pip'
  AND cam_enabled
  AND layout_json = '{"source":{"width":1920,"height":1080},"version":1,"cam_crop":{"x":41,"y":237,"w":303,"h":336},"game_crop":{"x":287,"y":0,"w":1080,"h":1080},"cam_position":{"x":0,"y":0,"w":636,"h":454}}'::jsonb;

UPDATE twitch_clips_social_media
SET layout_override_json = jsonb_set(
    jsonb_set(
        jsonb_set(layout_override_json, '{mode}', '"stacked"'::jsonb),
        '{cam_position}', '{"x":0,"y":0,"w":1080,"h":600}'::jsonb
    ),
    '{game_crop}', jsonb_build_object('x', 0, 'y', 0, 'w', (layout_override_json->'source'->>'width')::int, 'h', (layout_override_json->'source'->>'height')::int)
)
WHERE LOWER(streamer_login) = 'earlysalty'
  AND layout_override_json IN (
      '{"mode":"pip","source":{"width":1920,"height":1080},"version":1,"cam_crop":{"x":41,"y":237,"w":303,"h":336},"game_crop":{"x":287,"y":0,"w":1080,"h":1080},"cam_enabled":true,"cam_position":{"x":0,"y":0,"w":636,"h":454}}'::jsonb,
      '{"mode":"pip","source":{"width":1920,"height":1080},"version":1,"cam_crop":{"x":1500,"y":50,"w":380,"h":380},"game_crop":{"x":0,"y":0,"w":1080,"h":1080},"cam_enabled":true,"cam_position":{"x":712,"y":48,"w":320,"h":320}}'::jsonb
  );
