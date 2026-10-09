CREATE TABLE category_snapshot_versions (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    fingerprint text NOT NULL,
    valid_from timestamptz NOT NULL,
    stream_id text NOT NULL,
    user_id text NOT NULL,
    user_login text NOT NULL,
    title text NOT NULL,
    language text NOT NULL,
    started_at timestamptz NOT NULL,
    tags text[] NOT NULL,
    thumbnail_url text NOT NULL,
    is_mature boolean NOT NULL
);
CREATE INDEX category_snapshot_version_lookup ON category_snapshot_versions(fingerprint);
CREATE TABLE category_snapshot_samples (
    snapshot_at timestamptz NOT NULL REFERENCES category_collection_runs(snapshot_at),
    stream_id text NOT NULL,
    version_id bigint NOT NULL REFERENCES category_snapshot_versions(id),
    viewer_count bigint NOT NULL CHECK(viewer_count >= 0),
    sample_seconds double precision NOT NULL CHECK(sample_seconds BETWEEN 0 AND 300),
    PRIMARY KEY(snapshot_at,stream_id)
);
CREATE INDEX category_sample_version ON category_snapshot_samples(version_id,snapshot_at DESC);
CREATE INDEX category_version_channel ON category_snapshot_versions(user_id);
CREATE VIEW category_snapshots_normalized AS
SELECT s.snapshot_at,s.stream_id,v.user_id,v.user_login,s.viewer_count,v.title,v.language,
       v.started_at,v.tags,v.thumbnail_url,v.is_mature,s.sample_seconds
FROM category_snapshot_samples s JOIN category_snapshot_versions v ON v.id=s.version_id;
CREATE VIEW category_snapshots_read AS
SELECT * FROM category_snapshots_normalized
UNION ALL
SELECT o.* FROM category_stream_snapshots o WHERE NOT EXISTS(
    SELECT FROM category_snapshot_samples s WHERE (s.snapshot_at,s.stream_id)=(o.snapshot_at,o.stream_id));
CREATE TABLE category_snapshot_metrics (
    snapshot_at timestamptz NOT NULL,
    stream_id text NOT NULL,
    user_id text NOT NULL,
    user_login text NOT NULL,
    language text NOT NULL,
    started_at timestamptz NOT NULL,
    viewer_count bigint NOT NULL,
    sample_seconds double precision NOT NULL,
    PRIMARY KEY(snapshot_at,stream_id)
);
CREATE INDEX category_metric_channel_time ON category_snapshot_metrics(user_id,snapshot_at DESC);
CREATE VIEW category_snapshot_metric_source AS
SELECT snapshot_at,stream_id,user_id,user_login,language,started_at,viewer_count,sample_seconds
FROM category_snapshots_read
UNION ALL
SELECT m.* FROM category_snapshot_metrics m WHERE NOT EXISTS(
    SELECT FROM category_snapshot_samples s WHERE (s.snapshot_at,s.stream_id)=(m.snapshot_at,m.stream_id));
CREATE TABLE category_normalization_proofs (
    day date PRIMARY KEY,
    rows bigint NOT NULL,
    checksum text NOT NULL,
    proved_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE category_storage_state (
    singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton),
    normalized boolean NOT NULL DEFAULT false,
    writer_snapshot_at timestamptz,
    chat_metrics_ready boolean NOT NULL DEFAULT false,
    removal_authorized boolean NOT NULL DEFAULT false,
    last_alert_at timestamptz,
    failures bigint NOT NULL DEFAULT 0
);
INSERT INTO category_storage_state(singleton) VALUES(true);
CREATE FUNCTION category_snapshot_put(at timestamptz, sid text, uid text, login text, viewers bigint,
    heading text, lang text, started timestamptz, labels text[], thumbnail text, mature boolean, seconds double precision)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE fp text; vid bigint; old record;
BEGIN
    fp := encode(sha256(convert_to(jsonb_build_array(sid,uid,login,heading,lang,
        encode(timestamptz_send(started),'hex'),labels::text,thumbnail,mature)::text,'UTF8')),'hex');
    PERFORM pg_advisory_xact_lock(hashtextextended('category-version:'||fp,0));
    SELECT id INTO vid FROM public.category_snapshot_versions v
    WHERE v.fingerprint=fp AND (v.stream_id,v.user_id,v.user_login,v.title,v.language,v.started_at,v.tags,v.thumbnail_url,v.is_mature)
        IS NOT DISTINCT FROM (sid,uid,login,heading,lang,started,labels,thumbnail,mature) LIMIT 1 FOR KEY SHARE;
    IF vid IS NULL THEN
        INSERT INTO public.category_snapshot_versions(fingerprint,valid_from,stream_id,user_id,user_login,title,language,started_at,tags,thumbnail_url,is_mature)
        VALUES(fp,at,sid,uid,login,heading,lang,started,labels,thumbnail,mature) RETURNING id INTO vid;
    END IF;
    UPDATE public.category_snapshot_versions SET valid_from=least(valid_from,at) WHERE id=vid AND valid_from>at;
    IF EXISTS(SELECT FROM public.category_snapshot_metrics m WHERE m.snapshot_at=at AND m.stream_id=sid
        AND (m.user_id,m.user_login,m.language,m.started_at,m.viewer_count,float8send(m.sample_seconds))
            IS DISTINCT FROM (uid,login,lang,started,viewers,float8send(seconds))) THEN
        RAISE EXCEPTION 'archived snapshot metric conflict';
    END IF;
    INSERT INTO public.category_snapshot_samples VALUES(at,sid,vid,viewers,seconds) ON CONFLICT DO NOTHING;
    SELECT * INTO old FROM public.category_snapshot_samples WHERE snapshot_at=at AND stream_id=sid;
    IF old.version_id<>vid OR old.viewer_count<>viewers OR float8send(old.sample_seconds)<>float8send(seconds) THEN
        RAISE EXCEPTION 'snapshot key has conflicting content';
    END IF;
    DELETE FROM public.category_snapshot_metrics WHERE snapshot_at=at AND stream_id=sid;
END $$;
REVOKE ALL ON FUNCTION category_snapshot_put(timestamptz,text,text,text,bigint,text,text,timestamptz,text[],text,boolean,double precision) FROM PUBLIC;
CREATE FUNCTION category_snapshot_capture() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
    PERFORM public.category_snapshot_put(NEW.snapshot_at,NEW.stream_id,NEW.user_id,NEW.user_login,NEW.viewer_count,
        NEW.title,NEW.language,NEW.started_at,NEW.tags,NEW.thumbnail_url,NEW.is_mature,NEW.sample_seconds);
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION category_snapshot_capture() FROM PUBLIC;
CREATE TRIGGER category_snapshot_capture AFTER INSERT ON category_stream_snapshots
FOR EACH ROW EXECUTE FUNCTION category_snapshot_capture();

CREATE TABLE category_chat_metrics (
    sent_at timestamptz NOT NULL,
    room_user_id text NOT NULL,
    message_id text NOT NULL,
    chatter_user_id text NOT NULL,
    detected_lang text NOT NULL,
    message_len integer NOT NULL,
    shared_chat_copy boolean NOT NULL,
    source_room_id text,
    source_id text,
    PRIMARY KEY(sent_at,room_user_id,message_id)
);
CREATE INDEX category_chat_metric_target ON category_chat_metrics(room_user_id,message_id);
CREATE INDEX category_chat_metric_source_key ON category_chat_metrics(source_room_id,source_id) WHERE source_room_id IS NOT NULL;
CREATE FUNCTION category_chat_metric_capture() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
    INSERT INTO public.category_chat_metrics VALUES(NEW.sent_at,NEW.room_user_id,NEW.message_id,NEW.chatter_user_id,
        NEW.detected_lang,NEW.message_len,NEW.shared_chat_copy,NEW.tags->>'source-room-id',NEW.tags->>'source-id')
    ON CONFLICT DO NOTHING;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION category_chat_metric_capture() FROM PUBLIC;
CREATE TRIGGER category_chat_metric_capture AFTER INSERT ON category_chat_messages
FOR EACH ROW EXECUTE FUNCTION category_chat_metric_capture();
CREATE VIEW category_chat_metric_source AS
SELECT * FROM category_chat_metrics
UNION ALL
SELECT m.sent_at,m.room_user_id,m.message_id,m.chatter_user_id,m.detected_lang,m.message_len,
       m.shared_chat_copy,m.tags->>'source-room-id',m.tags->>'source-id'
FROM category_chat_messages m WHERE NOT EXISTS(SELECT FROM category_chat_metrics k
    WHERE (k.sent_at,k.room_user_id,k.message_id)=(m.sent_at,m.room_user_id,m.message_id));

CREATE FUNCTION category_snapshot_wire(s category_snapshots_normalized) RETURNS text
LANGUAGE sql IMMUTABLE SET search_path=pg_catalog,public AS $$
SELECT jsonb_build_object('snapshot_at',encode(timestamptz_send(s.snapshot_at),'hex'),'stream_id',s.stream_id,
    'user_id',s.user_id,'user_login',s.user_login,'viewer_count',s.viewer_count,'title',s.title,'language',s.language,
    'started_at',encode(timestamptz_send(s.started_at),'hex'),'tags',s.tags::text,'thumbnail_url',s.thumbnail_url,
    'is_mature',s.is_mature,'sample_seconds',encode(float8send(s.sample_seconds),'hex'))::text
$$;
CREATE FUNCTION category_chat_wire(m category_chat_messages) RETURNS text
LANGUAGE sql STABLE SET search_path=pg_catalog,public SET timezone='UTC' SET extra_float_digits=3 AS $$
SELECT (to_jsonb(m) || jsonb_build_object('tags',jsonb_strip_nulls(jsonb_build_object(
    'emotes',m.tags->'emotes','source-room-id',m.tags->'source-room-id','source-id',m.tags->'source-id'))))::text
$$;
REVOKE ALL ON FUNCTION category_snapshot_wire(category_snapshots_normalized),category_chat_wire(category_chat_messages) FROM PUBLIC;
CREATE TABLE category_archive_manifest (
    id uuid PRIMARY KEY,
    day date NOT NULL,
    kind text NOT NULL CHECK(kind IN ('chat','snapshots')),
    object_path text NOT NULL UNIQUE CHECK(object_path LIKE 'gdrive:category/%'),
    state text NOT NULL CHECK(state IN ('exporting','exported','verified','removed','restored')),
    rows bigint NOT NULL DEFAULT 0 CHECK(rows>=0),
    checksum text,
    cipher_checksum text,
    cipher_bytes bigint,
    key_id text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    verified_at timestamptz,
    removed_at timestamptz,
    compacted_at timestamptz,
    removal_after_at timestamptz,
    removal_after_room text,
    removal_after_item text,
    CHECK(state='exporting' OR (checksum IS NOT NULL AND cipher_checksum IS NOT NULL AND cipher_bytes>0))
);
CREATE TABLE category_archive_notifications (
    notification_day date PRIMARY KEY,
    content text NOT NULL,
    last_attempt_at timestamptz,
    notified_at timestamptz
);
CREATE INDEX category_archive_day ON category_archive_manifest(day,kind);
CREATE TABLE category_archive_members (
    manifest_id uuid NOT NULL REFERENCES category_archive_manifest(id),
    at timestamptz NOT NULL,
    room_user_id text NOT NULL,
    item_id text NOT NULL,
    checksum text NOT NULL,
    PRIMARY KEY(manifest_id,at,room_user_id,item_id)
);
CREATE INDEX category_archive_member_lookup ON category_archive_members(at,room_user_id,item_id);
CREATE FUNCTION category_archive_remove(mid uuid, batch integer) RETURNS bigint
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE m public.category_archive_manifest; removed bigint; mismatches bigint;
    members public.category_archive_members[]; last_member public.category_archive_members;
BEGIN
    IF batch<1 OR batch>1000 THEN RAISE EXCEPTION 'invalid archive batch'; END IF;
    SELECT * INTO STRICT m FROM public.category_archive_manifest WHERE id=mid FOR UPDATE;
    IF m.state NOT IN ('verified','removed') OR m.verified_at IS NULL THEN RAISE EXCEPTION 'archive not verified'; END IF;
    IF m.day >= (now() AT TIME ZONE 'UTC')::date THEN RAISE EXCEPTION 'archive day is open'; END IF;
    IF NOT (SELECT normalized AND chat_metrics_ready AND removal_authorized FROM public.category_storage_state WHERE singleton) THEN
        RAISE EXCEPTION 'storage transition or removal authorization incomplete';
    END IF;
    IF m.state='removed' THEN m.removal_after_at:=NULL; END IF;
    SELECT array_agg(k ORDER BY k.at,k.room_user_id,k.item_id) INTO members FROM (
        SELECT k.* FROM public.category_archive_members k WHERE k.manifest_id=mid
        AND (m.removal_after_at IS NULL OR (k.at,k.room_user_id,k.item_id)>(m.removal_after_at,m.removal_after_room,m.removal_after_item))
        ORDER BY k.at,k.room_user_id,k.item_id LIMIT batch) k;
    IF members IS NULL THEN
        DELETE FROM public.category_snapshot_versions v WHERE NOT EXISTS(SELECT FROM public.category_snapshot_samples s WHERE s.version_id=v.id);
        UPDATE public.category_archive_manifest SET state='removed',removed_at=now() WHERE id=mid;
        RETURN 0;
    END IF;
    PERFORM pg_advisory_xact_lock(782363991808);
    LOCK TABLE public.twitch_partners IN SHARE MODE;
    IF EXISTS(SELECT FROM public.twitch_streamers_partner_state p WHERE p.is_partner=1 AND nullif(btrim(p.twitch_user_id),'') IS NULL) THEN
        RAISE EXCEPTION 'active partner ID missing';
    END IF;
    IF m.kind='chat' THEN
        LOCK TABLE public.category_chat_messages IN SHARE ROW EXCLUSIVE MODE;
        SELECT count(*) INTO mismatches FROM unnest(members) k JOIN public.category_chat_messages c
            ON (c.sent_at,c.room_user_id,c.message_id)=(k.at,k.room_user_id,k.item_id)
            WHERE NOT EXISTS(SELECT FROM public.twitch_streamers_partner_state p WHERE p.is_partner=1 AND p.twitch_user_id=c.room_user_id)
            AND k.checksum<>encode(sha256(convert_to(public.category_chat_wire(c),'UTF8')),'hex');
        IF mismatches>0 THEN RAISE EXCEPTION 'archive content changed'; END IF;
        WITH gone AS (DELETE FROM public.category_chat_messages c USING unnest(members) k
            WHERE (c.sent_at,c.room_user_id,c.message_id)=(k.at,k.room_user_id,k.item_id)
            AND c.sent_at>=m.day::timestamp AT TIME ZONE 'UTC' AND c.sent_at<(m.day+1)::timestamp AT TIME ZONE 'UTC'
            AND NOT EXISTS(SELECT FROM public.twitch_streamers_partner_state p WHERE p.is_partner=1 AND p.twitch_user_id=c.room_user_id)
            RETURNING 1) SELECT count(*) INTO removed FROM gone;
    ELSE
        LOCK TABLE public.category_snapshot_samples IN SHARE ROW EXCLUSIVE MODE;
        SELECT count(*) INTO mismatches FROM unnest(members) k JOIN public.category_snapshots_normalized s
            ON (s.snapshot_at,s.user_id,s.stream_id)=(k.at,k.room_user_id,k.item_id)
            WHERE NOT EXISTS(SELECT FROM public.twitch_streamers_partner_state p WHERE p.is_partner=1 AND p.twitch_user_id=s.user_id)
            AND k.checksum<>encode(sha256(convert_to(public.category_snapshot_wire(s),'UTF8')),'hex');
        IF mismatches>0 THEN RAISE EXCEPTION 'archive content changed'; END IF;
        WITH eligible AS MATERIALIZED (SELECT s.* FROM public.category_snapshots_normalized s JOIN unnest(members) k
            ON (s.snapshot_at,s.user_id,s.stream_id)=(k.at,k.room_user_id,k.item_id)
            WHERE s.snapshot_at>=m.day::timestamp AT TIME ZONE 'UTC' AND s.snapshot_at<(m.day+1)::timestamp AT TIME ZONE 'UTC'
            AND NOT EXISTS(SELECT FROM public.twitch_streamers_partner_state p WHERE p.is_partner=1 AND p.twitch_user_id=s.user_id)), kept AS (
            INSERT INTO public.category_snapshot_metrics SELECT snapshot_at,stream_id,user_id,user_login,language,started_at,viewer_count,sample_seconds
            FROM eligible ON CONFLICT DO NOTHING), gone AS (
            DELETE FROM public.category_snapshot_samples c USING eligible e
            WHERE (c.snapshot_at,c.stream_id)=(e.snapshot_at,e.stream_id) RETURNING 1)
        SELECT count(*) INTO removed FROM gone;
    END IF;
    last_member:=members[array_length(members,1)];
    UPDATE public.category_archive_manifest SET state='verified',removal_after_at=last_member.at,
        compacted_at=CASE WHEN removed>0 THEN NULL ELSE compacted_at END,
        removal_after_room=last_member.room_user_id,removal_after_item=last_member.item_id WHERE id=mid;
    RETURN removed;
END $$;
REVOKE ALL ON FUNCTION category_archive_remove(uuid,integer) FROM PUBLIC;
CREATE FUNCTION category_archive_compact(mid uuid) RETURNS bigint
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public SET timezone='UTC' SET lock_timeout='5s' AS $$
DECLARE m public.category_archive_manifest; n text; replacement text; previous text; source oid; copied bigint; r text; changed boolean;
BEGIN
    SELECT * INTO STRICT m FROM public.category_archive_manifest WHERE id=mid FOR UPDATE;
    IF m.kind<>'chat' OR m.state<>'removed' OR m.verified_at IS NULL OR m.day >= (now() AT TIME ZONE 'UTC')::date THEN
        RAISE EXCEPTION 'chat archive removal incomplete';
    END IF;
    IF NOT (SELECT normalized AND chat_metrics_ready AND removal_authorized FROM public.category_storage_state WHERE singleton) THEN
        RAISE EXCEPTION 'storage transition or removal authorization incomplete';
    END IF;
    IF m.compacted_at IS NOT NULL THEN RETURN 0; END IF;
    PERFORM pg_advisory_xact_lock(782363991808);
    LOCK TABLE public.category_chat_messages IN ACCESS EXCLUSIVE MODE;
    n:='category_chat_messages_p'||to_char(m.day,'YYYYMMDD');
    source:=to_regclass(format('public.%I',n));
    IF source IS NULL THEN
        IF EXISTS(SELECT FROM public.category_chat_messages WHERE sent_at>=m.day::timestamptz AND sent_at<(m.day+1)::timestamptz) THEN
            RAISE EXCEPTION 'expected daily chat partition missing';
        END IF;
        UPDATE public.category_archive_manifest SET compacted_at=now() WHERE id=mid;
        RETURN 0;
    END IF;
    IF NOT EXISTS(SELECT FROM pg_inherits WHERE inhrelid=source AND inhparent='public.category_chat_messages'::regclass) THEN
        RAISE EXCEPTION 'expected daily chat partition missing';
    END IF;
    replacement:=n||'_replacement'; previous:=n||'_replaced';
    EXECUTE format('CREATE TABLE public.%I (LIKE public.%I INCLUDING ALL)',replacement,n);
    EXECUTE format('INSERT INTO public.%I SELECT * FROM public.%I',replacement,n);
    GET DIAGNOSTICS copied=ROW_COUNT;
    EXECUTE format('SELECT EXISTS((SELECT * FROM public.%I EXCEPT ALL SELECT * FROM public.%I) UNION ALL (SELECT * FROM public.%I EXCEPT ALL SELECT * FROM public.%I))',n,replacement,replacement,n) INTO STRICT changed;
    IF changed THEN RAISE EXCEPTION 'chat partition copy mismatch'; END IF;
    EXECUTE format('REVOKE ALL ON TABLE public.%I FROM PUBLIC',replacement);
    FOREACH r IN ARRAY ARRAY['twitchbot','twitchcollector','twitchcategoryarchive','twitchdash','twitchlegacy'] LOOP
        IF EXISTS(SELECT FROM pg_roles WHERE rolname=r) THEN EXECUTE format('REVOKE ALL ON TABLE public.%I FROM %I',replacement,r); END IF;
    END LOOP;
    EXECUTE format('ALTER TABLE public.category_chat_messages DETACH PARTITION public.%I',n);
    EXECUTE format('ALTER TABLE public.%I RENAME TO %I',n,previous);
    EXECUTE format('ALTER TABLE public.%I RENAME TO %I',replacement,n);
    EXECUTE format('ALTER TABLE public.category_chat_messages ATTACH PARTITION public.%I FOR VALUES FROM (%L) TO (%L)',n,m.day::text||' 00:00:00+00',(m.day+1)::text||' 00:00:00+00');
    EXECUTE format('DROP TABLE public.%I',previous);
    UPDATE public.category_archive_manifest SET compacted_at=now() WHERE id=mid;
    RETURN copied;
END $$;
REVOKE ALL ON FUNCTION category_archive_compact(uuid) FROM PUBLIC;
DO $$
BEGIN
    IF NOT EXISTS(SELECT FROM pg_roles WHERE rolname='twitchcategoryarchive') THEN CREATE ROLE twitchcategoryarchive NOLOGIN; END IF;
    GRANT EXECUTE ON FUNCTION category_archive_remove(uuid,integer),category_archive_compact(uuid) TO twitchcategoryarchive;
END $$;
DO $$
DECLARE r text; relation text;
BEGIN
    FOR relation IN SELECT c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
        WHERE n.nspname='public' AND (c.oid='public.category_stream_snapshots'::regclass
            OR c.oid IN (SELECT relid FROM pg_partition_tree('public.category_chat_messages'::regclass)))
    LOOP
        EXECUTE format('REVOKE UPDATE,DELETE,TRUNCATE ON TABLE public.%I FROM PUBLIC',relation);
        FOREACH r IN ARRAY ARRAY['twitchbot','twitchcollector','twitchcategoryarchive','twitchdash','twitchlegacy'] LOOP
            IF EXISTS(SELECT FROM pg_roles WHERE rolname=r) THEN
                EXECUTE format('REVOKE UPDATE,DELETE,TRUNCATE ON TABLE public.%I FROM %I',relation,r);
            END IF;
        END LOOP;
    END LOOP;
    FOREACH r IN ARRAY ARRAY['twitchbot','twitchcollector','twitchdash','twitchlegacy'] LOOP
        IF EXISTS(SELECT FROM pg_roles WHERE rolname=r) THEN
            EXECUTE format('REVOKE ALL ON FUNCTION category_archive_remove(uuid,integer),category_archive_compact(uuid) FROM %I',r);
        END IF;
    END LOOP;
    FOREACH relation IN ARRAY ARRAY['category_snapshot_versions','category_snapshot_samples','category_snapshots_normalized',
        'category_snapshots_read','category_snapshot_metrics','category_snapshot_metric_source','category_normalization_proofs',
        'category_chat_metrics','category_chat_metric_source','category_storage_state','category_archive_manifest',
        'category_archive_members','category_archive_notifications'] LOOP
        EXECUTE format('REVOKE ALL ON TABLE public.%I FROM PUBLIC',relation);
        FOREACH r IN ARRAY ARRAY['twitchbot','twitchcollector','twitchcategoryarchive','twitchdash','twitchlegacy'] LOOP
            IF EXISTS(SELECT FROM pg_roles WHERE rolname=r) THEN
                EXECUTE format('REVOKE ALL ON TABLE public.%I FROM %I',relation,r);
            END IF;
        END LOOP;
    END LOOP;
END $$;
DO $$
DECLARE r text;
BEGIN
    FOREACH r IN ARRAY ARRAY['twitchbot','twitchcollector','twitchcategoryarchive'] LOOP
        IF EXISTS(SELECT FROM pg_roles WHERE rolname=r) THEN
            EXECUTE format('GRANT SELECT ON category_snapshot_versions,category_snapshot_samples,category_snapshots_normalized,category_snapshots_read,category_snapshot_metrics,category_snapshot_metric_source,category_chat_metrics,category_chat_metric_source,category_storage_state,category_archive_manifest,category_archive_members TO %I',r);
            EXECUTE format('GRANT EXECUTE ON FUNCTION category_snapshot_put(timestamptz,text,text,text,bigint,text,text,timestamptz,text[],text,boolean,double precision),category_snapshot_wire(category_snapshots_normalized),category_chat_wire(category_chat_messages) TO %I',r);
            IF r IN ('twitchbot','twitchcollector') THEN
                EXECUTE format('GRANT UPDATE(writer_snapshot_at) ON category_storage_state TO %I',r);
            END IF;
        END IF;
    END LOOP;
    IF EXISTS(SELECT FROM pg_roles WHERE rolname='twitchbot') THEN
        GRANT SELECT(twitch_user_id,is_partner) ON twitch_streamers_partner_state TO twitchbot;
        GRANT INSERT,UPDATE ON category_archive_manifest TO twitchbot;
        GRANT INSERT,DELETE ON category_archive_members TO twitchbot;
        GRANT UPDATE(last_alert_at,failures,writer_snapshot_at) ON category_storage_state TO twitchbot;
        GRANT SELECT,INSERT,UPDATE ON category_archive_notifications TO twitchbot;
    END IF;
    IF EXISTS(SELECT FROM pg_roles WHERE rolname='twitchdash') THEN
        GRANT SELECT ON category_snapshot_metric_source TO twitchdash;
    END IF;
END $$;

CREATE OR REPLACE FUNCTION category_redact_chat_event_locked(room_id text, message_id text, user_id text, event_at timestamptz)
RETURNS bigint LANGUAGE sql SECURITY INVOKER SET search_path=pg_catalog,public AS $$
    WITH bounds AS MATERIALIZED (
        SELECT COALESCE((SELECT s.started_at FROM public.category_snapshot_metric_source AS s
            WHERE s.user_id=$1 AND s.snapshot_at <= $4 ORDER BY s.snapshot_at DESC LIMIT 1),$4) AS started_at,
            $4 AS ended_at
    ), notice AS (
        INSERT INTO public.category_chat_redactions(room_user_id,message_id)
        SELECT $1,$2
        WHERE nullif(btrim($1),'') IS NOT NULL AND nullif(btrim($2),'') IS NOT NULL
        ON CONFLICT DO NOTHING
    ), user_notice AS (
        INSERT INTO public.category_chat_user_redactions
            (room_user_id,chatter_user_id,started_at,ended_at)
        SELECT $1,$3,b.started_at,b.ended_at FROM bounds AS b
        WHERE nullif(btrim($1),'') IS NOT NULL AND nullif(btrim($3),'') IS NOT NULL
          AND $2 IS NULL AND b.started_at <= b.ended_at
        ON CONFLICT DO NOTHING
    ), removed AS (
        DELETE FROM public.category_chat_messages AS m
        WHERE nullif(btrim($1),'') IS NOT NULL AND (
            (nullif(btrim($2),'') IS NOT NULL AND (
                (m.room_user_id=$1 AND m.message_id=$2)
                OR (m.tags ? 'source-room-id' AND m.tags ? 'source-id'
                    AND m.tags->>'source-room-id'=$1 AND m.tags->>'source-id'=$2)
            )) OR (
                $2 IS NULL AND nullif(btrim($3),'') IS NOT NULL
                AND (m.room_user_id=$1 OR (m.tags ? 'source-room-id'
                    AND m.tags->>'source-room-id'=$1))
                AND m.chatter_user_id=$3
                AND m.sent_at >= (SELECT b.started_at FROM bounds AS b)
                AND m.sent_at <= (SELECT b.ended_at FROM bounds AS b)
            )
        ) RETURNING sent_at,room_user_id,detected_lang
    ), metric_removed AS (
        DELETE FROM public.category_chat_metrics m
        WHERE nullif(btrim($1),'') IS NOT NULL AND (
            (nullif(btrim($2),'') IS NOT NULL AND (
                (m.room_user_id=$1 AND m.message_id=$2) OR (m.source_room_id=$1 AND m.source_id=$2)
            )) OR ($2 IS NULL AND nullif(btrim($3),'') IS NOT NULL
                AND (m.room_user_id=$1 OR m.source_room_id=$1) AND m.chatter_user_id=$3
                AND m.sent_at >= (SELECT started_at FROM bounds) AND m.sent_at <= (SELECT ended_at FROM bounds)))
        RETURNING sent_at,room_user_id,detected_lang
    ), dirty AS (
        INSERT INTO public.category_chat_dirty
        SELECT DISTINCT date_trunc('hour',sent_at),room_user_id,detected_lang FROM (SELECT * FROM removed UNION SELECT * FROM metric_removed) x
        ON CONFLICT DO NOTHING
    ) SELECT count(*)::bigint FROM removed;
$$;

CREATE OR REPLACE FUNCTION category_lock_chat_rooms(room_ids text[]) RETURNS void
LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog AS $$
DECLARE room_id text;
BEGIN
    PERFORM pg_advisory_xact_lock_shared(782363991808);
    FOR room_id IN SELECT DISTINCT value FROM unnest(room_ids) AS ids(value)
        WHERE nullif(btrim(value),'') IS NOT NULL ORDER BY value
    LOOP PERFORM pg_advisory_xact_lock(hashtextextended('category-chat-room:' || room_id,0)); END LOOP;
END $$;
CREATE FUNCTION category_restore_chat(payload jsonb) RETURNS bigint
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE m public.category_chat_messages; inserted bigint;
BEGIN
    SELECT * INTO m FROM jsonb_populate_record(NULL::public.category_chat_messages,payload);
    PERFORM public.category_lock_chat_rooms(ARRAY[m.room_user_id,m.tags->>'source-room-id']);
    WITH rows AS (INSERT INTO public.category_chat_messages SELECT m.*
        WHERE NOT EXISTS(SELECT FROM public.category_chat_redactions r
            WHERE (r.room_user_id=m.room_user_id AND r.message_id=m.message_id)
                OR (r.room_user_id=m.tags->>'source-room-id' AND r.message_id=m.tags->>'source-id'))
        AND NOT EXISTS(SELECT FROM public.category_chat_user_redactions r
            WHERE r.chatter_user_id=m.chatter_user_id AND m.sent_at BETWEEN r.started_at AND r.ended_at
                AND (r.room_user_id=m.room_user_id OR r.room_user_id=m.tags->>'source-room-id'))
        ON CONFLICT DO NOTHING RETURNING sent_at,room_user_id,detected_lang), dirty AS (
        INSERT INTO public.category_chat_dirty SELECT DISTINCT date_trunc('hour',sent_at),room_user_id,detected_lang
        FROM rows ON CONFLICT DO NOTHING)
    SELECT count(*) INTO inserted FROM rows;
    RETURN inserted;
END $$;
REVOKE ALL ON FUNCTION category_restore_chat(jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION category_restore_chat(jsonb) TO twitchcategoryarchive;
GRANT USAGE ON SCHEMA public TO twitchcategoryarchive;
GRANT SELECT ON category_chat_messages,category_collector_config TO twitchcategoryarchive;
GRANT SELECT(twitch_user_id,is_partner) ON twitch_streamers_partner_state TO twitchcategoryarchive;
GRANT INSERT,UPDATE ON category_archive_manifest TO twitchcategoryarchive;
GRANT INSERT,DELETE ON category_archive_members TO twitchcategoryarchive;

CREATE OR REPLACE FUNCTION category_redact_chat_event(room_id text,message_id text,user_id text,event_at timestamptz)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public SET timezone='UTC' AS $$
BEGIN
    PERFORM public.category_lock_chat_rooms(ARRAY[room_id]);
    RETURN public.category_redact_chat_event_locked(room_id,message_id,user_id,event_at);
END $$;
CREATE FUNCTION category_compact_tags(tags jsonb) RETURNS jsonb
LANGUAGE sql IMMUTABLE SET search_path=pg_catalog AS $$
SELECT jsonb_strip_nulls(jsonb_build_object('emotes',tags->'emotes','source-room-id',tags->'source-room-id','source-id',tags->'source-id'))
$$;
