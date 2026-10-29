ALTER TABLE social_media_streamer_layout DROP CONSTRAINT IF EXISTS social_media_layout_mode_chk;
ALTER TABLE social_media_streamer_layout ADD CONSTRAINT social_media_layout_mode_chk CHECK (mode IN ('pip', 'stacked', 'blur_pad', 'fill'));
