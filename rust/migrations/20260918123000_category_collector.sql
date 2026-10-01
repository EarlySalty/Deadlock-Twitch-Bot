-- Execute as postgres. Additive, isolated category storage; no partner mutations.
CREATE TABLE IF NOT EXISTS category_channels (
    user_id text PRIMARY KEY,
    login text NOT NULL,
    display_name text NOT NULL DEFAULT '',
    created_at timestamptz,
    broadcaster_type text NOT NULL DEFAULT '',
    description text NOT NULL DEFAULT '',
    profile_image_url text NOT NULL DEFAULT '',
    first_seen timestamptz NOT NULL DEFAULT now(),
    last_seen timestamptz NOT NULL DEFAULT now(),
    broadcaster_language text NOT NULL DEFAULT 'und',
    profile_checked_at timestamptz,
    followers_total bigint CHECK (followers_total >= 0),
    followers_checked_at timestamptz,
    followers_http_status integer
);
CREATE INDEX IF NOT EXISTS category_channels_last_seen ON category_channels(last_seen DESC);

CREATE TABLE IF NOT EXISTS category_collection_runs (
    snapshot_at timestamptz PRIMARY KEY,
    completed_at timestamptz NOT NULL,
    streams integer NOT NULL CHECK (streams >= 0),
    viewers bigint NOT NULL CHECK (viewers >= 0),
    poll_seconds integer NOT NULL CHECK (poll_seconds >= 60)
);
CREATE TABLE IF NOT EXISTS category_stream_snapshots (
    snapshot_at timestamptz NOT NULL REFERENCES category_collection_runs(snapshot_at) ON DELETE CASCADE,
    stream_id text NOT NULL,
    user_id text NOT NULL,
    user_login text NOT NULL,
    viewer_count bigint NOT NULL CHECK (viewer_count >= 0),
    title text NOT NULL,
    language text NOT NULL,
    started_at timestamptz NOT NULL,
    tags text[] NOT NULL DEFAULT '{}',
    thumbnail_url text NOT NULL DEFAULT '',
    is_mature boolean NOT NULL,
    sample_seconds double precision NOT NULL CHECK (sample_seconds >= 0 AND sample_seconds <= 300),
    PRIMARY KEY (snapshot_at, stream_id)
);
CREATE INDEX IF NOT EXISTS category_snapshots_channel_time ON category_stream_snapshots(user_id, snapshot_at DESC);
CREATE INDEX IF NOT EXISTS category_snapshots_language_time ON category_stream_snapshots(language, snapshot_at DESC);

CREATE TABLE IF NOT EXISTS category_chat_messages (
    sent_at timestamptz NOT NULL,
    received_at timestamptz NOT NULL,
    message_id text NOT NULL,
    room_user_id text NOT NULL,
    chatter_user_id text NOT NULL,
    chatter_login text NOT NULL,
    message_text text NOT NULL,
    message_len integer NOT NULL CHECK (message_len >= 0),
    detected_lang text NOT NULL,
    language_confidence double precision NOT NULL CHECK (language_confidence BETWEEN 0 AND 1),
    stream_language text NOT NULL,
    emote_count integer NOT NULL DEFAULT 0 CHECK (emote_count >= 0),
    shared_chat_copy boolean NOT NULL DEFAULT false,
    tags jsonb NOT NULL DEFAULT '{}',
    PRIMARY KEY (sent_at, room_user_id, message_id)
) PARTITION BY RANGE(sent_at);
CREATE INDEX IF NOT EXISTS category_chat_room_message ON category_chat_messages(room_user_id, message_id);
CREATE INDEX IF NOT EXISTS category_chat_room_time ON category_chat_messages(room_user_id, sent_at);

CREATE TABLE IF NOT EXISTS category_chat_dirty (
    hour_at timestamptz NOT NULL,
    room_user_id text NOT NULL,
    language text NOT NULL,
    PRIMARY KEY(hour_at, room_user_id, language)
);
CREATE TABLE IF NOT EXISTS category_chat_rollup (
    hour_at timestamptz NOT NULL,
    room_user_id text NOT NULL,
    language text NOT NULL,
    messages bigint NOT NULL,
    distinct_chatter bigint NOT NULL,
    total_chars bigint NOT NULL,
    avg_len double precision NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY(hour_at, room_user_id, language)
);
CREATE INDEX IF NOT EXISTS category_rollup_language_time ON category_chat_rollup(language, hour_at DESC);
CREATE TABLE IF NOT EXISTS category_collector_status (
    singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton),
    heartbeat_at timestamptz NOT NULL DEFAULT now(),
    details jsonb NOT NULL DEFAULT '{}'
);

-- Media metadata only. This table never contains video/audio bytes.
CREATE TABLE IF NOT EXISTS category_media (
    kind text NOT NULL CHECK(kind IN ('video','clip')),
    media_id text NOT NULL,
    user_id text NOT NULL,
    metadata jsonb NOT NULL,
    category_verified boolean NOT NULL DEFAULT false,
    first_seen timestamptz NOT NULL DEFAULT now(),
    last_seen timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY(kind,media_id)
);
CREATE INDEX IF NOT EXISTS category_media_channel ON category_media(user_id,kind);
CREATE TABLE IF NOT EXISTS category_media_jobs (
    user_id text NOT NULL,
    kind text NOT NULL CHECK(kind IN ('video','clip')),
    cursor text,
    window_start timestamptz NOT NULL DEFAULT now() - interval '7 days',
    window_end timestamptz NOT NULL DEFAULT now(),
    next_at timestamptz NOT NULL DEFAULT now(),
    pages integer NOT NULL DEFAULT 0,
    last_error text,
    PRIMARY KEY(user_id,kind)
);

-- The service can prepare only these fixed UTC day partitions, not arbitrary DDL.
CREATE OR REPLACE FUNCTION category_prepare_partitions() RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public AS $$
DECLARE d date; n text;
BEGIN
    FOR d IN SELECT ((now() AT TIME ZONE 'UTC')::date + x) FROM generate_series(-1,1) AS x LOOP
        n := 'category_chat_messages_p' || to_char(d,'YYYYMMDD');
        EXECUTE format('CREATE TABLE IF NOT EXISTS public.%I PARTITION OF public.category_chat_messages FOR VALUES FROM (%L) TO (%L)',
            n, d::text || ' 00:00:00+00', (d+1)::text || ' 00:00:00+00');
    END LOOP;
END $$;
REVOKE ALL ON FUNCTION category_prepare_partitions() FROM PUBLIC;
SELECT category_prepare_partitions();

-- Reclaims actual disk blocks by dropping closed day partitions. Never prune
-- a partition with unprocessed rollups. Pressure shortening is explicit.
CREATE OR REPLACE FUNCTION category_prune_partitions(retention_days integer, max_bytes bigint)
RETURNS TABLE(dropped integer, pressure boolean, raw_bytes bigint)
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public AS $$
DECLARE child record; boundary timestamptz; bytes bigint;
BEGIN
    IF retention_days < 1 OR retention_days > 90 OR max_bytes < 104857600 THEN
        RAISE EXCEPTION 'invalid category retention limits';
    END IF;
    dropped := 0; pressure := false;
    SELECT COALESCE(sum(pg_total_relation_size(i.inhrelid)),0)::bigint INTO bytes
        FROM pg_inherits i WHERE i.inhparent = 'public.category_chat_messages'::regclass;
    FOR child IN
        SELECT c.relname, pg_total_relation_size(c.oid) AS size
        FROM pg_inherits i JOIN pg_class c ON c.oid=i.inhrelid
        WHERE i.inhparent='public.category_chat_messages'::regclass
          AND c.relname ~ '^category_chat_messages_p[0-9]{8}$'
        ORDER BY c.relname
    LOOP
        boundary := (to_date(right(child.relname,8),'YYYYMMDD') + 1)::timestamp AT TIME ZONE 'UTC';
        IF boundary > now() - make_interval(days => retention_days)
            AND NOT (bytes > max_bytes AND boundary <= date_trunc('day',now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC') THEN
            CONTINUE;
        END IF;
        IF EXISTS(SELECT 1 FROM public.category_chat_dirty WHERE hour_at >= boundary - interval '1 day' AND hour_at < boundary) THEN
            CONTINUE;
        END IF;
        pressure := pressure OR boundary > now() - make_interval(days => retention_days);
        EXECUTE format('DROP TABLE public.%I',child.relname);
        bytes := bytes - child.size;
        dropped := dropped + 1;
    END LOOP;
    raw_bytes := bytes;
    RETURN NEXT;
END $$;
REVOKE ALL ON FUNCTION category_prune_partitions(integer,bigint) FROM PUBLIC;

-- Existing runtime roles. New dedicated roles receive these same explicit grants
-- during deployment; no ALTER DEFAULT PRIVILEGES on unrelated application data.
DO $$
DECLARE role_name text;
BEGIN
    FOREACH role_name IN ARRAY ARRAY['twitchbot','twitchdash','twitchcollector'] LOOP
        IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname=role_name) THEN
            EXECUTE format('GRANT SELECT ON category_channels, category_collection_runs, category_stream_snapshots, category_chat_rollup, category_collector_status, category_media TO %I', role_name);
            IF role_name <> 'twitchdash' THEN
                EXECUTE format('GRANT SELECT,INSERT,UPDATE,DELETE ON category_channels, category_collection_runs, category_stream_snapshots, category_chat_messages, category_chat_dirty, category_chat_rollup, category_collector_status, category_media, category_media_jobs TO %I',role_name);
                EXECUTE format('GRANT EXECUTE ON FUNCTION category_prepare_partitions(), category_prune_partitions(integer,bigint) TO %I',role_name);
            END IF;
        END IF;
    END LOOP;
END $$;
