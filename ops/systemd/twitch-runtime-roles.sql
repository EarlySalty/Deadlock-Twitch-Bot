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

ALTER ROLE twitchbot NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
ALTER ROLE twitchdash NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
ALTER ROLE twitchbot PASSWORD NULL;
ALTER ROLE twitchdash PASSWORD NULL;
ALTER ROLE twitchbot RESET ALL;
ALTER ROLE twitchdash RESET ALL;

DO $keine_rollenerbschaft$
DECLARE
    membership record;
BEGIN
    FOR membership IN
        SELECT granted.rolname AS granted_role, members.rolname AS member_role
        FROM pg_auth_members memberships
        JOIN pg_roles granted ON granted.oid = memberships.roleid
        JOIN pg_roles members ON members.oid = memberships.member
        WHERE members.rolname IN ('twitchbot', 'twitchdash')
    LOOP
        EXECUTE format(
            'REVOKE %I FROM %I',
            membership.granted_role,
            membership.member_role
        );
    END LOOP;
END
$keine_rollenerbschaft$;

ALTER ROLE twitchbot IN DATABASE twitch_analytics SET search_path = public, pg_catalog;
ALTER ROLE twitchdash IN DATABASE twitch_analytics SET search_path = public, pg_catalog;

GRANT CONNECT ON DATABASE twitch_analytics TO twitchbot, twitchdash;
GRANT USAGE ON SCHEMA public TO twitchbot, twitchdash;
REVOKE CREATE ON SCHEMA public FROM twitchbot, twitchdash;

-- Konvergent zuerst alles entziehen, dann die fachliche Matrix neu aufbauen.
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM twitchbot, twitchdash;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public FROM twitchbot, twitchdash;
GRANT SELECT, INSERT, UPDATE, DELETE
    ON ALL TABLES IN SCHEMA public TO twitchbot, twitchdash;
GRANT USAGE, SELECT
    ON ALL SEQUENCES IN SCHEMA public TO twitchbot, twitchdash;

-- Der Bot verarbeitet Twitch-Laufzeitdaten, aber keine Web-Sessions,
-- Admin-Audits, Affiliate-PII oder Zahlungsdaten. Das Dashboard besitzt diese
-- Rechte, weil genau dort Login-, Admin-, Affiliate- und Billing-Flows leben.
DO $dienstmatrix$
DECLARE
    private_table text;
    ingest_table text;
BEGIN
    FOREACH private_table IN ARRAY ARRAY[
        'dashboard_sessions',
        'dashboard_admin_audit_events',
        'twitch_admin_roles',
        'affiliate_accounts',
        'affiliate_commissions',
        'affiliate_gutschrift_counter',
        'affiliate_gutschriften',
        'affiliate_pii',
        'affiliate_streamer_claims',
        'twitch_billing_events',
        'twitch_billing_profiles',
        'twitch_billing_subscriptions'
    ]
    LOOP
        IF to_regclass(format('public.%I', private_table)) IS NOT NULL THEN
            EXECUTE format(
                'REVOKE ALL PRIVILEGES ON TABLE public.%I FROM twitchbot',
                private_table
            );
        END IF;
    END LOOP;

    -- EventSub-Transporttabellen werden ausschließlich vom Bot geschrieben.
    -- Das Dashboard darf den Kapazitätsstand und Fehlerzustand weiterhin lesen.
    FOREACH ingest_table IN ARRAY ARRAY[
        'twitch_eventsub_bridge_dead_letter',
        'twitch_eventsub_bridge_outbox',
        'twitch_eventsub_capacity_snapshot',
        'twitch_eventsub_processing_dead_letter',
        'twitch_eventsub_processing_inbox'
    ]
    LOOP
        IF to_regclass(format('public.%I', ingest_table)) IS NOT NULL THEN
            EXECUTE format(
                'REVOKE INSERT, UPDATE, DELETE ON TABLE public.%I FROM twitchdash',
                ingest_table
            );
        END IF;
    END LOOP;
END
$dienstmatrix$;

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

-- Neue Tabellen starten ohne Laufzeitrechte. Nach jeder Migration baut der
-- ExecStartPost diese Matrix anhand der nun vorhandenen Tabellen neu auf.
ALTER DEFAULT PRIVILEGES FOR ROLE postgres IN SCHEMA public
    REVOKE ALL ON TABLES FROM twitchbot, twitchdash;
ALTER DEFAULT PRIVILEGES FOR ROLE postgres IN SCHEMA public
    REVOKE ALL ON SEQUENCES FROM twitchbot, twitchdash;
