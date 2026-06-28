-- Engagement smoke-account sender auth must be part of the SQLx migration
-- contract; runtime DDL remains idempotent for older deployments.

CREATE TABLE IF NOT EXISTS twitch_engagement_sender_auth (
    twitch_user_id     TEXT PRIMARY KEY,
    twitch_login       TEXT NOT NULL,
    access_token_enc   BYTEA NOT NULL,
    refresh_token_enc  BYTEA NOT NULL,
    scopes             TEXT,
    token_expires_at   BIGINT NOT NULL,
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);
