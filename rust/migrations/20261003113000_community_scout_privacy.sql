-- Ausschließlich Privacy für die neuen Community-Vorschlagskopien.
ALTER TABLE twitch_scout_community_suggestions
    ADD COLUMN submitted_at TIMESTAMPTZ,
    ADD COLUMN privacy_epoch BIGINT;
CREATE TABLE twitch_scout_community_privacy (
    identity_hash TEXT PRIMARY KEY CHECK (identity_hash ~ '^[0-9a-f]{64}$'),
    epoch BIGINT NOT NULL CHECK (epoch > 0),
    operation_id UUID NOT NULL UNIQUE,
    activity_since TIMESTAMPTZ
);
CREATE TABLE twitch_scout_erased_replay_keys (
    key_hash TEXT PRIMARY KEY CHECK (key_hash ~ '^[0-9a-f]{64}$')
);
