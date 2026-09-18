-- Separate, read-only Twitch category collector. Apply as postgres, never
-- from the runtime. All timestamps are timestamptz; language is NOT geography.
CREATE TABLE category_collector_config (
    id integer PRIMARY KEY CHECK (id = 1),
    enabled boolean NOT NULL DEFAULT true,
    deadlock_game_id text NOT NULL DEFAULT '',
    discovery_interval_seconds integer NOT NULL DEFAULT 60 CHECK (discovery_interval_seconds BETWEEN 30 AND 3600),
    chat_enabled boolean NOT NULL DEFAULT true,
    roster_decay_seconds integer NOT NULL DEFAULT 180 CHECK (roster_decay_seconds BETWEEN 60 AND 900),
    retention_days integer NOT NULL DEFAULT 90 CHECK (retention_days BETWEEN 1 AND 90),
    vod_metadata_enabled boolean NOT NULL DEFAULT true,
    updated_at timestamptz NOT NULL DEFAULT now()
);
INSERT INTO category_collector_config(id) VALUES (1);

CREATE TABLE category_polls (
    poll_id bigserial PRIMARY KEY,
    started_at timestamptz NOT NULL,
    completed_at timestamptz,
    status text NOT NULL DEFAULT 'running' CHECK (status IN ('running','complete','incomplete','failed')),
    stream_count integer NOT NULL DEFAULT 0 CHECK (stream_count >= 0),
    viewer_total bigint NOT NULL DEFAULT 0 CHECK (viewer_total >= 0),
    page_count integer NOT NULL DEFAULT 0 CHECK (page_count >= 0),
    error_detail text
);
CREATE INDEX category_polls_started_at_idx ON category_polls(started_at DESC);

CREATE TABLE category_channels (
    user_id text PRIMARY KEY CHECK (user_id <> ''),
    login text NOT NULL,
    display_name text,
    description text,
    broadcaster_type text,
    broadcaster_language text,
    profile_created_at timestamptz,
    profile_image_url text,
    first_seen_at timestamptz NOT NULL DEFAULT now(),
    last_seen_live_at timestamptz,
    metadata_fetched_at timestamptz,
    videos_cursor text,
    clips_cursor text,
    videos_complete boolean NOT NULL DEFAULT false,
    clips_complete boolean NOT NULL DEFAULT false,
    videos_pages integer NOT NULL DEFAULT 0,
    clips_pages integer NOT NULL DEFAULT 0,
    metadata_error text
);

CREATE TABLE category_stream_snapshots (
    snapshot_id bigserial PRIMARY KEY,
    poll_id bigint NOT NULL REFERENCES category_polls(poll_id),
    snapshot_at timestamptz NOT NULL,
    stream_id text NOT NULL CHECK (stream_id <> ''),
    user_id text NOT NULL CHECK (user_id <> ''),
    user_login text NOT NULL,
    viewer_count integer NOT NULL CHECK (viewer_count >= 0),
    title text,
    language text,
    started_at timestamptz NOT NULL,
    tags jsonb,
    thumbnail_url text,
    is_mature boolean NOT NULL DEFAULT false,
    UNIQUE (poll_id, stream_id)
);
CREATE INDEX category_snapshots_time_idx ON category_stream_snapshots(snapshot_at);
CREATE INDEX category_snapshots_user_time_idx ON category_stream_snapshots(user_id, snapshot_at);
CREATE INDEX category_snapshots_language_time_idx ON category_stream_snapshots(language, snapshot_at);

CREATE TABLE category_chat_messages (
    room_user_id text NOT NULL CHECK (room_user_id <> ''),
    message_id text NOT NULL CHECK (message_id <> ''),
    source_message_id text,
    source_room_id text,
    sent_at timestamptz NOT NULL,
    received_at timestamptz NOT NULL DEFAULT now(),
    chatter_user_id text NOT NULL CHECK (chatter_user_id <> ''),
    chatter_login text NOT NULL,
    message_text text NOT NULL,
    text_len integer NOT NULL CHECK (text_len >= 0),
    detected_lang text NOT NULL DEFAULT 'und',
    lang_confidence real,
    lang_method text NOT NULL DEFAULT 'unknown',
    emote_count integer NOT NULL DEFAULT 0 CHECK (emote_count >= 0),
    irc_tags jsonb NOT NULL DEFAULT '{}',
    redacted_at timestamptz,
    PRIMARY KEY(room_user_id, message_id)
);
CREATE INDEX category_chat_messages_sent_at_idx ON category_chat_messages(sent_at);
CREATE INDEX category_chat_messages_room_time_idx ON category_chat_messages(room_user_id, sent_at);
CREATE INDEX category_chat_messages_chatter_idx ON category_chat_messages(room_user_id, chatter_user_id, sent_at);

CREATE TABLE category_chat_rollup (
    hour_bucket timestamptz NOT NULL,
    room_user_id text NOT NULL,
    lang text NOT NULL,
    messages bigint NOT NULL,
    distinct_chatters bigint NOT NULL,
    total_len bigint NOT NULL,
    avg_len double precision,
    computed_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY(hour_bucket, room_user_id, lang)
);
CREATE INDEX category_rollup_hour_idx ON category_chat_rollup(hour_bucket);
-- Durable catch-up queue, populated in the same transaction as raw inserts.
CREATE TABLE category_chat_dirty_hours (hour_bucket timestamptz PRIMARY KEY);

-- List metadata only. No media files, downloads, audio or speech processing.
CREATE TABLE category_media_items (
    kind text NOT NULL CHECK (kind IN ('video','clip')),
    media_id text NOT NULL,
    user_id text NOT NULL,
    created_at timestamptz,
    metadata jsonb NOT NULL,
    fetched_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY(kind, media_id)
);
CREATE INDEX category_media_user_idx ON category_media_items(user_id, kind, created_at DESC);

CREATE TABLE category_collector_status (
    id integer PRIMARY KEY CHECK (id = 1),
    last_poll_started_at timestamptz,
    last_complete_poll_at timestamptz,
    last_error text,
    last_error_at timestamptz,
    roster_size integer NOT NULL DEFAULT 0,
    connected_shards integer NOT NULL DEFAULT 0,
    chat_privmsgs_dispatched bigint NOT NULL DEFAULT 0,
    chat_privmsgs_dropped bigint NOT NULL DEFAULT 0,
    chat_queue_dropped bigint NOT NULL DEFAULT 0,
    chat_control_events_dropped bigint NOT NULL DEFAULT 0,
    chat_commands_dropped bigint NOT NULL DEFAULT 0,
    chat_invalid_logins_rejected bigint NOT NULL DEFAULT 0,
    chat_reconnects bigint NOT NULL DEFAULT 0,
    chat_duplicates_skipped bigint NOT NULL DEFAULT 0,
    chat_redactions bigint NOT NULL DEFAULT 0,
    retention_deleted_total bigint NOT NULL DEFAULT 0,
    last_retention_run_at timestamptz,
    last_rollup_hour timestamptz,
    updated_at timestamptz NOT NULL DEFAULT now()
);
INSERT INTO category_collector_status(id) VALUES (1);
