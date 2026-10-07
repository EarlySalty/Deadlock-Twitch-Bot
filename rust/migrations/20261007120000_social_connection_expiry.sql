ALTER TABLE social_media_platform_auth
    ADD COLUMN refresh_expires_at TIMESTAMPTZ,
    ADD COLUMN needs_reauth BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN reauth_required_at TIMESTAMPTZ,
    ADD COLUMN reauth_notified_at TIMESTAMPTZ;

CREATE INDEX idx_social_media_reauth_pending
    ON social_media_platform_auth (id)
    WHERE enabled = 1 AND reauth_required_at IS NOT NULL AND reauth_notified_at IS NULL;
