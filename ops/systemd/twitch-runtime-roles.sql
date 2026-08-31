\set ON_ERROR_STOP on

-- Lokale Peer-Logins: kein Passwort, kein Superuser, kein DDL. Unix-Konto und
-- PostgreSQL-Rolle tragen absichtlich denselben Namen.
DO $rollen$
BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'twitchbot') THEN
        CREATE ROLE twitchbot LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE
            NOREPLICATION NOBYPASSRLS;
    END IF;
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'twitchdash') THEN
        CREATE ROLE twitchdash LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE
            NOREPLICATION NOBYPASSRLS;
    END IF;
END
$rollen$;

ALTER ROLE twitchbot NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
ALTER ROLE twitchdash NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
ALTER ROLE twitchbot PASSWORD NULL;
ALTER ROLE twitchdash PASSWORD NULL;

DO $keine_rollenerbschaft$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM pg_auth_members memberships
        JOIN pg_roles members ON members.oid = memberships.member
        WHERE members.rolname IN ('twitchbot', 'twitchdash')
    ) THEN
        RAISE EXCEPTION 'Twitch-Laufzeitrolle erbt unerwartet eine andere PostgreSQL-Rolle';
    END IF;
END
$keine_rollenerbschaft$;

ALTER ROLE twitchbot IN DATABASE twitch_analytics SET search_path = public, pg_catalog;
ALTER ROLE twitchdash IN DATABASE twitch_analytics SET search_path = public, pg_catalog;

GRANT CONNECT ON DATABASE twitch_analytics TO twitchbot, twitchdash;
GRANT USAGE ON SCHEMA public TO twitchbot, twitchdash;
REVOKE CREATE ON SCHEMA public FROM twitchbot, twitchdash;

GRANT SELECT, INSERT, UPDATE, DELETE
    ON ALL TABLES IN SCHEMA public TO twitchbot, twitchdash;
GRANT USAGE, SELECT
    ON ALL SEQUENCES IN SCHEMA public TO twitchbot, twitchdash;

-- Laufzeitprozesse dürfen weder den Migrationsstand noch Sicherungstabellen
-- manipulieren. Die fachliche Spalte schema_version ist davon nicht betroffen.
DO $optional_revoke$
BEGIN
    IF to_regclass('public._sqlx_migrations') IS NOT NULL THEN
        REVOKE ALL ON TABLE public._sqlx_migrations FROM twitchbot, twitchdash;
    END IF;
    IF to_regclass('public.tb_schema_ownership') IS NOT NULL THEN
        REVOKE ALL ON TABLE public.tb_schema_ownership FROM twitchbot, twitchdash;
    END IF;
    IF to_regclass('public.schema_version') IS NOT NULL THEN
        REVOKE ALL ON TABLE public.schema_version FROM twitchbot, twitchdash;
    END IF;
    IF to_regclass('public.twitch_stream_sessions_duration_repair_backup') IS NOT NULL THEN
        REVOKE ALL ON TABLE public.twitch_stream_sessions_duration_repair_backup
            FROM twitchbot, twitchdash;
    END IF;
END
$optional_revoke$;

-- Alle künftigen, vom dedizierten Migrator (postgres) angelegten Objekte sind
-- sofort für DML verfügbar, aber weiterhin nicht im Eigentum der Laufzeitrollen.
ALTER DEFAULT PRIVILEGES FOR ROLE postgres IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO twitchbot, twitchdash;
ALTER DEFAULT PRIVILEGES FOR ROLE postgres IN SCHEMA public
    GRANT USAGE, SELECT ON SEQUENCES TO twitchbot, twitchdash;
