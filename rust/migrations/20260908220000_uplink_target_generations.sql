-- Keine Tokens: dauerhafte Versionsgrenze für Zieloperationen über RPC.
-- nextval wird bei Rollback nicht zurückgesetzt; keine Generation wird wiederverwendet.
CREATE SEQUENCE IF NOT EXISTS uplink_target_generation_seq AS bigint MINVALUE 1;
CREATE TABLE IF NOT EXISTS uplink_target_generations (
    twitch_user_id text NOT NULL CHECK (twitch_user_id ~ '^[1-9][0-9]*$'),
    platform text NOT NULL CHECK (platform IN ('twitch','kick','youtube','tiktok')),
    generation bigint NOT NULL CHECK (generation > 0),
    enabled boolean NOT NULL,
    disconnect_pending boolean NOT NULL DEFAULT false,
    last_disconnected_at timestamptz,
    PRIMARY KEY (twitch_user_id, platform),
    CHECK (NOT disconnect_pending OR NOT enabled),
    CHECK (enabled OR last_disconnected_at IS NOT NULL)
);
