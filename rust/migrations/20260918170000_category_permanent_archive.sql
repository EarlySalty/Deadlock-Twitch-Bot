-- Verbindlicher Archivschutz: keine Alterslöschung und keine Kürzung bei Platzmangel.
-- Bereits gespeicherte Daten und angewandte Migrationen bleiben unverändert.
CREATE TABLE category_collector_config (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    enabled boolean NOT NULL DEFAULT true,
    poll_seconds integer NOT NULL DEFAULT 60 CHECK (poll_seconds BETWEEN 60 AND 300),
    raw_budget_bytes bigint NOT NULL DEFAULT 21474836480 CHECK (raw_budget_bytes >= 104857600),
    min_free_bytes bigint NOT NULL DEFAULT 10737418240 CHECK (min_free_bytes >= 1073741824),
    media_enabled boolean NOT NULL DEFAULT true,
    preserve_raw_data boolean NOT NULL DEFAULT true CHECK (preserve_raw_data),
    updated_at timestamptz NOT NULL DEFAULT now()
);
INSERT INTO category_collector_config(singleton) VALUES (true);
COMMENT ON TABLE category_collector_config IS
    'Sammlerverhalten in PostgreSQL. Speichergrenzen pausieren neue Erfassung, niemals Bestandsdaten löschen.';

-- Auch ein Aufruf aus einem alten Dienststand darf keine Partition mehr löschen.
-- Die historische Signatur bleibt kompatibel, liefert aber ausschließlich Messwerte.
CREATE OR REPLACE FUNCTION category_prune_partitions(retention_days integer, max_bytes bigint)
RETURNS TABLE(dropped integer, pressure boolean, raw_bytes bigint)
LANGUAGE sql STABLE SECURITY INVOKER SET search_path = pg_catalog, public AS $$
    SELECT 0, max_bytes > 0 AND bytes >= max_bytes, bytes
    FROM (SELECT COALESCE(sum(pg_total_relation_size(i.inhrelid)),0)::bigint AS bytes
          FROM pg_inherits i
          WHERE i.inhparent = 'public.category_chat_messages'::regclass) AS usage;
$$;
REVOKE ALL ON FUNCTION category_prune_partitions(integer,bigint) FROM PUBLIC;
COMMENT ON FUNCTION category_prune_partitions(integer,bigint) IS
    'Nur lesende Kompatibilität. Alters- und budgetbasierte Löschungen sind dauerhaft deaktiviert.';

-- Ausschließlich IDs, keine zweite Kopie eines entfernten Textes. Verhindert,
-- dass eine später zugestellte IRC-Wiederholung eine gezielt entfernte Zeile zurückbringt.
CREATE TABLE category_chat_redactions (
    room_user_id text NOT NULL CHECK (length(btrim(room_user_id)) > 0),
    message_id text NOT NULL CHECK (length(btrim(message_id)) > 0),
    observed_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY(room_user_id,message_id)
);

-- Der Dienst bekommt keine freie DELETE-Berechtigung auf Rohdaten.
-- Nur explizite Moderationsziele werden durch diese eng begrenzte Funktion bearbeitet.
-- Ein kanalweiter CLEARCHAT ohne Ziel ist ausdrücklich KEINE Archivlöschung.
CREATE FUNCTION category_redact_chat_event(room_id text, message_id text, user_id text)
RETURNS bigint LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
    WITH notice AS (
        INSERT INTO public.category_chat_redactions(room_user_id,message_id)
        SELECT $1,$2 WHERE nullif(btrim($1),'') IS NOT NULL AND nullif(btrim($2),'') IS NOT NULL
        ON CONFLICT DO NOTHING
    ), removed AS (
        DELETE FROM public.category_chat_messages AS m
        WHERE nullif(btrim($1),'') IS NOT NULL AND (
            (nullif(btrim($2),'') IS NOT NULL AND (
                (m.room_user_id=$1 AND m.message_id=$2)
                OR (m.tags->>'source-room-id'=$1 AND m.tags->>'source-id'=$2)
            )) OR (
                $2 IS NULL AND nullif(btrim($3),'') IS NOT NULL
                AND m.room_user_id=$1 AND m.chatter_user_id=$3
                AND m.sent_at >= COALESCE((SELECT s.started_at FROM public.category_stream_snapshots AS s
                    WHERE s.user_id=$1 ORDER BY s.snapshot_at DESC LIMIT 1),now())
                AND m.sent_at <= now()
            )
        ) RETURNING sent_at,room_user_id,detected_lang
    ), dirty AS (
        INSERT INTO public.category_chat_dirty
        SELECT DISTINCT date_trunc('hour',sent_at),room_user_id,detected_lang FROM removed
        ON CONFLICT DO NOTHING
    ) SELECT count(*)::bigint FROM removed;
$$;
REVOKE ALL ON FUNCTION category_redact_chat_event(text,text,text) FROM PUBLIC;

DO $$
DECLARE role_name text; child record;
BEGIN
    FOREACH role_name IN ARRAY ARRAY['twitchcollector','twitchdash'] LOOP
        IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname=role_name) THEN
            EXECUTE format('GRANT SELECT ON category_collector_config TO %I', role_name);
        END IF;
    END LOOP;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname='twitchcollector') THEN
        GRANT SELECT ON category_chat_redactions TO twitchcollector;
        GRANT EXECUTE ON FUNCTION category_redact_chat_event(text,text,text) TO twitchcollector;
        REVOKE UPDATE,DELETE,TRUNCATE ON category_chat_messages FROM twitchcollector;
        FOR child IN SELECT inhrelid::regclass AS relation FROM pg_inherits
            WHERE inhparent='category_chat_messages'::regclass LOOP
            EXECUTE format('REVOKE UPDATE,DELETE,TRUNCATE ON %s FROM twitchcollector',child.relation);
        END LOOP;
    END IF;
END $$;
