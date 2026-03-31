-- Twitch Analytics – Postgres/TimescaleDB Schema (lossless, no retention)
-- Run: psql "$TWITCH_ANALYTICS_DSN" -f cogs/twitch/migrations/twitch_analytics_schema.sql
-- Assumptions: Postgres 16+, extension timescaledb installed.

CREATE EXTENSION IF NOT EXISTS timescaledb;

-- ========= Core Dimension =========
CREATE TABLE IF NOT EXISTS streamer_dim (
    twitch_login           TEXT PRIMARY KEY,
    twitch_user_id         TEXT,
    discord_user_id        TEXT,
    discord_display_name   TEXT,
    is_partner             BOOLEAN DEFAULT FALSE,
    is_monitored_only      BOOLEAN DEFAULT FALSE,
    archived_at            TIMESTAMPTZ,
    updated_at             TIMESTAMPTZ DEFAULT NOW()
);

-- ========= Streamer Registry =========
CREATE TABLE IF NOT EXISTS twitch_streamers (
    id                       BIGSERIAL PRIMARY KEY,
    twitch_login             TEXT UNIQUE NOT NULL,
    twitch_user_id           TEXT,
    require_discord_link     INTEGER,
    last_description         TEXT,
    last_link_ok             INTEGER,
    added_by                 TEXT,
    created_at               TIMESTAMPTZ DEFAULT NOW(),
    last_link_checked_at     TIMESTAMPTZ,
    next_link_check_at       TIMESTAMPTZ,
    manual_verified_permanent INTEGER DEFAULT 0,
    manual_verified_until    TIMESTAMPTZ,
    manual_verified_at       TIMESTAMPTZ,
    discord_user_id          TEXT,
    discord_display_name     TEXT,
    is_on_discord            INTEGER DEFAULT 0,
    manual_partner_opt_out   INTEGER DEFAULT 0,
    raid_bot_enabled         INTEGER DEFAULT 0,
    archived_at              TIMESTAMPTZ,
    silent_ban               INTEGER DEFAULT 0,
    silent_raid              INTEGER DEFAULT 0,
    is_monitored_only        INTEGER DEFAULT 0,
    live_ping_role_id        BIGINT,
    live_ping_enabled        INTEGER DEFAULT 1
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_streamers_login_lower
    ON twitch_streamers (LOWER(twitch_login));

-- Companion view: calculated partner flags
-- Drop/recreate avoids CREATE OR REPLACE failures when prior versions used s.*
-- and later table columns changed order.
DROP VIEW IF EXISTS twitch_streamers_partner_state;
CREATE VIEW twitch_streamers_partner_state AS
WITH base AS (
    SELECT
        s.twitch_login,
        s.twitch_user_id,
        s.require_discord_link,
        s.next_link_check_at,
        s.discord_user_id,
        s.discord_display_name,
        s.is_on_discord,
        s.manual_verified_permanent,
        s.manual_verified_until,
        s.manual_verified_at,
        s.manual_partner_opt_out,
        s.created_at,
        s.archived_at,
        s.raid_bot_enabled,
        s.silent_ban,
        s.silent_raid,
        s.is_monitored_only,
        CASE
            WHEN COALESCE(s.manual_verified_permanent, 0) = 1
                 OR (s.manual_verified_until IS NOT NULL AND s.manual_verified_until >= NOW())
                 OR s.manual_verified_at IS NOT NULL
            THEN 1 ELSE 0
        END AS is_verified,
        s.live_ping_role_id,
        COALESCE(s.live_ping_enabled, 1) AS live_ping_enabled
    FROM twitch_streamers s
)
SELECT
    base.twitch_login,
    base.twitch_user_id,
    base.require_discord_link,
    base.next_link_check_at,
    base.discord_user_id,
    base.discord_display_name,
    base.is_on_discord,
    base.manual_verified_permanent,
    base.manual_verified_until,
    base.manual_verified_at,
    base.manual_partner_opt_out,
    base.created_at,
    base.archived_at,
    base.raid_bot_enabled,
    base.silent_ban,
    base.silent_raid,
    base.is_monitored_only,
    base.is_verified,
    CASE
        WHEN base.is_verified = 1
             AND COALESCE(base.manual_partner_opt_out, 0) = 0
             AND COALESCE(base.is_monitored_only, 0) = 0
        THEN 1 ELSE 0
    END AS is_partner,
    CASE
        WHEN base.is_verified = 1
             AND COALESCE(base.manual_partner_opt_out, 0) = 0
             AND COALESCE(base.is_monitored_only, 0) = 0
        THEN 1 ELSE 0
    END AS is_partner_active,
    base.live_ping_role_id,
    base.live_ping_enabled
FROM base;

-- ========= Live State =========
CREATE TABLE IF NOT EXISTS twitch_live_state (
    twitch_user_id            TEXT PRIMARY KEY,
    streamer_login            TEXT NOT NULL,
    last_stream_id            TEXT,
    last_started_at           TEXT,
    last_title                TEXT,
    last_game_id              TEXT,
    last_discord_message_id   TEXT,
    last_notified_at          TEXT,
    is_live                   INTEGER DEFAULT 0,
    last_seen_at              TEXT,
    last_game                 TEXT,
    last_viewer_count         INTEGER DEFAULT 0,
    last_tracking_token       TEXT,
    active_session_id         BIGINT,
    had_deadlock_in_session   INTEGER DEFAULT 0,
    last_deadlock_seen_at     TEXT
);
CREATE INDEX IF NOT EXISTS idx_twitch_live_state_login ON twitch_live_state(streamer_login);
CREATE INDEX IF NOT EXISTS idx_twitch_live_state_is_live ON twitch_live_state(is_live);
CREATE INDEX IF NOT EXISTS idx_twitch_live_state_active_session ON twitch_live_state(active_session_id);

-- ========= OAuth Tokens (Raid Auth) =========
CREATE TABLE IF NOT EXISTS twitch_raid_auth (
    twitch_user_id        TEXT PRIMARY KEY,
    twitch_login          TEXT NOT NULL,
    access_token          TEXT DEFAULT 'ENC',
    refresh_token         TEXT DEFAULT 'ENC',
    token_expires_at      TIMESTAMPTZ NOT NULL,
    scopes                TEXT NOT NULL,
    authorized_at         TIMESTAMPTZ DEFAULT NOW(),
    last_refreshed_at     TIMESTAMPTZ,
    raid_enabled          BOOLEAN DEFAULT TRUE,
    created_at            TIMESTAMPTZ DEFAULT NOW(),
    legacy_access_token   TEXT,
    legacy_refresh_token  TEXT,
    legacy_scopes         TEXT,
    legacy_saved_at       TIMESTAMPTZ,
    needs_reauth          BOOLEAN DEFAULT FALSE,
    reauth_notified_at    TIMESTAMPTZ,
    access_token_enc      BYTEA,
    refresh_token_enc     BYTEA,
    enc_version           INTEGER DEFAULT 1,
    enc_kid               TEXT DEFAULT 'v1',
    enc_migrated_at       TIMESTAMPTZ
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_raid_auth_login
    ON twitch_raid_auth (LOWER(twitch_login));

-- ========= Sessions & Chat =========
CREATE TABLE IF NOT EXISTS twitch_stream_sessions (
    id                  BIGSERIAL PRIMARY KEY,
    streamer_login      TEXT NOT NULL,
    stream_id           TEXT,
    started_at          TIMESTAMPTZ NOT NULL,
    ended_at            TIMESTAMPTZ,
    duration_seconds    INTEGER DEFAULT 0,
    start_viewers       INTEGER DEFAULT 0,
    peak_viewers        INTEGER DEFAULT 0,
    end_viewers         INTEGER DEFAULT 0,
    avg_viewers         DOUBLE PRECISION DEFAULT 0,
    samples             INTEGER DEFAULT 0,
    retention_5m        DOUBLE PRECISION,
    retention_10m       DOUBLE PRECISION,
    retention_20m       DOUBLE PRECISION,
    dropoff_pct         DOUBLE PRECISION,
    dropoff_label       TEXT,
    unique_chatters     INTEGER DEFAULT 0,
    first_time_chatters INTEGER DEFAULT 0,
    returning_chatters  INTEGER DEFAULT 0,
    followers_start     INTEGER,
    followers_end       INTEGER,
    follower_delta      INTEGER,
    stream_title        TEXT,
    notification_text   TEXT,
    language            TEXT,
    is_mature           BOOLEAN DEFAULT FALSE,
    tags                TEXT,
    had_deadlock_in_session BOOLEAN DEFAULT FALSE,
    game_name           TEXT,
    notes               TEXT
);
SELECT create_hypertable('twitch_stream_sessions', 'started_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
SELECT add_compression_policy('twitch_stream_sessions', INTERVAL '7 days', if_not_exists => TRUE);
ALTER TABLE twitch_stream_sessions SET (timescaledb.compress, timescaledb.compress_segmentby = 'streamer_login', timescaledb.compress_orderby = 'started_at DESC');
CREATE INDEX IF NOT EXISTS idx_twitch_sessions_login ON twitch_stream_sessions(streamer_login, started_at);
CREATE INDEX IF NOT EXISTS idx_twitch_sessions_open ON twitch_stream_sessions(streamer_login) WHERE ended_at IS NULL;

CREATE TABLE IF NOT EXISTS twitch_session_viewers (
    session_id          BIGINT NOT NULL REFERENCES twitch_stream_sessions(id) ON DELETE CASCADE,
    ts_utc              TIMESTAMPTZ NOT NULL,
    minutes_from_start  INTEGER,
    viewer_count        INTEGER NOT NULL,
    PRIMARY KEY(session_id, ts_utc)
);
SELECT create_hypertable('twitch_session_viewers', 'ts_utc', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '1 day');
ALTER TABLE twitch_session_viewers SET (timescaledb.compress, timescaledb.compress_segmentby = 'session_id', timescaledb.compress_orderby = 'ts_utc DESC');
SELECT add_compression_policy('twitch_session_viewers', INTERVAL '7 days', if_not_exists => TRUE);

CREATE TABLE IF NOT EXISTS twitch_session_chatters (
    session_id            BIGINT NOT NULL REFERENCES twitch_stream_sessions(id) ON DELETE CASCADE,
    streamer_login        TEXT NOT NULL,
    chatter_login         TEXT NOT NULL,
    chatter_id            TEXT,
    first_message_at      TIMESTAMPTZ NOT NULL,
    messages              INTEGER DEFAULT 0,
    is_first_time_streamer  BOOLEAN DEFAULT FALSE,
    seen_via_chatters_api BOOLEAN DEFAULT FALSE,
    last_seen_at          TIMESTAMPTZ,
    PRIMARY KEY (session_id, chatter_login)
);
SELECT create_hypertable('twitch_session_chatters', 'first_message_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
ALTER TABLE twitch_session_chatters SET (timescaledb.compress, timescaledb.compress_segmentby = 'session_id,streamer_login', timescaledb.compress_orderby = 'first_message_at DESC');
SELECT add_compression_policy('twitch_session_chatters', INTERVAL '7 days', if_not_exists => TRUE);
CREATE INDEX IF NOT EXISTS idx_twitch_session_chatters_login ON twitch_session_chatters(streamer_login, session_id);

-- Global chatter rollup per streamer (viewer overlap, uniques)
CREATE TABLE IF NOT EXISTS twitch_chatter_rollup (
    streamer_login   TEXT NOT NULL,
    chatter_login    TEXT NOT NULL,
    chatter_id       TEXT,
    first_seen_at    TIMESTAMPTZ NOT NULL,
    last_seen_at     TIMESTAMPTZ NOT NULL,
    total_messages   INTEGER DEFAULT 0,
    total_sessions   INTEGER DEFAULT 0,
    PRIMARY KEY (streamer_login, chatter_login)
);
CREATE INDEX IF NOT EXISTS idx_twitch_chatter_rollup_chatter ON twitch_chatter_rollup(chatter_login);

CREATE TABLE IF NOT EXISTS twitch_chat_messages (
    id               BIGSERIAL PRIMARY KEY,
    session_id       BIGINT NOT NULL REFERENCES twitch_stream_sessions(id) ON DELETE CASCADE,
    streamer_login   TEXT NOT NULL,
    chatter_login    TEXT,
    chatter_id       TEXT,
    message_id       TEXT,
    message_ts       TIMESTAMPTZ NOT NULL,
    is_command       BOOLEAN DEFAULT FALSE,
    content          TEXT
);
SELECT create_hypertable('twitch_chat_messages', 'message_ts', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '1 day');
ALTER TABLE twitch_chat_messages SET (timescaledb.compress, timescaledb.compress_segmentby = 'streamer_login,session_id', timescaledb.compress_orderby = 'message_ts DESC');
SELECT add_compression_policy('twitch_chat_messages', INTERVAL '7 days', if_not_exists => TRUE);
CREATE INDEX IF NOT EXISTS idx_twitch_chat_messages_session ON twitch_chat_messages(session_id, message_ts);
CREATE INDEX IF NOT EXISTS idx_twitch_chat_messages_streamer_ts ON twitch_chat_messages(streamer_login, message_ts);
CREATE INDEX IF NOT EXISTS idx_twitch_chat_messages_message_id ON twitch_chat_messages(message_id);

CREATE TABLE IF NOT EXISTS twitch_observability_events (
    id           BIGSERIAL PRIMARY KEY,
    flow_type    TEXT NOT NULL,
    flow_id      TEXT NOT NULL,
    entity_login TEXT,
    entity_id    TEXT,
    step         TEXT NOT NULL,
    decision     TEXT NOT NULL,
    details_json TEXT NOT NULL DEFAULT '{}',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
SELECT create_hypertable('twitch_observability_events', 'created_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
ALTER TABLE twitch_observability_events SET (timescaledb.compress, timescaledb.compress_segmentby = 'flow_type,flow_id', timescaledb.compress_orderby = 'created_at DESC');
SELECT add_compression_policy('twitch_observability_events', INTERVAL '7 days', if_not_exists => TRUE);
CREATE INDEX IF NOT EXISTS idx_twitch_observability_events_flow ON twitch_observability_events(flow_type, flow_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_twitch_observability_events_entity ON twitch_observability_events(entity_login, created_at DESC);

-- ========= Periodic Stats =========
CREATE TABLE IF NOT EXISTS twitch_stats_tracked (
    ts_utc       TIMESTAMPTZ NOT NULL,
    streamer     TEXT NOT NULL,
    viewer_count INTEGER,
    is_partner   BOOLEAN DEFAULT FALSE,
    game_name    TEXT,
    stream_title TEXT,
    tags         TEXT
);
SELECT create_hypertable('twitch_stats_tracked', 'ts_utc', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
ALTER TABLE twitch_stats_tracked SET (timescaledb.compress, timescaledb.compress_segmentby = 'streamer', timescaledb.compress_orderby = 'ts_utc DESC');
SELECT add_compression_policy('twitch_stats_tracked', INTERVAL '7 days', if_not_exists => TRUE);

CREATE TABLE IF NOT EXISTS twitch_stats_category (
    ts_utc       TIMESTAMPTZ NOT NULL,
    streamer     TEXT NOT NULL,
    viewer_count INTEGER,
    is_partner   BOOLEAN DEFAULT FALSE,
    game_name    TEXT,
    stream_title TEXT,
    tags         TEXT
);
SELECT create_hypertable('twitch_stats_category', 'ts_utc', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
ALTER TABLE twitch_stats_category SET (timescaledb.compress, timescaledb.compress_segmentby = 'streamer', timescaledb.compress_orderby = 'ts_utc DESC');
SELECT add_compression_policy('twitch_stats_category', INTERVAL '7 days', if_not_exists => TRUE);

-- ========= Events =========
CREATE TABLE IF NOT EXISTS twitch_follow_events (
    id             BIGSERIAL PRIMARY KEY,
    streamer_login TEXT NOT NULL,
    twitch_user_id TEXT NOT NULL,
    follower_login TEXT NOT NULL,
    follower_id    TEXT,
    followed_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
SELECT create_hypertable('twitch_follow_events', 'followed_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
ALTER TABLE twitch_follow_events SET (timescaledb.compress, timescaledb.compress_segmentby = 'streamer_login', timescaledb.compress_orderby = 'followed_at DESC');
SELECT add_compression_policy('twitch_follow_events', INTERVAL '7 days', if_not_exists => TRUE);
CREATE INDEX IF NOT EXISTS idx_twitch_follow_events_streamer ON twitch_follow_events(streamer_login, followed_at);

CREATE TABLE IF NOT EXISTS twitch_subscription_events (
    id                 BIGSERIAL PRIMARY KEY,
    session_id         BIGINT,
    twitch_user_id     TEXT NOT NULL,
    event_type         TEXT NOT NULL,
    user_login         TEXT,
    tier               TEXT,
    is_gift            BOOLEAN DEFAULT FALSE,
    gifter_login       TEXT,
    cumulative_months  INTEGER,
    streak_months      INTEGER,
    message            TEXT,
    total_gifted       INTEGER,
    received_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
SELECT create_hypertable('twitch_subscription_events', 'received_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
ALTER TABLE twitch_subscription_events SET (timescaledb.compress, timescaledb.compress_segmentby = 'twitch_user_id', timescaledb.compress_orderby = 'received_at DESC');
SELECT add_compression_policy('twitch_subscription_events', INTERVAL '7 days', if_not_exists => TRUE);

CREATE TABLE IF NOT EXISTS twitch_channel_points_events (
    id              BIGSERIAL PRIMARY KEY,
    session_id      BIGINT,
    twitch_user_id  TEXT NOT NULL,
    user_login      TEXT,
    reward_id       TEXT,
    reward_title    TEXT,
    reward_cost     INTEGER,
    user_input      TEXT,
    redeemed_at     TIMESTAMPTZ NOT NULL
);
SELECT create_hypertable('twitch_channel_points_events', 'redeemed_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
ALTER TABLE twitch_channel_points_events SET (timescaledb.compress, timescaledb.compress_segmentby = 'twitch_user_id', timescaledb.compress_orderby = 'redeemed_at DESC');
SELECT add_compression_policy('twitch_channel_points_events', INTERVAL '7 days', if_not_exists => TRUE);

CREATE TABLE IF NOT EXISTS twitch_bits_events (
    id              BIGSERIAL PRIMARY KEY,
    session_id      BIGINT,
    twitch_user_id  TEXT NOT NULL,
    donor_login     TEXT,
    amount          INTEGER NOT NULL,
    message         TEXT,
    received_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
SELECT create_hypertable('twitch_bits_events', 'received_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
ALTER TABLE twitch_bits_events SET (timescaledb.compress, timescaledb.compress_segmentby = 'twitch_user_id', timescaledb.compress_orderby = 'received_at DESC');
SELECT add_compression_policy('twitch_bits_events', INTERVAL '7 days', if_not_exists => TRUE);

CREATE TABLE IF NOT EXISTS twitch_hype_train_events (
    id               BIGSERIAL PRIMARY KEY,
    session_id       BIGINT,
    twitch_user_id   TEXT NOT NULL,
    started_at       TIMESTAMPTZ,
    ended_at         TIMESTAMPTZ,
    duration_seconds INTEGER,
    level            INTEGER,
    total_progress   INTEGER,
    event_phase      TEXT DEFAULT 'end'
);
SELECT create_hypertable('twitch_hype_train_events', 'started_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
ALTER TABLE twitch_hype_train_events SET (timescaledb.compress, timescaledb.compress_segmentby = 'twitch_user_id', timescaledb.compress_orderby = 'started_at DESC');
SELECT add_compression_policy('twitch_hype_train_events', INTERVAL '7 days', if_not_exists => TRUE);

CREATE TABLE IF NOT EXISTS twitch_ad_break_events (
    id               BIGSERIAL PRIMARY KEY,
    session_id       BIGINT,
    twitch_user_id   TEXT NOT NULL,
    duration_seconds INTEGER,
    is_automatic     BOOLEAN DEFAULT FALSE,
    started_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
SELECT create_hypertable('twitch_ad_break_events', 'started_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
ALTER TABLE twitch_ad_break_events SET (timescaledb.compress, timescaledb.compress_segmentby = 'twitch_user_id', timescaledb.compress_orderby = 'started_at DESC');
SELECT add_compression_policy('twitch_ad_break_events', INTERVAL '7 days', if_not_exists => TRUE);

CREATE TABLE IF NOT EXISTS twitch_ban_events (
    id               BIGSERIAL PRIMARY KEY,
    session_id       BIGINT,
    twitch_user_id   TEXT NOT NULL,
    event_type       TEXT NOT NULL,
    target_login     TEXT,
    target_id        TEXT,
    moderator_login  TEXT,
    reason           TEXT,
    ends_at          TIMESTAMPTZ,
    received_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
SELECT create_hypertable('twitch_ban_events', 'received_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
ALTER TABLE twitch_ban_events SET (timescaledb.compress, timescaledb.compress_segmentby = 'twitch_user_id', timescaledb.compress_orderby = 'received_at DESC');
SELECT add_compression_policy('twitch_ban_events', INTERVAL '7 days', if_not_exists => TRUE);

CREATE TABLE IF NOT EXISTS twitch_shoutout_events (
    id                      BIGSERIAL PRIMARY KEY,
    twitch_user_id          TEXT NOT NULL,
    direction               TEXT NOT NULL,
    other_broadcaster_id    TEXT,
    other_broadcaster_login TEXT,
    moderator_login         TEXT,
    viewer_count            INTEGER DEFAULT 0,
    received_at             TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
SELECT create_hypertable('twitch_shoutout_events', 'received_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
ALTER TABLE twitch_shoutout_events SET (timescaledb.compress, timescaledb.compress_segmentby = 'twitch_user_id', timescaledb.compress_orderby = 'received_at DESC');
SELECT add_compression_policy('twitch_shoutout_events', INTERVAL '7 days', if_not_exists => TRUE);

CREATE TABLE IF NOT EXISTS twitch_channel_updates (
    id              BIGSERIAL PRIMARY KEY,
    twitch_user_id  TEXT NOT NULL,
    title           TEXT,
    game_name       TEXT,
    language        TEXT,
    recorded_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
SELECT create_hypertable('twitch_channel_updates', 'recorded_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
ALTER TABLE twitch_channel_updates SET (timescaledb.compress, timescaledb.compress_segmentby = 'twitch_user_id', timescaledb.compress_orderby = 'recorded_at DESC');
SELECT add_compression_policy('twitch_channel_updates', INTERVAL '7 days', if_not_exists => TRUE);

CREATE TABLE IF NOT EXISTS twitch_raid_history (
    id                       BIGSERIAL NOT NULL,
    from_broadcaster_id      TEXT NOT NULL,
    from_broadcaster_login   TEXT NOT NULL,
    to_broadcaster_id        TEXT NOT NULL,
    to_broadcaster_login     TEXT NOT NULL,
    viewer_count             INTEGER DEFAULT 0,
    stream_duration_sec      INTEGER,
    reason                   TEXT,
    executed_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    success                  BOOLEAN DEFAULT TRUE,
    error_message            TEXT,
    target_stream_started_at TIMESTAMPTZ,
    candidates_count         INTEGER DEFAULT 0
);
SELECT create_hypertable('twitch_raid_history', 'executed_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_raid_history_id_executed_at
    ON twitch_raid_history(id, executed_at);
ALTER TABLE twitch_raid_history SET (timescaledb.compress, timescaledb.compress_segmentby = 'from_broadcaster_login', timescaledb.compress_orderby = 'executed_at DESC');
SELECT add_compression_policy('twitch_raid_history', INTERVAL '7 days', if_not_exists => TRUE);

CREATE TABLE IF NOT EXISTS twitch_raid_arrival_tracking (
    id                        BIGSERIAL PRIMARY KEY,
    detected_at               TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_signal_at            TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    from_broadcaster_id       TEXT,
    from_broadcaster_login    TEXT NOT NULL,
    to_broadcaster_id         TEXT NOT NULL,
    to_broadcaster_login      TEXT NOT NULL,
    viewer_count              INTEGER NOT NULL DEFAULT 0,
    classification            TEXT NOT NULL,
    confirmation_signals      TEXT NOT NULL DEFAULT '',
    primary_signal            TEXT,
    correlation_status        TEXT,
    correlation_detail        TEXT,
    source_resolution         TEXT,
    raid_history_id           BIGINT,
    raid_history_executed_at  TIMESTAMPTZ,
    unraid_seen               BOOLEAN NOT NULL DEFAULT FALSE,
    last_unraid_at            TIMESTAMPTZ
);
SELECT create_hypertable('twitch_raid_arrival_tracking', 'detected_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
ALTER TABLE twitch_raid_arrival_tracking SET (timescaledb.compress, timescaledb.compress_segmentby = 'to_broadcaster_id', timescaledb.compress_orderby = 'detected_at DESC');
SELECT add_compression_policy('twitch_raid_arrival_tracking', INTERVAL '7 days', if_not_exists => TRUE);
CREATE INDEX IF NOT EXISTS idx_twitch_raid_arrival_tracking_target
    ON twitch_raid_arrival_tracking(to_broadcaster_id, detected_at DESC);
CREATE INDEX IF NOT EXISTS idx_twitch_raid_arrival_tracking_source
    ON twitch_raid_arrival_tracking(from_broadcaster_login, detected_at DESC);
CREATE INDEX IF NOT EXISTS idx_twitch_raid_arrival_tracking_history_ref
    ON twitch_raid_arrival_tracking(raid_history_id, raid_history_executed_at);

CREATE TABLE IF NOT EXISTS twitch_confirmed_external_recruitment_raids (
    id                     BIGSERIAL PRIMARY KEY,
    raid_flow_id           TEXT UNIQUE,
    from_broadcaster_id    TEXT,
    from_broadcaster_login TEXT NOT NULL,
    to_broadcaster_id      TEXT NOT NULL,
    to_broadcaster_login   TEXT NOT NULL,
    viewer_count           INTEGER DEFAULT 0,
    confirmation_signal    TEXT,
    confirmed_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_confirmed_external_recruitment_raids_target
    ON twitch_confirmed_external_recruitment_raids(to_broadcaster_id);

CREATE TABLE IF NOT EXISTS twitch_external_recruitment_blacklist_pending (
    target_id            TEXT PRIMARY KEY,
    target_login         TEXT NOT NULL,
    confirmed_raid_count INTEGER NOT NULL,
    threshold_reached_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    blacklist_after      TIMESTAMPTZ NOT NULL,
    last_raid_flow_id    TEXT,
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_external_recruitment_blacklist_pending_due
    ON twitch_external_recruitment_blacklist_pending(blacklist_after);

CREATE TABLE IF NOT EXISTS twitch_external_bot_ban_check_pending (
    target_id    TEXT PRIMARY KEY,
    target_login TEXT NOT NULL,
    source       TEXT NOT NULL,
    run_after    TIMESTAMPTZ NOT NULL,
    scheduled_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_external_bot_ban_check_pending_due
    ON twitch_external_bot_ban_check_pending(run_after);

-- ========= Snapshots =========
CREATE TABLE IF NOT EXISTS twitch_subscriptions_snapshot (
    id             BIGSERIAL PRIMARY KEY,
    twitch_user_id TEXT NOT NULL,
    twitch_login   TEXT,
    total          INTEGER DEFAULT 0,
    tier1          INTEGER DEFAULT 0,
    tier2          INTEGER DEFAULT 0,
    tier3          INTEGER DEFAULT 0,
    points         INTEGER DEFAULT 0,
    snapshot_at    TIMESTAMPTZ DEFAULT NOW()
);
SELECT create_hypertable('twitch_subscriptions_snapshot', 'snapshot_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
ALTER TABLE twitch_subscriptions_snapshot SET (timescaledb.compress, timescaledb.compress_segmentby = 'twitch_user_id', timescaledb.compress_orderby = 'snapshot_at DESC');
SELECT add_compression_policy('twitch_subscriptions_snapshot', INTERVAL '7 days', if_not_exists => TRUE);

CREATE TABLE IF NOT EXISTS twitch_eventsub_capacity_snapshot (
    id                  BIGSERIAL PRIMARY KEY,
    ts_utc              TIMESTAMPTZ DEFAULT NOW(),
    trigger_reason      TEXT,
    listener_count      INTEGER DEFAULT 0,
    ready_listeners     INTEGER DEFAULT 0,
    failed_listeners    INTEGER DEFAULT 0,
    used_slots          INTEGER DEFAULT 0,
    total_slots         INTEGER DEFAULT 0,
    headroom_slots      INTEGER DEFAULT 0,
    listeners_at_limit  INTEGER DEFAULT 0,
    utilization_pct     DOUBLE PRECISION DEFAULT 0,
    listeners_json      TEXT
);
SELECT create_hypertable('twitch_eventsub_capacity_snapshot', 'ts_utc', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
ALTER TABLE twitch_eventsub_capacity_snapshot SET (timescaledb.compress, timescaledb.compress_segmentby = 'trigger_reason', timescaledb.compress_orderby = 'ts_utc DESC');
SELECT add_compression_policy('twitch_eventsub_capacity_snapshot', INTERVAL '7 days', if_not_exists => TRUE);

CREATE TABLE IF NOT EXISTS eventsub_guard_state (
    kind       TEXT NOT NULL,
    guard_key  TEXT NOT NULL,
    expires_at DOUBLE PRECISION NOT NULL,
    updated_at DOUBLE PRECISION NOT NULL,
    PRIMARY KEY (kind, guard_key)
);
CREATE INDEX IF NOT EXISTS idx_eventsub_guard_state_expiry
    ON eventsub_guard_state(expires_at);

CREATE TABLE IF NOT EXISTS twitch_eventsub_bridge_outbox (
    message_id      TEXT PRIMARY KEY,
    sub_type        TEXT NOT NULL,
    payload_json    TEXT NOT NULL,
    queued_at       DOUBLE PRECISION NOT NULL,
    next_attempt_at DOUBLE PRECISION NOT NULL,
    attempt_count   INTEGER NOT NULL DEFAULT 0,
    last_error      TEXT
);
CREATE INDEX IF NOT EXISTS idx_twitch_eventsub_bridge_outbox_due
    ON twitch_eventsub_bridge_outbox(next_attempt_at, queued_at);

CREATE TABLE IF NOT EXISTS twitch_eventsub_bridge_dead_letter (
    message_id        TEXT PRIMARY KEY,
    sub_type          TEXT NOT NULL,
    payload_json      TEXT NOT NULL,
    queued_at         DOUBLE PRECISION NOT NULL,
    dead_lettered_at  DOUBLE PRECISION NOT NULL,
    attempt_count     INTEGER NOT NULL,
    last_error        TEXT
);

CREATE INDEX IF NOT EXISTS idx_twitch_eventsub_bridge_dead_lettered_at
    ON twitch_eventsub_bridge_dead_letter(dead_lettered_at);

CREATE TABLE IF NOT EXISTS twitch_eventsub_processing_inbox (
    work_id          TEXT PRIMARY KEY,
    work_type        TEXT NOT NULL,
    message_id       TEXT,
    payload_json     TEXT NOT NULL,
    queued_at        DOUBLE PRECISION NOT NULL,
    next_attempt_at  DOUBLE PRECISION NOT NULL,
    attempt_count    INTEGER NOT NULL DEFAULT 0,
    last_error       TEXT
);

CREATE INDEX IF NOT EXISTS idx_twitch_eventsub_processing_inbox_due
    ON twitch_eventsub_processing_inbox(next_attempt_at, queued_at);

CREATE TABLE IF NOT EXISTS twitch_eventsub_processing_dead_letter (
    work_id           TEXT PRIMARY KEY,
    work_type         TEXT NOT NULL,
    message_id        TEXT,
    payload_json      TEXT NOT NULL,
    queued_at         DOUBLE PRECISION NOT NULL,
    dead_lettered_at  DOUBLE PRECISION NOT NULL,
    attempt_count     INTEGER NOT NULL,
    last_error        TEXT
);

CREATE INDEX IF NOT EXISTS idx_twitch_eventsub_processing_dead_lettered_at
    ON twitch_eventsub_processing_dead_letter(dead_lettered_at);

CREATE TABLE IF NOT EXISTS twitch_ads_schedule_snapshot (
    id                BIGSERIAL PRIMARY KEY,
    twitch_user_id    TEXT NOT NULL,
    twitch_login      TEXT,
    next_ad_at        TIMESTAMPTZ,
    last_ad_at        TIMESTAMPTZ,
    duration          INTEGER,
    preroll_free_time INTEGER,
    snooze_count      INTEGER,
    snooze_refresh_at TIMESTAMPTZ,
    snapshot_at       TIMESTAMPTZ DEFAULT NOW()
);
SELECT create_hypertable('twitch_ads_schedule_snapshot', 'snapshot_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
ALTER TABLE twitch_ads_schedule_snapshot SET (timescaledb.compress, timescaledb.compress_segmentby = 'twitch_user_id', timescaledb.compress_orderby = 'snapshot_at DESC');
SELECT add_compression_policy('twitch_ads_schedule_snapshot', INTERVAL '7 days', if_not_exists => TRUE);

-- ========= Social Media / Clips =========
CREATE TABLE IF NOT EXISTS twitch_clips_social_media (
    id                  BIGSERIAL PRIMARY KEY,
    clip_id             TEXT NOT NULL UNIQUE,
    clip_url            TEXT NOT NULL,
    clip_title          TEXT,
    clip_thumbnail_url  TEXT,
    streamer_login      TEXT NOT NULL,
    twitch_user_id      TEXT,
    created_at          TIMESTAMPTZ NOT NULL,
    duration_seconds    DOUBLE PRECISION,
    view_count          INTEGER DEFAULT 0,
    game_name           TEXT,
    status              TEXT DEFAULT 'pending',
    downloaded_at       TIMESTAMPTZ,
    local_file_path     TEXT,
    converted_file_path TEXT,
    uploaded_tiktok     BOOLEAN DEFAULT FALSE,
    uploaded_youtube    BOOLEAN DEFAULT FALSE,
    uploaded_instagram  BOOLEAN DEFAULT FALSE,
    tiktok_video_id     TEXT,
    youtube_video_id    TEXT,
    instagram_media_id  TEXT,
    tiktok_uploaded_at  TIMESTAMPTZ,
    youtube_uploaded_at TIMESTAMPTZ,
    instagram_uploaded_at TIMESTAMPTZ,
    custom_title        TEXT,
    custom_description  TEXT,
    hashtags            TEXT,
    music_track         TEXT,
    last_analytics_sync TIMESTAMPTZ
);
SELECT create_hypertable('twitch_clips_social_media', 'created_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '30 days');
ALTER TABLE twitch_clips_social_media SET (timescaledb.compress, timescaledb.compress_segmentby = 'streamer_login', timescaledb.compress_orderby = 'created_at DESC');
SELECT add_compression_policy('twitch_clips_social_media', INTERVAL '30 days', if_not_exists => TRUE);
CREATE INDEX IF NOT EXISTS idx_twitch_clips_social_media_streamer ON twitch_clips_social_media(streamer_login, created_at);
CREATE INDEX IF NOT EXISTS idx_twitch_clips_social_media_status ON twitch_clips_social_media(status);

CREATE TABLE IF NOT EXISTS twitch_clips_social_analytics (
    id                BIGSERIAL PRIMARY KEY,
    clip_id           BIGINT NOT NULL REFERENCES twitch_clips_social_media(id) ON DELETE CASCADE,
    platform          TEXT NOT NULL,
    platform_video_id TEXT,
    views             INTEGER DEFAULT 0,
    likes             INTEGER DEFAULT 0,
    comments          INTEGER DEFAULT 0,
    shares            INTEGER DEFAULT 0,
    saves             INTEGER DEFAULT 0,
    watch_time_avg    DOUBLE PRECISION,
    completion_rate   DOUBLE PRECISION,
    ctr               DOUBLE PRECISION,
    engagement_rate   DOUBLE PRECISION,
    external_clicks   INTEGER DEFAULT 0,
    new_followers     INTEGER DEFAULT 0,
    synced_at         TIMESTAMPTZ NOT NULL,
    posted_at         TIMESTAMPTZ
);
SELECT create_hypertable('twitch_clips_social_analytics', 'synced_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '30 days');
ALTER TABLE twitch_clips_social_analytics SET (timescaledb.compress, timescaledb.compress_segmentby = 'platform', timescaledb.compress_orderby = 'synced_at DESC');
SELECT add_compression_policy('twitch_clips_social_analytics', INTERVAL '30 days', if_not_exists => TRUE);
CREATE INDEX IF NOT EXISTS idx_twitch_clips_social_analytics_clip ON twitch_clips_social_analytics(clip_id, synced_at);
CREATE INDEX IF NOT EXISTS idx_twitch_clips_social_analytics_platform ON twitch_clips_social_analytics(platform, posted_at);

CREATE TABLE IF NOT EXISTS twitch_clips_upload_queue (
    id            BIGSERIAL PRIMARY KEY,
    clip_id       BIGINT NOT NULL REFERENCES twitch_clips_social_media(id) ON DELETE CASCADE,
    platform      TEXT NOT NULL,
    status        TEXT DEFAULT 'pending',
    priority      INTEGER DEFAULT 0,
    title         TEXT,
    description   TEXT,
    hashtags      TEXT,
    scheduled_at  TIMESTAMPTZ,
    attempts      INTEGER DEFAULT 0,
    last_error    TEXT,
    last_attempt_at TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at  TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_twitch_clips_upload_queue_status ON twitch_clips_upload_queue(status, priority DESC);

CREATE TABLE IF NOT EXISTS clip_templates_global (
    id                    BIGSERIAL PRIMARY KEY,
    template_name         TEXT NOT NULL UNIQUE,
    description_template  TEXT NOT NULL,
    hashtags              TEXT NOT NULL,
    category              TEXT,
    usage_count           INTEGER DEFAULT 0,
    created_at            TIMESTAMPTZ DEFAULT NOW(),
    created_by            TEXT
);

CREATE TABLE IF NOT EXISTS clip_templates_streamer (
    id                    BIGSERIAL PRIMARY KEY,
    streamer_login        TEXT NOT NULL,
    template_name         TEXT NOT NULL,
    description_template  TEXT NOT NULL,
    hashtags              TEXT NOT NULL,
    is_default            BOOLEAN DEFAULT FALSE,
    created_at            TIMESTAMPTZ DEFAULT NOW(),
    updated_at            TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE(streamer_login, template_name)
);
CREATE INDEX IF NOT EXISTS idx_clip_templates_streamer_login ON clip_templates_streamer(streamer_login);

CREATE TABLE IF NOT EXISTS clip_last_hashtags (
    streamer_login TEXT PRIMARY KEY,
    hashtags       TEXT NOT NULL,
    last_used_at   TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS clip_fetch_history (
    id              BIGSERIAL PRIMARY KEY,
    streamer_login  TEXT NOT NULL,
    fetched_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    clips_found     INTEGER DEFAULT 0,
    clips_new       INTEGER DEFAULT 0,
    fetch_duration_ms INTEGER,
    error           TEXT
);
CREATE INDEX IF NOT EXISTS idx_clip_fetch_history_streamer ON clip_fetch_history(streamer_login, fetched_at DESC);

-- ========= Link & Promo Tracking =========
CREATE TABLE IF NOT EXISTS twitch_link_clicks (
    id               BIGSERIAL PRIMARY KEY,
    clicked_at       TIMESTAMPTZ DEFAULT NOW(),
    streamer_login   TEXT NOT NULL,
    tracking_token   TEXT,
    discord_user_id  TEXT,
    discord_username TEXT,
    guild_id         TEXT,
    channel_id       TEXT,
    message_id       TEXT,
    ref_code         TEXT,
    source_hint      TEXT
);
SELECT create_hypertable('twitch_link_clicks', 'clicked_at', if_not_exists => TRUE, migrate_data => TRUE, chunk_time_interval => INTERVAL '7 days');
ALTER TABLE twitch_link_clicks SET (timescaledb.compress, timescaledb.compress_segmentby = 'streamer_login', timescaledb.compress_orderby = 'clicked_at DESC');
SELECT add_compression_policy('twitch_link_clicks', INTERVAL '7 days', if_not_exists => TRUE);
CREATE INDEX IF NOT EXISTS idx_twitch_link_clicks_streamer ON twitch_link_clicks(streamer_login);

-- ========= Go-Live Announcement Builder =========
CREATE TABLE IF NOT EXISTS twitch_live_announcement_configs (
    streamer_login           TEXT PRIMARY KEY,
    config_json              TEXT NOT NULL,
    allowed_editor_role_ids  TEXT,
    updated_at               TIMESTAMPTZ DEFAULT NOW(),
    updated_by               TEXT
);
CREATE INDEX IF NOT EXISTS idx_live_announce_configs_updated_at
    ON twitch_live_announcement_configs(updated_at);

-- ========= Global Chat Promo Announcement Mode =========
CREATE TABLE IF NOT EXISTS twitch_global_promo_modes (
    config_key     TEXT PRIMARY KEY,
    mode           TEXT NOT NULL DEFAULT 'standard',
    custom_message TEXT,
    starts_at      TEXT,
    ends_at        TEXT,
    is_enabled     INTEGER NOT NULL DEFAULT 0,
    updated_at     TIMESTAMPTZ DEFAULT NOW(),
    updated_by     TEXT
);
CREATE INDEX IF NOT EXISTS idx_twitch_global_promo_modes_updated_at
    ON twitch_global_promo_modes(updated_at);

-- ========= Raid Retention Rollup (computed, not a hypertable) =========
CREATE TABLE IF NOT EXISTS twitch_raid_retention (
    raid_id                BIGINT NOT NULL,
    from_broadcaster_login TEXT NOT NULL,
    to_broadcaster_login   TEXT NOT NULL,
    viewer_count_sent      INTEGER NOT NULL,
    executed_at            TIMESTAMPTZ NOT NULL,
    target_session_id      BIGINT REFERENCES twitch_stream_sessions(id),
    chatters_at_plus5m     INTEGER,
    chatters_at_plus15m    INTEGER,
    chatters_at_plus30m    INTEGER,
    known_from_raider      INTEGER,
    new_to_target          INTEGER,
    new_chatters           INTEGER,
    computed_at            TIMESTAMPTZ DEFAULT NOW(),
    PRIMARY KEY (raid_id, executed_at),
    CONSTRAINT twitch_raid_retention_raid_history_ref_fkey
        FOREIGN KEY (raid_id, executed_at)
        REFERENCES twitch_raid_history(id, executed_at)
        ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_twitch_raid_retention_raid_id
    ON twitch_raid_retention(raid_id);

-- ========= Housekeeping =========
ANALYZE;
