ALTER TABLE social_media_partner_access ADD COLUMN twitch_user_id TEXT;
CREATE UNIQUE INDEX social_media_partner_access_identity
    ON social_media_partner_access (twitch_user_id)
    WHERE twitch_user_id IS NOT NULL;
