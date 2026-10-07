ALTER TABLE twitch_clips_social_media ADD COLUMN IF NOT EXISTS tiktok_post_options JSONB;
ALTER TABLE twitch_clips_upload_queue ADD COLUMN IF NOT EXISTS tiktok_post_options JSONB;
ALTER TABLE twitch_clips_upload_queue ADD COLUMN IF NOT EXISTS tiktok_publish_status TEXT;

CREATE OR REPLACE FUNCTION snapshot_tiktok_post_options() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.platform = 'tiktok' THEN
        SELECT tiktok_post_options INTO NEW.tiktok_post_options
        FROM twitch_clips_social_media WHERE id = NEW.clip_id;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER snapshot_tiktok_post_options
BEFORE INSERT ON twitch_clips_upload_queue
FOR EACH ROW EXECUTE FUNCTION snapshot_tiktok_post_options();
