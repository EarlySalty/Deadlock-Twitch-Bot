-- B9/B4-024: Analytics-Schema-Typdrift aus der F1-Baseline reparieren.
--
-- Die Live-DB ist bereits auf dem Rust-Vertrag (bigint/timestamptz/float8).
-- Diese Migration ist deshalb strikt guarded: jede Spalte wird nur geaendert,
-- wenn ihr aktueller Typ vom Zieltyp abweicht.
--
-- Timescale-Befund: die hier betroffenen Tabellen werden in den kanonischen
-- Rust-Migrationen nicht per create_hypertable angelegt. Die einzige
-- Hypertable-Migration betrifft twitch_observability_events. Auf abweichenden
-- Zielsystemen mit manuell hypertabellierten Analytics-Tabellen koennen ALTER
-- TYPEs Timescale-/Compression-Einschraenkungen und laengere Locks ausloesen.
--
-- FK/PK-Reihenfolge: FKs auf twitch_stream_sessions(id) werden nur bei Bedarf
-- temporaer entfernt und mit ihrer bestehenden Definition wieder angelegt; die
-- Primaerschluessel werden vor Typaenderungen ihrer Schluesselspalten analog
-- temporaer entfernt.

DO $$
DECLARE
    target RECORD;
    stream_id_needs_fix BOOLEAN;
    raid_target_needs_fix BOOLEAN;
    viewers_pk_needs_fix BOOLEAN;
    stream_pk_needs_fix BOOLEAN;
    fk_cycle_needs_fix BOOLEAN;
BEGIN
    SELECT EXISTS (
        SELECT 1
          FROM pg_attribute
         WHERE attrelid = to_regclass('public.twitch_stream_sessions')
           AND attname = 'id'
           AND NOT attisdropped
           AND atttypid <> 'int8'::regtype
    ) INTO stream_id_needs_fix;

    SELECT EXISTS (
        SELECT 1
          FROM pg_attribute
         WHERE attrelid = to_regclass('public.twitch_raid_retention')
           AND attname = 'target_session_id'
           AND NOT attisdropped
           AND atttypid <> 'int8'::regtype
    ) INTO raid_target_needs_fix;

    SELECT EXISTS (
        SELECT 1
          FROM pg_attribute
         WHERE attrelid = to_regclass('public.twitch_session_viewers')
           AND attname = 'session_id'
           AND NOT attisdropped
           AND atttypid <> 'int8'::regtype
    ) OR EXISTS (
        SELECT 1
          FROM pg_attribute
         WHERE attrelid = to_regclass('public.twitch_session_viewers')
           AND attname = 'ts_utc'
           AND NOT attisdropped
           AND atttypid <> 'timestamptz'::regtype
    ) INTO viewers_pk_needs_fix;

    stream_pk_needs_fix := stream_id_needs_fix;
    fk_cycle_needs_fix := stream_id_needs_fix OR raid_target_needs_fix;

    IF fk_cycle_needs_fix THEN
        CREATE TEMP TABLE IF NOT EXISTS analytics_schema_type_fix_fks (
            table_schema TEXT NOT NULL,
            table_name TEXT NOT NULL,
            constraint_name TEXT NOT NULL,
            constraint_def TEXT NOT NULL
        ) ON COMMIT DROP;

        TRUNCATE analytics_schema_type_fix_fks;

        INSERT INTO analytics_schema_type_fix_fks
            (table_schema, table_name, constraint_name, constraint_def)
        SELECT n.nspname,
               rel.relname,
               c.conname,
               pg_get_constraintdef(c.oid)
          FROM pg_constraint c
          JOIN pg_class rel ON rel.oid = c.conrelid
          JOIN pg_namespace n ON n.oid = rel.relnamespace
         WHERE c.contype = 'f'
           AND c.confrelid = to_regclass('public.twitch_stream_sessions')
           AND (
               SELECT a.attnum
                 FROM pg_attribute a
                WHERE a.attrelid = c.confrelid
                  AND a.attname = 'id'
                  AND NOT a.attisdropped
           ) = ANY (c.confkey);

        FOR target IN
            SELECT * FROM analytics_schema_type_fix_fks
        LOOP
            EXECUTE format(
                'ALTER TABLE %I.%I DROP CONSTRAINT IF EXISTS %I',
                target.table_schema,
                target.table_name,
                target.constraint_name
            );
        END LOOP;
    END IF;

    IF stream_pk_needs_fix THEN
        CREATE TEMP TABLE IF NOT EXISTS analytics_schema_type_fix_stream_pk (
            table_schema TEXT NOT NULL,
            table_name TEXT NOT NULL,
            constraint_name TEXT NOT NULL,
            constraint_def TEXT NOT NULL
        ) ON COMMIT DROP;

        TRUNCATE analytics_schema_type_fix_stream_pk;

        INSERT INTO analytics_schema_type_fix_stream_pk
            (table_schema, table_name, constraint_name, constraint_def)
        SELECT n.nspname,
               rel.relname,
               c.conname,
               pg_get_constraintdef(c.oid)
          FROM pg_constraint c
          JOIN pg_class rel ON rel.oid = c.conrelid
          JOIN pg_namespace n ON n.oid = rel.relnamespace
         WHERE c.contype = 'p'
           AND c.conrelid = to_regclass('public.twitch_stream_sessions')
           AND c.conname = 'twitch_stream_sessions_pkey';

        FOR target IN
            SELECT * FROM analytics_schema_type_fix_stream_pk
        LOOP
            EXECUTE format(
                'ALTER TABLE %I.%I DROP CONSTRAINT IF EXISTS %I',
                target.table_schema,
                target.table_name,
                target.constraint_name
            );
        END LOOP;

        ALTER TABLE public.twitch_stream_sessions
            ALTER COLUMN id DROP DEFAULT,
            ALTER COLUMN id TYPE bigint USING id::bigint;

        IF to_regclass('public.twitch_stream_sessions_id_seq') IS NOT NULL THEN
            ALTER SEQUENCE public.twitch_stream_sessions_id_seq AS bigint;
            ALTER TABLE public.twitch_stream_sessions
                ALTER COLUMN id SET DEFAULT nextval('public.twitch_stream_sessions_id_seq'::regclass);
        END IF;

        FOR target IN
            SELECT * FROM analytics_schema_type_fix_stream_pk
        LOOP
            EXECUTE format(
                'ALTER TABLE %I.%I ADD CONSTRAINT %I %s',
                target.table_schema,
                target.table_name,
                target.constraint_name,
                target.constraint_def
            );
        END LOOP;
    ELSIF EXISTS (
        SELECT 1
          FROM pg_sequence
         WHERE seqrelid = to_regclass('public.twitch_stream_sessions_id_seq')
           AND seqtypid <> 'int8'::regtype
    ) THEN
        ALTER SEQUENCE public.twitch_stream_sessions_id_seq AS bigint;
    END IF;

    IF EXISTS (
        SELECT 1
          FROM pg_attribute
         WHERE attrelid = to_regclass('public.twitch_stream_sessions')
           AND attname = 'avg_viewers'
           AND NOT attisdropped
           AND atttypid <> 'float8'::regtype
    ) THEN
        ALTER TABLE public.twitch_stream_sessions
            ALTER COLUMN avg_viewers DROP DEFAULT,
            ALTER COLUMN avg_viewers TYPE double precision USING avg_viewers::double precision,
            ALTER COLUMN avg_viewers SET DEFAULT 0;
    END IF;

    IF viewers_pk_needs_fix THEN
        CREATE TEMP TABLE IF NOT EXISTS analytics_schema_type_fix_viewers_pk (
            table_schema TEXT NOT NULL,
            table_name TEXT NOT NULL,
            constraint_name TEXT NOT NULL,
            constraint_def TEXT NOT NULL
        ) ON COMMIT DROP;

        TRUNCATE analytics_schema_type_fix_viewers_pk;

        INSERT INTO analytics_schema_type_fix_viewers_pk
            (table_schema, table_name, constraint_name, constraint_def)
        SELECT n.nspname,
               rel.relname,
               c.conname,
               pg_get_constraintdef(c.oid)
          FROM pg_constraint c
          JOIN pg_class rel ON rel.oid = c.conrelid
          JOIN pg_namespace n ON n.oid = rel.relnamespace
         WHERE c.contype = 'p'
           AND c.conrelid = to_regclass('public.twitch_session_viewers')
           AND c.conname = 'twitch_session_viewers_pkey';

        FOR target IN
            SELECT * FROM analytics_schema_type_fix_viewers_pk
        LOOP
            EXECUTE format(
                'ALTER TABLE %I.%I DROP CONSTRAINT IF EXISTS %I',
                target.table_schema,
                target.table_name,
                target.constraint_name
            );
        END LOOP;

        IF EXISTS (
            SELECT 1
              FROM pg_attribute
             WHERE attrelid = to_regclass('public.twitch_session_viewers')
               AND attname = 'session_id'
               AND NOT attisdropped
               AND atttypid <> 'int8'::regtype
        ) THEN
            ALTER TABLE public.twitch_session_viewers
                ALTER COLUMN session_id TYPE bigint USING session_id::bigint;
        END IF;

        IF EXISTS (
            SELECT 1
              FROM pg_attribute
             WHERE attrelid = to_regclass('public.twitch_session_viewers')
               AND attname = 'ts_utc'
               AND NOT attisdropped
               AND atttypid <> 'timestamptz'::regtype
        ) THEN
            ALTER TABLE public.twitch_session_viewers
                ALTER COLUMN ts_utc TYPE timestamp with time zone USING ts_utc::timestamptz;
        END IF;

        FOR target IN
            SELECT * FROM analytics_schema_type_fix_viewers_pk
        LOOP
            EXECUTE format(
                'ALTER TABLE %I.%I ADD CONSTRAINT %I %s',
                target.table_schema,
                target.table_name,
                target.constraint_name,
                target.constraint_def
            );
        END LOOP;
    END IF;

    IF EXISTS (
        SELECT 1
          FROM pg_attribute
         WHERE attrelid = to_regclass('public.twitch_chat_messages')
           AND attname = 'session_id'
           AND NOT attisdropped
           AND atttypid <> 'int8'::regtype
    ) THEN
        ALTER TABLE public.twitch_chat_messages
            ALTER COLUMN session_id TYPE bigint USING session_id::bigint;
    END IF;

    IF EXISTS (
        SELECT 1
          FROM pg_attribute
         WHERE attrelid = to_regclass('public.twitch_chat_messages')
           AND attname = 'message_ts'
           AND NOT attisdropped
           AND atttypid <> 'timestamptz'::regtype
    ) THEN
        ALTER TABLE public.twitch_chat_messages
            ALTER COLUMN message_ts TYPE timestamp with time zone USING message_ts::timestamptz;
    END IF;

    IF raid_target_needs_fix THEN
        ALTER TABLE public.twitch_raid_retention
            ALTER COLUMN target_session_id TYPE bigint USING target_session_id::bigint;
    END IF;

    IF fk_cycle_needs_fix THEN
        FOR target IN
            SELECT * FROM analytics_schema_type_fix_fks
        LOOP
            EXECUTE format(
                'ALTER TABLE %I.%I ADD CONSTRAINT %I %s',
                target.table_schema,
                target.table_name,
                target.constraint_name,
                target.constraint_def
            );
        END LOOP;
    END IF;
END $$;
