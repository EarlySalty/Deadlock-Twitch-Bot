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

-- Lesbare Einstellungen ohne neue Schreibrechte auf fremde Daten.
DO $$
DECLARE role_name text;
BEGIN
    FOREACH role_name IN ARRAY ARRAY['twitchcollector','twitchdash'] LOOP
        IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname=role_name) THEN
            EXECUTE format('GRANT SELECT ON category_collector_config TO %I', role_name);
        END IF;
    END LOOP;
END $$;

-- Der Laufzeitnutzer bekommt keine freie DELETE-Berechtigung auf Rohdaten.
-- Nur explizite Moderationsziele werden durch diese eng begrenzte Funktion bearbeitet.
CREATE FUNCTION category_redact_chat_event(room_id text, message_id text, user_id text)
RETURNS bigint LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
    WITH removed AS (
        DELETE FROM public.category_chat_messages AS m
        WHERE nullif(btrim(room_id),'') IS NOT NULL AND (
            (nullif(btrim(message_id),'') IS NOT NULL AND (
                (m.room_user_id=room_id AND m.message_id=category_redact_chat_event.message_id)
                OR (m.tags->>'source-room-id'=room_id AND m.tags->>'source-id'=category_redact_chat_event.message_id)
            )) OR (
                message_id IS NULL AND nullif(btrim(user_id),'') IS NOT NULL
                AND m.room_user_id=room_id AND m.chatter_user_id=category_redact_chat_event.user_id
                AND m.sent_at >= COALESCE((SELECT s.started_at FROM public.category_stream_snapshots AS s
                    WHERE s.user_id=room_id ORDER BY s.snapshot_at DESC LIMIT 1),now())
            )
        ) RETURNING sent_at,room_user_id,detected_lang
    ), dirty AS (
        INSERT INTO public.category_chat_dirty
        SELECT DISTINCT date_trunc('hour',sent_at),room_user_id,detected_lang FROM removed
        ON CONFLICT DO NOTHING
    ) SELECT count(*)::bigint FROM removed;
$$;
REVOKE ALL ON FUNCTION category_redact_chat_event(text,text,text) FROM PUBLIC;
